// A chat's live Claude session (docs/chat.md 4.5.6, 4.7): one CLI process, one `query()` fed
// by a queue of prompts for as long as the session lives. The driver iterates the query
// itself, not its raw generator, whose `return()` would wait behind a pending read (T3
// adapter pitfall 1). A stop interrupts the turn and keeps the process when the turn's result
// comes, closing it when none comes in time (7); what the CLI wrote on stderr before its
// first frame goes with an error, as a refusal may be told only there (45). A settings change
// the live process can't take is said at once, so the actor reopens the session before its
// next turn; one the process failed to take half-way restarts it, resumed, before the next.

import type {
  CanUseTool,
  Options,
  PermissionMode,
  Query,
  SDKMessage,
  SDKSystemMessage,
  SDKUserMessage,
  SlashCommand,
} from "@anthropic-ai/claude-agent-sdk";

import { errorMessage, log } from "../../log";
import type { Effort, Mode } from "../../protocol/model";
import { Queue } from "../../queue";
import {
  processFailure,
  type ProviderSession,
  type SessionChange,
  type SessionSink,
  type SessionSpec,
  type TurnInput,
} from "../provider";
import { answerTool, type Asking } from "./approvals";
import type { Env } from "./environment";
import { Frames } from "./frames";
import { asks, modeOf, PERMISSION, sessionOptions } from "./options";
import { userMessage } from "./prompts";
import { Groups } from "./spawn";
import { Lifecycle, Turns } from "./turns";

/** What the driver uses of a query; the SDK's `Query` is one. */
export type Conversation = AsyncIterable<SDKMessage> & Pick<
  Query,
  | "interrupt"
  | "setPermissionMode"
  | "setModel"
  | "applyFlagSettings"
  | "stopTask"
  | "supportedCommands"
  | "initializationResult"
  | "rewindFiles"
  | "close"
>;

/** Starts a query, as the SDK's `query` does. */
export type Start = (params: {
  readonly prompt: string | AsyncIterable<SDKUserMessage>;
  readonly options: Options;
}) => Conversation;

/** Where and how a session's CLI runs, found when the session opens. */
export interface Launch {
  readonly executable: string;
  readonly env: Env;
  /** Whether the conversation has turns on disk to resume (T3 adapter pitfalls 9, 10). */
  readonly resumable: boolean;
}

export interface SessionDeps {
  readonly launch: (spec: SessionSpec) => Promise<Launch>;
  readonly start: Start;
  /** Tells the catalog what slash commands the session's folder has. */
  readonly commands: (cwd: string, commands: readonly SlashCommand[]) => void;
  /** How long a stopped turn has to end before its process is closed. */
  readonly stopMs: number;
}

/** How much of stderr is kept until the first frame. */
const STDERR_TAIL = 2048;
const FRESH = "The earlier conversation wasn't available, so Claude starts this one fresh.";
const NO_TURN = "No turn of this chat is running.";

/** What a settings change asks of the live process, decided when the change is made. */
interface Applying {
  readonly model?: string;
  readonly effort?: Effort;
  readonly fast?: boolean;
  readonly mode?: PermissionMode;
}

export class ClaudeSession implements ProviderSession {
  readonly #sink: SessionSink;
  readonly #deps: SessionDeps;
  #prompts = new Queue<SDKUserMessage>();
  readonly #frames: Frames;
  readonly #turns: Turns;
  /** Resolves once the CLI started, or failed to. */
  readonly #opening: Promise<void>;
  #spec: SessionSpec;
  #chain: Promise<void> = Promise.resolve();
  #conversation: Conversation | undefined;
  #groups: Groups | undefined;
  #closing = false;
  #heard = false;
  #stderr = "";
  #listed = false;
  #stopping: Timer | undefined;
  /** Whether the conversation has turns, which a restarted process resumes. */
  #used: boolean;
  /** Whether a change failed half-way, so the process restarts before the next turn. */
  #stale = false;
  /**
   * What the process was opened with, which decides the modes it can move to: whether it
   * asks the person, and may bypass permissions; `mode: null` when Claude's settings chose.
   */
  #opened: { readonly plan: boolean; readonly mode: Mode | null };
  /** The mode Claude's settings chose, when the session was opened with none. */
  #settled: PermissionMode | undefined;
  /** The mode in force, as the CLI last said. */
  #mode: PermissionMode | undefined;

  constructor(spec: SessionSpec, sink: SessionSink, deps: SessionDeps) {
    this.#spec = spec;
    this.#opened = { plan: spec.plan, mode: spec.permission };
    this.#used = spec.resume;
    this.#sink = sink;
    this.#deps = deps;
    this.#frames = new Frames((event) => {
      sink.event(event);
    }, deps.stopMs);
    this.#turns = new Turns((event) => {
      // From now on the conversation has that turn: a restart goes on from it.
      if (event.type === "turn.started") {
        this.#spec = { ...this.#spec, resumeAt: null };
      }
      sink.event(event);
    }, this.#frames, () => {
      clearTimeout(this.#stopping);
    });
    this.#opening = this.#open(spec.resume);
  }

  start(turn: TurnInput): void {
    if (this.#turns.running) {
      throw new Error("A turn of this session is running already.");
    }
    const { promptId } = turn;
    this.#turns.start(promptId, turn.compact);
    this.#then(async () => {
      await this.#opening;
      if (this.#stale) {
        await this.#restart();
      }
      const conversation = this.#conversation;
      if (conversation === undefined || !this.#turns.runs(promptId)) {
        return;
      }
      await this.#leavePlan(conversation);
      const message = await userMessage(turn);
      if (this.#turns.runs(promptId)) {
        this.#turns.sent(promptId);
        this.#used = true;
        this.#prompts.push(message);
      }
    }, (error) => {
      log.warn("A turn's prompt couldn't be sent", error);
      this.#turns.fail(promptId, processFailure(errorMessage(error)));
    });
  }

  steer(turn: TurnInput): boolean {
    if (!this.#turns.steer(turn.promptId)) {
      return false;
    }
    this.#then(async () => {
      this.#prompts.push(await userMessage(turn, "next"));
    }, (error) => {
      log.warn("A message couldn't join the turn", error);
      this.#turns.unsteer(turn.promptId);
    });
    return true;
  }

  async interrupt(): Promise<void> {
    const prompts = new Set(this.#turns.prompts());
    const conversation = this.#conversation;
    if (!this.#turns.interrupt() || conversation === undefined) {
      return;
    }
    clearTimeout(this.#stopping);
    this.#stopping = setTimeout(() => {
      log.warn("Claude didn't stop its turn in time; its process is closed.");
      this.#end();
    }, this.#deps.stopMs);
    const receipt = await conversation.interrupt();
    // A prompt of the turn still queued would run after it: the process goes instead.
    if (receipt?.still_queued.some((id) => prompts.has(id)) === true) {
      log.info("A stopped turn's message was still queued; Claude's process is closed.");
      this.#end();
    }
  }

  stopSubagent(key: string): void {
    const conversation = this.#conversation;
    this.#frames.subagents.stop(key, async (taskId) => await conversation?.stopTask(taskId));
  }

  configure(change: SessionChange): "applied" | "reopen" {
    const next: SessionSpec = { ...this.#spec, ...change };
    const conversation = this.#conversation;
    if (conversation === undefined || this.#stale) {
      // Not opened yet, or restarting before the next turn: it opens with the change.
      this.#spec = next;
      return "applied";
    }
    const applying = this.#applying(change, next);
    if (applying === undefined) {
      return "reopen";
    }
    this.#spec = next;
    // In the order asked, before the next turn's prompt, which waits behind it.
    this.#then(async () => {
      await this.#apply(conversation, applying);
    }, (error) => {
      // A change that failed half-way is made whole by a new process (pitfall 27).
      log.warn("Claude's session didn't take a settings change; it restarts before the next turn",
        error);
      this.#stale = true;
    });
    return "applied";
  }

  kill(): void {
    this.#closing = true;
    this.#prompts.end();
    this.#groups?.kill();
    this.#conversation?.close();
  }

  async close(): Promise<void> {
    this.#closing = true;
    this.#prompts.end();
    this.#conversation?.close();
    await this.#groups?.terminate();
  }

  /** Starts the CLI, resuming the conversation when it has turns; a failure ends the session. */
  async #open(resume: boolean): Promise<void> {
    try {
      await this.#launch(resume);
    } catch (error) {
      log.warn("Claude's session didn't open", error);
      this.#ended(errorMessage(error));
    }
  }

  async #launch(resume: boolean): Promise<void> {
    const launch = await this.#deps.launch({ ...this.#spec, resume });
    if (this.#closing) {
      return;
    }
    const spec = this.#spec;
    this.#opened = { plan: spec.plan, mode: spec.permission };
    if (resume && !launch.resumable) {
      this.#sink.event({ type: "notice", level: "warning", text: FRESH });
    }
    const groups = new Groups("Claude's session", (text) => {
      this.#heardStderr(text);
    });
    this.#groups = groups;
    const options = sessionOptions(spec, {
      executable: launch.executable,
      env: launch.env,
      cwd: spec.cwd,
      spawn: groups.spawn,
    }, {
      resume: resume && launch.resumable,
      canUseTool: this.#canUseTool,
    });
    const conversation = this.#deps.start({ prompt: this.#prompts, options });
    this.#conversation = conversation;
    this.#read(conversation).catch((error: unknown) => {
      log.error("Reading Claude's session failed", error);
    });
  }

  /** Ends the process that failed to take a change, and starts one with the chat's settings. */
  async #restart(): Promise<void> {
    this.#stale = false;
    log.info("Claude's process restarts, resumed, with the chat's settings.");
    const conversation = this.#conversation;
    const groups = this.#groups;
    // The process that's ending is no longer the session's: its end isn't the session's.
    this.#conversation = undefined;
    this.#groups = undefined;
    this.#prompts.end();
    this.#prompts = new Queue();
    conversation?.close();
    await groups?.terminate();
    this.#heard = false;
    this.#stderr = "";
    this.#settled = undefined;
    await this.#open(this.#used);
  }

  async #read(conversation: Conversation): Promise<void> {
    let why: string | null = null;
    try {
      for await (const frame of conversation) {
        this.#heard = true;
        this.#frame(frame);
      }
    } catch (error) {
      why = this.#closing ? null : this.#why(error);
    }
    if (conversation !== this.#conversation) {
      log.debug("A process Claude's session restarted away from ended.");
      return;
    }
    if (why === null && !this.#heard && !this.#closing && this.#stderr.trim() !== "") {
      why = this.#stderr.trim();
    }
    this.#ended(why);
  }

  #frame(frame: SDKMessage): void {
    try {
      // The one frame outside the SDK's union that the driver reads, seen by its type first.
      const { type }: { readonly type: string } = frame;
      const lifecycle = type === "command_lifecycle" ? Lifecycle.safeParse(frame) : undefined;
      if (lifecycle?.success === true) {
        this.#turns.lifecycle(lifecycle.data);
        return;
      }
      if (frame.type === "system" && frame.subtype === "init") {
        this.#init(frame);
      } else if (frame.type === "system" && frame.subtype === "status") {
        this.#mode = frame.permissionMode ?? this.#mode;
      }
      this.#turns.frame(frame);
    } catch (error) {
      log.error("A frame of Claude's couldn't be read", error);
    }
  }

  /** The CLI says, each turn, what the session runs with. */
  #init(frame: SDKSystemMessage): void {
    this.#mode = frame.permissionMode;
    if (!this.#opened.plan && this.#opened.mode === null) {
      // Opened with no mode, the session runs in the one Claude's settings chose.
      this.#settled ??= frame.permissionMode;
    }
    this.#frames.init(frame);
    this.#sink.event({ type: "session.ready", mode: modeOf(frame.permissionMode) });
    const conversation = this.#conversation;
    if (!this.#listed && conversation !== undefined) {
      this.#listed = true;
      const { cwd } = this.#spec;
      conversation.supportedCommands().then((commands) => {
        this.#deps.commands(cwd, commands);
      }, (error: unknown) => {
        log.warn("Claude's slash commands couldn't be read", error);
      });
    }
  }

  /** The process ended, or never started. */
  #ended(error: string | null): void {
    clearTimeout(this.#stopping);
    this.#conversation = undefined;
    this.#prompts.end();
    this.#turns.abandon();
    this.#sink.event({ type: "session.ended", error });
  }

  /** Closes the process, whose end the actor hears as the session's. */
  #end(): void {
    this.close().catch((error: unknown) => {
      log.warn("Ending Claude's process failed", error);
    });
  }

  /**
   * What the live process is to be asked for `change`, or `undefined` when only a new one can
   * take it: no model or no effort means what a new process reads from the person's settings
   * (pitfall 28), and a mode `#target` can't reach needs a process opened with it.
   */
  #applying(change: SessionChange, next: SessionSpec): Applying | undefined {
    if ((change.model !== undefined && next.model === null)
      || (change.effort !== undefined && next.effort === null)) {
      return undefined;
    }
    const moded = change.permission !== undefined || change.plan !== undefined;
    const mode = moded ? this.#target(next) : undefined;
    if (moded && mode === undefined) {
      return undefined;
    }
    return {
      ...(change.model === undefined || next.model === null ? {} : { model: next.model }),
      ...(change.effort === undefined || next.effort === null ? {} : { effort: next.effort }),
      ...(change.fast === undefined ? {} : { fast: next.fast }),
      ...(mode === undefined ? {} : { mode }),
    };
  }

  async #apply(conversation: Conversation, applying: Applying): Promise<void> {
    if (applying.model !== undefined) {
      await conversation.setModel(applying.model);
    }
    if (applying.effort !== undefined) {
      await conversation.applyFlagSettings({ effortLevel: applying.effort });
    }
    if (applying.fast !== undefined) {
      await conversation.applyFlagSettings({ fastMode: applying.fast ? true : null });
    }
    if (applying.mode !== undefined) {
      await conversation.setPermissionMode(applying.mode);
      this.#mode = applying.mode;
    }
  }

  /**
   * A denied ExitPlanMode leaves the CLI in plan mode; a turn out of plan mode restores the
   * chat's mode first (T3 adapter pitfall 11).
   */
  async #leavePlan(conversation: Conversation): Promise<void> {
    if (this.#spec.plan || this.#mode !== "plan") {
      return;
    }
    const target = this.#target(this.#spec) ?? "default";
    try {
      await conversation.setPermissionMode(target);
      this.#mode = target;
    } catch (error) {
      log.warn("Claude's session didn't leave plan mode", error);
    }
  }

  /**
   * The mode `spec` asks of the live session; `undefined` when only a new process can take it:
   * Full access needs the process to have been allowed it, every other mode a process that
   * asks the person, and no mode at all whatever Claude's settings would choose.
   */
  #target(spec: SessionSpec): PermissionMode | undefined {
    const asking = asks(this.#opened.plan, this.#opened.mode);
    if (spec.plan) {
      return asking ? "plan" : undefined;
    }
    const mode = spec.permission;
    if (mode === null) {
      return this.#settled === undefined || this.#settled === "plan" ? undefined : this.#settled;
    }
    if (mode === "full_access") {
      return this.#opened.mode === "full_access" ? PERMISSION[mode] : undefined;
    }
    return asking ? PERMISSION[mode] : undefined;
  }

  readonly #canUseTool: CanUseTool = async (name, input, request) => {
    // A subagent asks with or without a turn, and says nothing of the turn if there's one.
    if (request.agentID !== undefined) {
      const asking = this.#frames.subagents.asking(request.agentID, this.#sink);
      return await answerTool(asking, name, input, request);
    }
    return this.#turns.permission()
      ? await answerTool(this.#asking, name, input, request)
      : { behavior: "deny", message: NO_TURN };
  };

  readonly #asking: Asking = {
    ask: async (payload, signal) => await this.#sink.ask(payload, signal, false),
    event: (event) => {
      this.#sink.event(event);
    },
    started: (toolUseId, name, input, kept) => {
      this.#frames.toolStarted(toolUseId, name, input, kept);
    },
    declined: (toolUseId) => {
      this.#frames.toolDeclined(toolUseId);
    },
  };

  #heardStderr(text: string): void {
    if (!this.#heard) {
      this.#stderr = `${this.#stderr}${text}`.slice(-STDERR_TAIL);
    }
  }

  /** What went wrong, with what the CLI said on stderr before it said anything else. */
  #why(caught: unknown): string {
    const message = errorMessage(caught);
    const said = this.#heard ? "" : this.#stderr.trim();
    return said === "" ? message : `${message}\n${said}`;
  }

  #then(work: () => Promise<void>, failed: (error: unknown) => void): void {
    this.#chain = this.#chain.then(work).catch(failed);
  }
}
