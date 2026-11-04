// Claude's subagents (docs/chat.md 4.5.10, 4.7). An Agent call starts one, and a frame that names
// the call as its parent is its own: it goes to its own timeline, whatever turn of the
// conversation runs by then, as one started in the background runs on past its turn. The call's
// result says only that it was launched, then; how it ended comes in a `task_notification`, the
// frames of its task saying what it does until then.

import { randomUUID } from "node:crypto";

import type { SDKMessage, SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

import { errorMessage, log } from "../../log";
import { MessageId, newId } from "../../protocol/ids";
import { Effort, type SubagentUsage } from "../../protocol/model";
import type {
  DriverEvent,
  ItemBody,
  ItemEvent,
  Outcome,
  SessionSink,
  SubagentChange,
  SubagentStart,
} from "../provider";
import type { Asking } from "./approvals";
import { resultText, settleResults, Steps } from "./steps";
import { Texts } from "./text";
import { fieldsOf, head, text, type ToolResult } from "./tools";

type System = Extract<SDKMessage, { type: "system" }>;
type TaskStarted = Extract<System, { subtype: "task_started" }>;
type TaskProgress = Extract<System, { subtype: "task_progress" }>;
type TaskUpdated = Extract<System, { subtype: "task_updated" }>;
type TaskNotification = Extract<System, { subtype: "task_notification" }>;

class Agent {
  readonly key: string;
  readonly steps: Steps;
  readonly texts: Texts;
  /** What it was asked, which a frame of its says again and needn't show twice. */
  readonly prompt: string;
  /** Its task's id, once its task said. */
  taskId: string | undefined;
  /** Whether it runs on past the turn that started it. */
  background = false;
  running = true;
  /** The report it handed back itself, which says more than what the CLI says of it. */
  report: string | undefined;
  /** The tasks it started that still run, which it can end its say on and wait for. */
  readonly tasks = new Set<string>();
  /** The CLI told of its task's end while it waits, and won't again if it's stopped as it is. */
  told = false;
  /** Ends it, should it not say it stopped when asked to. */
  stopping: Timer | undefined;

  constructor(key: string, steps: Steps, texts: Texts, started: SubagentStart) {
    this.key = key;
    this.steps = steps;
    this.texts = texts;
    this.prompt = started.prompt;
  }

  /** Whether it's waiting on a task of its own, its task told of as over by now. */
  waits(told: boolean): void {
    this.told = told;
  }

  /** It handed back `report`, which it ends on. */
  handedBack(report: string): void {
    this.report = report;
  }

  /** It's over: nothing of what it was doing is kept. */
  ended(): void {
    this.running = false;
    this.report = undefined;
    clearTimeout(this.stopping);
    this.steps.clear();
    this.texts.clear();
  }
}

/** The most of a subagent's report its row keeps. */
const SUMMARY_BOUND = 4 * 1024;
/** The tool a subagent ends on, whose message is its report to the agent that started it. */
const HANDBACK = "SubagentHandback";
/** How the CLI tells a subagent that a task of its ended, in words no one said. */
const NOTIFIED = "<task-notification>";
const WAITING = "Waiting on what it started";

/** What an Agent call's structured result says, of what its driver reads (`AgentOutput`). */
const Output = z.object({
  status: z.string().optional(),
  isAsync: z.boolean().optional(),
  resolvedModel: z.string().optional(),
  totalTokens: z.number().optional(),
  totalToolUseCount: z.number().optional(),
  content: z.array(z.object({ text: z.string().optional() })).optional(),
});
type Output = z.infer<typeof Output>;

export class Subagents {
  readonly #emit: (event: DriverEvent) => void;
  readonly #stopMs: number;
  /** Every subagent of the session so far, by its Agent call's id: one may be started again. */
  readonly #agents = new Map<string, Agent>();
  readonly #tasks = new Map<string, Agent>();

  constructor(emit: (event: DriverEvent) => void, stopMs: number) {
    this.#emit = emit;
    this.#stopMs = stopMs;
  }

  /** An Agent call was seen, in its block or its permission request. */
  start(key: string, input: unknown, parentKey: string | null): void {
    if (!this.#agents.has(key)) {
      this.#started(key, parentKey, called(input));
    }
  }

  /** Takes the result of call `key`, if it's an Agent call's. */
  settle(key: string, result: ToolResult): boolean {
    const agent = this.#agents.get(key);
    if (agent === undefined) {
      return false;
    }
    const parsed = Output.safeParse(result.structured);
    const output: Output = parsed.success ? parsed.data : {};
    if (output.resolvedModel !== undefined) {
      this.#changed(agent, { model: output.resolvedModel });
    }
    if (launched(output)) {
      agent.background = true;
    } else if (agent.running && (result.isError || !agent.background)) {
      const outcome = result.isError ? "failed" : "completed";
      this.#end(agent, outcome, agent.report ?? reported(output, result.text), usageOf(output));
    }
    return true;
  }

  /** Where subagent `taskId`'s requests go, its calls seen first in them being its own. */
  asking(taskId: string, sink: SessionSink): Asking {
    return {
      ask: async (payload, signal) => await sink.ask(payload, signal, true),
      event: sink.event,
      started: (toolUseId, name, input, kept) => {
        this.#tasks.get(taskId)?.steps.start(toolUseId, { name, input, kept });
      },
      declined: (toolUseId) => {
        this.decline(toolUseId);
      },
    };
  }

  /** The person or Claude's checks declined a call, which may be a subagent's. */
  decline(toolUseId: string): void {
    for (const agent of this.#agents.values()) {
      if (agent.running) {
        agent.steps.decline(toolUseId);
      }
    }
  }

  /** Whether `frame` is a subagent's, which is then taken care of. */
  hears(frame: SDKMessage): boolean {
    if (frame.type === "system") {
      return this.#task(frame);
    }
    if (!("parent_tool_use_id" in frame) || typeof frame.parent_tool_use_id !== "string") {
      return false;
    }
    const agent = this.#agents.get(frame.parent_tool_use_id);
    if (agent === undefined) {
      log.debug(`A frame came of a subagent not seen (${frame.parent_tool_use_id}).`);
    } else if (frame.type === "assistant") {
      agent.texts.snapshot(frame.message);
      for (const block of frame.message.content) {
        if (block.type === "tool_use") {
          this.#called(agent, block.id, block.name, block.input);
        }
      }
    } else if (frame.type === "user" && !("isReplay" in frame)) {
      this.#heard(agent, frame);
    }
    return true;
  }

  /** The turn ended as `outcome`: a subagent it was waiting on won't be heard of again. */
  turnEnded(outcome: Outcome): void {
    for (const agent of this.#agents.values()) {
      if (agent.running && !agent.background) {
        const left = outcome === "completed" ? "failed" : "interrupted";
        this.#end(agent, left, null, null);
      }
    }
  }

  /** Asks subagent `key` to stop, and ends it if it doesn't say it did in time. */
  stop(key: string, stopTask: (taskId: string) => Promise<void>): void {
    const agent = this.#agents.get(key);
    if (agent?.running !== true) {
      return;
    }
    if (agent.taskId === undefined) {
      this.#end(agent, "interrupted", null, null);
      return;
    }
    stopTask(agent.taskId).then(() => {
      if (agent.running && agent.told) {
        this.#end(agent, "interrupted", null, null);
      }
    }).catch((error: unknown) => {
      log.warn(`Claude didn't take stopping a subagent: ${errorMessage(error)}`);
    });
    clearTimeout(agent.stopping);
    agent.stopping = setTimeout(() => {
      if (agent.running) {
        log.warn("A subagent didn't say it stopped in time.");
        this.#end(agent, "interrupted", null, null);
      }
    }, this.#stopMs);
  }

  /** A call of its: the one that hands back its report is its answer, not a tool it used. */
  #called(agent: Agent, id: string, name: string, input: unknown): void {
    const report = name === HANDBACK ? text(fieldsOf(input), "message") : undefined;
    if (report === undefined) {
      agent.steps.start(id, { name, input });
      return;
    }
    agent.handedBack(report);
    this.#shown(agent, id, { kind: "assistant_text", payload: { text: report } });
  }

  /** What was said to it: the tools' results, and any words beyond what it was first asked. */
  #heard(agent: Agent, frame: SDKUserMessage): void {
    const { content } = frame.message;
    const said = resultText(content);
    if (said.trim() !== "" && said !== agent.prompt && !isNotice(frame, said)) {
      this.#shown(agent, frame.uuid ?? randomUUID(), {
        kind: "user_message",
        payload: { messageId: newId(MessageId), text: said, attachments: [], delivered: "turn" },
      });
    }
    if (typeof content !== "string") {
      settleResults(agent.steps, content, frame.tool_use_result);
    }
  }

  /** Something finished as it's first heard of, in its timeline. */
  #shown(agent: Agent, id: string, item: ItemBody): void {
    const event: ItemEvent = { type: "item.updated", key: id, item, status: "completed" };
    this.#emit({ type: "subagent.event", key: agent.key, event });
  }

  /** Takes a system frame of a task's, whichever task it's of. */
  #task(frame: System): boolean {
    if (frame.subtype === "task_started") {
      this.#taskStarted(frame);
    } else if (frame.subtype === "task_progress") {
      this.#taskProgress(frame);
    } else if (frame.subtype === "task_updated") {
      this.#taskUpdated(frame);
    } else if (frame.subtype === "task_notification") {
      this.#taskEnded(frame);
    } else {
      return false;
    }
    return true;
  }

  /** A task's start: of an Agent call seen, or of a subagent started again or by itself. */
  #taskStarted(frame: TaskStarted): void {
    const owner = frame.parent_task_id === undefined
      ? undefined
      : this.#tasks.get(frame.parent_task_id);
    if (frame.skip_transcript !== true) {
      owner?.tasks.add(frame.task_id);
    }
    if (!isAgent(frame)) {
      return;
    }
    const key = frame.tool_use_id ?? frame.task_id;
    const known = this.#agents.get(key);
    const agent = known?.running === true
      ? known
      : this.#started(key, owner?.key ?? null, {
          description: frame.description,
          agentType: frame.subagent_type ?? null,
          prompt: frame.prompt ?? "",
          model: null,
          effort: null,
        });
    agent.waits(false);
    agent.taskId = frame.task_id;
    agent.background ||= frame.is_backgrounded === true;
    this.#tasks.set(frame.task_id, agent);
  }

  #taskProgress(frame: TaskProgress): void {
    const agent = this.#tasks.get(frame.task_id);
    if (agent?.running === true) {
      const progress = frame.summary ?? frame.description;
      this.#changed(agent, { progress, usage: spent(frame.usage) });
    }
  }

  #taskUpdated(frame: TaskUpdated): void {
    const agent = this.#tasks.get(frame.task_id);
    const { is_backgrounded: background } = frame.patch;
    if (agent !== undefined && background !== undefined) {
      agent.background = background;
    }
  }

  /**
   * A task said how it ended. A subagent with a task of its own still running and nothing handed
   * back is waiting for that task, whatever this says: it goes on when the task ends.
   */
  #taskEnded(frame: TaskNotification): void {
    for (const owner of this.#agents.values()) {
      owner.tasks.delete(frame.task_id);
    }
    const agent = this.#tasks.get(frame.task_id);
    if (agent?.running !== true) {
      return;
    }
    const usage = frame.usage === undefined ? null : spent(frame.usage);
    if (frame.status === "completed" && agent.report === undefined && agent.tasks.size > 0) {
      agent.waits(true);
      this.#changed(agent, { progress: WAITING, usage });
      return;
    }
    const outcome = frame.status === "stopped" ? "interrupted" : frame.status;
    this.#end(agent, outcome, agent.report ?? frame.summary, usage);
  }

  #started(key: string, parentKey: string | null, subagent: SubagentStart): Agent {
    this.#emit({ type: "subagent.started", key, parentKey, subagent });
    const emit = (event: ItemEvent): void => {
      this.#emit({ type: "subagent.event", key, event });
    };
    const steps = new Steps(emit, this, key);
    steps.begin(key);
    const agent = new Agent(key, steps, new Texts(emit), subagent);
    this.#agents.set(key, agent);
    return agent;
  }

  #changed(agent: Agent, change: SubagentChange): void {
    this.#emit({ type: "subagent.changed", key: agent.key, change });
  }

  #end(agent: Agent, outcome: Outcome, said: string | null, usage: SubagentUsage | null): void {
    const summary = said === null || said.trim() === "" ? null : head(said.trim(), SUMMARY_BOUND);
    agent.ended();
    if (agent.taskId !== undefined) {
      this.#tasks.delete(agent.taskId);
    }
    this.#emit({ type: "subagent.ended", key: agent.key, outcome, summary, usage });
  }
}

/** What an Agent call's input asks for. */
function called(input: unknown): SubagentStart {
  const fields = fieldsOf(input);
  const effort = Effort.safeParse(text(fields, "effort"));
  return {
    description: text(fields, "description") ?? "Subagent",
    agentType: text(fields, "subagent_type") ?? null,
    prompt: text(fields, "prompt") ?? "",
    model: text(fields, "model") ?? null,
    effort: effort.success ? effort.data : null,
  };
}

/** Whether a task is a subagent's rather than a shell's, or something else hidden from sight. */
function isAgent(frame: TaskStarted): boolean {
  if (frame.skip_transcript === true) {
    return false;
  }
  return frame.task_type === undefined
    ? frame.subagent_type !== undefined
    : frame.task_type === "local_agent";
}

/** Whether words to a subagent are the CLI's, telling it a task of its ended. */
function isNotice(frame: SDKUserMessage, said: string): boolean {
  return frame.isSynthetic === true
    || frame.origin?.kind === "task-notification"
    || said.startsWith(NOTIFIED);
}

/** Whether an Agent call's result only says the subagent was launched (`AgentOutput`). */
function launched(output: Output): boolean {
  const { status } = output;
  return output.isAsync === true || status === "async_launched" || status === "remote_launched";
}

/** A finished call's report, its last block after any notes of the CLI's, or else its text. */
function reported(output: Output, said: string): string {
  return output.content?.at(-1)?.text ?? said;
}

function usageOf(output: Output): SubagentUsage | null {
  const { totalTokens: tokens, totalToolUseCount: tools } = output;
  return tokens === undefined || tools === undefined ? null : { tokens, tools };
}

/** What a task's frame says it used. */
function spent(usage: TaskProgress["usage"]): SubagentUsage {
  return { tokens: usage.total_tokens, tools: usage.tool_uses };
}
