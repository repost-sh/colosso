// The Claude driver's test bench: a query that plays the CLI's part, frame by frame as a test
// says, and a sink that keeps what the driver reported.

import { randomUUID, type UUID } from "node:crypto";

import type {
  Options,
  PermissionMode,
  Query,
  RewindFilesResult,
  SDKControlInitializeResponse,
  SDKControlInterruptResponse,
  SDKMessage,
  SDKUserMessage,
  SlashCommand,
} from "@anthropic-ai/claude-agent-sdk";

import type { RequestPayload, Response } from "../../src/protocol/model";
import type { ToolRequest } from "../../src/providers/claude/approvals";
import {
  ClaudeSession,
  type Conversation,
  type Launch,
  type SessionDeps,
  type Start,
} from "../../src/providers/claude/session";
import type {
  DriverEvent,
  SessionSink,
  SessionSpec,
  TurnInput,
} from "../../src/providers/provider";
import { Queue } from "../../src/queue";
import { settle } from "../support/wait";
import { ofType } from "./mapped";

type Flags = Parameters<Query["applyFlagSettings"]>[0];

/** The environment a CLI on the test bench runs in. */
export const ENV = { PATH: "/usr/bin:/bin", HOME: "/Users/dev" };

/** A control request the driver made of the CLI. */
export type Control =
  | { readonly method: "interrupt" }
  | { readonly method: "setPermissionMode"; readonly mode: PermissionMode }
  | { readonly method: "setModel"; readonly model: string | undefined }
  | { readonly method: "applyFlagSettings"; readonly flags: Flags };

/** What the CLI answers its initialize request with. */
export const INITIALIZED: SDKControlInitializeResponse = {
  commands: [{ name: "compact", description: "Compact the conversation", argumentHint: "" }],
  agents: [],
  output_style: "default",
  available_output_styles: ["default"],
  models: [{
    value: "default",
    displayName: "Default",
    description: "The recommended model",
    supportedEffortLevels: ["low", "medium", "high"],
    supportsFastMode: true,
    supportsAutoMode: true,
  }],
  account: { email: "dev@example.com", subscriptionType: "max", tokenSource: "claude.ai" },
  claude_code_version: "2.1.317",
};

/** The CLI's side of a query: frames a test sends, prompts the driver sent. */
export class FakeQuery implements Conversation {
  readonly options: Options;
  readonly prompts: SDKUserMessage[] = [];
  readonly controls: Control[] = [];
  /** The tasks the driver asked to stop. */
  readonly stopped: string[] = [];
  /** The control requests' methods and the prompts, in the order the CLI had them. */
  readonly journal: string[] = [];
  /** What `interrupt` answers. */
  receipt: SDKControlInterruptResponse = { still_queued: [] };
  commands: SlashCommand[] = [{ name: "review", description: "Review a diff", argumentHint: "" }];
  closed = false;
  initialized: SDKControlInitializeResponse = INITIALIZED;
  /** What `rewindFiles` answers. */
  rewound: RewindFilesResult = { canRewind: true, filesChanged: [], insertions: 0, deletions: 0 };
  /** The rewinds asked of it, as message and whether it was only a dry run. */
  readonly rewinds: { readonly promptId: string; readonly dryRun: boolean }[] = [];
  /** Whether settings changes fail, as a process that died half-way would. */
  refuses = false;
  readonly #frames = new Queue<SDKMessage>();
  readonly #heard: (() => void)[] = [];
  /** Told of each prompt as it reaches the CLI. */
  readonly #onPrompt: ((prompt: SDKUserMessage) => void) | undefined;
  /** Reads the driver's prompts for as long as it gives them. */
  readonly #taking: Promise<void>;

  constructor(params: Parameters<Start>[0], onPrompt?: (prompt: SDKUserMessage) => void) {
    this.options = params.options;
    this.#onPrompt = onPrompt;
    const { prompt } = params;
    this.#taking = typeof prompt === "string" ? Promise.resolve() : this.#take(prompt);
  }

  async rewindFiles(
    promptId: string,
    options?: { dryRun?: boolean },
  ): Promise<RewindFilesResult> {
    this.rewinds.push({ promptId, dryRun: options?.dryRun ?? false });
    return await Promise.resolve(this.rewound);
  }

  /** Whether the driver ended its prompts. */
  async promptsEnded(): Promise<void> {
    await this.#taking;
  }

  /** The CLI sends `frames`. */
  send(...frames: SDKMessage[]): void {
    for (const frame of frames) {
      this.#frames.push(frame);
    }
  }

  /** The CLI's output ends, with `failure` when it broke. */
  end(failure?: Error): void {
    this.#frames.end(failure);
  }

  /** The `count`th prompt the driver sent, once it's sent. */
  async prompt(count: number): Promise<SDKUserMessage> {
    while (this.prompts.length < count) {
      const heard = Promise.withResolvers<undefined>();
      this.#heard.push(() => {
        heard.resolve(undefined);
      });
      await heard.promise;
    }
    const sent = this.prompts[count - 1];
    if (sent === undefined) {
      throw new Error(`No prompt ${String(count)}.`);
    }
    return sent;
  }

  [Symbol.asyncIterator](): AsyncIterator<SDKMessage, undefined> {
    return this.#frames[Symbol.asyncIterator]();
  }

  async interrupt(): Promise<SDKControlInterruptResponse> {
    this.controls.push({ method: "interrupt" });
    this.journal.push("interrupt");
    return await Promise.resolve(this.receipt);
  }

  async setPermissionMode(mode: PermissionMode): Promise<void> {
    this.controls.push({ method: "setPermissionMode", mode });
    await this.#answered();
  }

  async stopTask(taskId: string): Promise<void> {
    this.stopped.push(taskId);
    await Promise.resolve();
  }

  async setModel(model?: string): Promise<void> {
    this.controls.push({ method: "setModel", model });
    await this.#answered();
  }

  async applyFlagSettings(flags: Flags): Promise<void> {
    this.controls.push({ method: "applyFlagSettings", flags });
    await this.#answered();
  }

  async supportedCommands(): Promise<SlashCommand[]> {
    return await Promise.resolve(this.commands);
  }

  async initializationResult(): Promise<SDKControlInitializeResponse> {
    return await Promise.resolve(this.initialized);
  }

  close(): void {
    this.closed = true;
    this.end();
  }

  async #answered(): Promise<void> {
    const control = this.controls.at(-1);
    this.journal.push(control?.method ?? "control");
    if (this.refuses) {
      throw new Error("The CLI closed its control channel.");
    }
    await Promise.resolve();
  }

  async #take(prompts: AsyncIterable<SDKUserMessage>): Promise<void> {
    for await (const prompt of prompts) {
      this.prompts.push(prompt);
      this.journal.push("prompt");
      this.#onPrompt?.(prompt);
      for (const heard of this.#heard.splice(0)) {
        heard();
      }
    }
  }
}

/** A question the driver put to the person, and how to answer it. */
export interface Asked {
  readonly payload: RequestPayload;
  readonly signal: AbortSignal;
  readonly bySubagent: boolean;
  readonly answer: (response: Response) => void;
}

/** What a session reported, and the questions it's waiting on. */
export class Sink implements SessionSink {
  readonly events: DriverEvent[] = [];
  readonly asked: Asked[] = [];
  readonly #finished = Promise.withResolvers<undefined>();

  /** Resolves once a turn has finished. */
  get finished(): Promise<undefined> {
    return this.#finished.promise;
  }

  readonly event = (event: DriverEvent): void => {
    this.events.push(event);
    if (event.type === "turn.finished") {
      this.#finished.resolve(undefined);
    }
  };

  readonly ask = async (
    payload: RequestPayload,
    signal: AbortSignal,
    bySubagent: boolean,
  ): Promise<Response> => {
    const answer = Promise.withResolvers<Response>();
    this.asked.push({ payload, signal, bySubagent, answer: answer.resolve });
    return await answer.promise;
  };

  /** The events of `type`, in order. */
  of<T extends DriverEvent["type"]>(type: T): Extract<DriverEvent, { type: T }>[] {
    return ofType(this.events, type);
  }

  /** The event types, in order, without the usage reports. */
  types(): string[] {
    return this.events.map((event) => event.type).filter((type) => type !== "usage");
  }
}

/** A plain prompt of `text`, as the actor gives a session one. */
export function turnOf(text: string, promptId: UUID = randomUUID()): TurnInput {
  return { promptId, text, attachments: [], compact: false };
}

/** What the SDK tells `canUseTool` beside the tool and its input, as recorded. */
export function toolRequest(change: Partial<ToolRequest> = {}): ToolRequest {
  return {
    signal: new AbortController().signal,
    toolUseID: "toolu_01HPrUmGikhvkmnhPGJdRvse",
    requestId: "7d1545fe-07b0-4cbf-9644-20703d004d67",
    ...change,
  };
}

export function spec(change: Partial<SessionSpec> = {}): SessionSpec {
  return {
    cwd: "/work/rec-cwd",
    sessionId: randomUUID(),
    resume: false,
    resumeAt: null,
    model: null,
    effort: null,
    fast: false,
    permission: "ask",
    plan: false,
    ...change,
  };
}

/** A session on fake queries, each kept as it's started. */
export class Bench {
  readonly sink = new Sink();
  readonly queries: FakeQuery[] = [];
  readonly commands: { cwd: string; names: string[] }[] = [];
  readonly session: ClaudeSession;
  /** The turns waiting for their prompts to reach a CLI, by prompt id. */
  readonly #sending = new Map<string, (query: FakeQuery) => void>();

  constructor(opened: SessionSpec = spec(), launch: Partial<Launch> = {}, stopMs = 50) {
    const deps: SessionDeps = {
      launch: async () => await Promise.resolve({
        executable: "/opt/homebrew/bin/claude",
        env: ENV,
        resumable: false,
        ...launch,
      }),
      start: (params) => {
        const query = new FakeQuery(params, (prompt) => {
          this.#sending.get(prompt.uuid ?? "")?.(query);
        });
        this.queries.push(query);
        return query;
      },
      commands: (cwd, list) => {
        this.commands.push({ cwd, names: list.map((command) => command.name) });
      },
      stopMs,
    };
    this.session = new ClaudeSession(opened, this.sink, deps);
  }

  /** The query the session runs, once it opened. */
  async query(): Promise<FakeQuery> {
    await settle();
    const query = this.queries.at(-1);
    if (query === undefined) {
      throw new Error("The session started no query.");
    }
    return query;
  }

  /** Starts a turn and waits for its prompt to reach the CLI; gives the prompt's id. */
  async turn(text = "Why is the sky blue?"): Promise<{ promptId: UUID; query: FakeQuery }> {
    const promptId = randomUUID();
    const reached = Promise.withResolvers<FakeQuery>();
    this.#sending.set(promptId, reached.resolve);
    this.session.start(turnOf(text, promptId));
    return { promptId, query: await reached.promise };
  }
}

const benches: Bench[] = [];

/** A bench that `closeBenches`, which a test file runs after each test, closes. */
export function bench(...args: ConstructorParameters<typeof Bench>): Bench {
  const made = new Bench(...args);
  benches.push(made);
  return made;
}

/** Closes every bench `bench` made. */
export async function closeBenches(): Promise<void> {
  for (const made of benches.splice(0)) {
    await made.session.close();
  }
}
