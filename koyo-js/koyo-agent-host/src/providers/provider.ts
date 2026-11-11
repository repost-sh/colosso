// The seam between a thread's actor and whoever runs the agent (docs/chat.md C6, 4.6): the
// actor speaks in these terms only, never in a provider's SDK types. Claude is the first
// provider, a scripted fake the second; another (Codex) plugs in here, and the actor branches
// on its capabilities, never on its name.

import type { UUID } from "node:crypto";

import type { Catalog, FilesBack, Params, ProviderStatus } from "../protocol/methods";
import type {
  AccountLimits,
  Effort,
  Failure,
  Item,
  Mode,
  NoticeLevel,
  ProviderId,
  RequestPayload,
  Response,
  SubagentUsage,
  Usage,
} from "../protocol/model";

export interface Capabilities {
  /** A message can join a running turn. */
  readonly steer: boolean;
  /** A closed session can be resumed with its id. */
  readonly resume: boolean;
  readonly images: boolean;
  readonly compact: boolean;
  readonly plan: boolean;
}

/** What putting a conversation's files back would change, by the provider's names for them. */
export type FileChanges = Omit<FilesBack, "blocked">;

/** What a session is opened with. */
export interface SessionSpec {
  /** The chat's id, which Koyo's MCP server names it by. */
  readonly threadId: string;
  readonly cwd: string;
  /** The conversation's id, minted by the host before the first turn. */
  readonly sessionId: string;
  /** Whether the conversation has turns already, to continue rather than start. */
  readonly resume: boolean;
  readonly model: string | null;
  readonly effort: Effort | null;
  readonly fast: boolean;
  /**
   * The chat's mode, else Koyo's default; `null` when neither names one, so the provider's
   * own settings choose.
   */
  readonly permission: Mode | null;
  readonly plan: boolean;
  /** The entry of the conversation to go on from, dropping what came after; else its latest. */
  readonly resumeAt: string | null;
  /** The `koyo` the session starts as Koyo's MCP server, as Koyo's settings say (`config.set`). */
  readonly koyoTools: string | null;
}

/** A change to a live session's settings. */
export type SessionChange = Partial<
  Pick<SessionSpec, "model" | "effort" | "fast" | "permission" | "plan">
>;

/** A prompt as a session takes it: its id is fresh for every send (T3 adapter pitfall 7). */
export interface TurnInput {
  readonly promptId: UUID;
  readonly text: string;
  /** Files by their path on disk. */
  readonly attachments: readonly { readonly path: string; readonly mime: string }[];
  /**
   * Whether the turn compacts the conversation (Koyo's `/compact`), which a provider does in
   * its own way; a message joining a running turn never does.
   */
  readonly compact: boolean;
}

/** Koyo's settings for the agents (`config.set`). */
export type Config = Params<"config.set">;

/** How a turn ended. */
export type Outcome = "completed" | "interrupted" | "failed";

/** A failure of the agent's process, as a person reads it. */
export function processFailure(message: string): Failure {
  return { kind: "process", message, retryAt: null };
}

/** Each kind of item as its kind and payload alone. */
type Body<I> = I extends { kind: unknown; payload: unknown } ? Pick<I, "kind" | "payload"> : never;

/** An item as a provider describes it: what it is, without where the thread keeps it. */
export type ItemBody = Body<Item>;

/**
 * What's said of a turn's timeline. `key` is the provider's own stable name for an item (a tool
 * use's id, a message's id and block); the actor gives the item its id and place the first
 * time it sees the key.
 */
export type ItemEvent =
  | { readonly type: "item.started"; readonly key: string; readonly item: ItemBody }
  | { readonly type: "item.appended"; readonly key: string; readonly text: string }
  | {
    readonly type: "item.updated";
    readonly key: string;
    readonly item: ItemBody;
    readonly status: Item["status"];
  }
  | { readonly type: "notice"; readonly level: NoticeLevel; readonly text: string };

/** A subagent as it starts, or starts again. */
export interface SubagentStart {
  readonly description: string;
  readonly agentType: string | null;
  /** What it was asked, which starts its thread. */
  readonly prompt: string;
  /** What it runs, when that's not what its thread's parent runs. */
  readonly model: string | null;
  readonly effort: Effort | null;
}

/** What's new of a running subagent. */
export interface SubagentChange {
  readonly model?: string;
  readonly progress?: string;
  readonly usage?: SubagentUsage | null;
}

/**
 * What's said of a subagent, whatever turn runs by then, as one may run on past the turn that
 * started it (docs/chat.md 4.5.10). `key` is the provider's own stable name for it, and
 * `parentKey` the subagent's that started it, if one did.
 */
export type SubagentEvent =
  | {
    readonly type: "subagent.started";
    readonly key: string;
    readonly parentKey: string | null;
    readonly subagent: SubagentStart;
  }
  | { readonly type: "subagent.changed"; readonly key: string; readonly change: SubagentChange }
  | { readonly type: "subagent.event"; readonly key: string; readonly event: ItemEvent }
  | {
    readonly type: "subagent.ended";
    readonly key: string;
    readonly outcome: Outcome;
    readonly summary: string | null;
    readonly usage: SubagentUsage | null;
  };

/**
 * What a session reports. A turn's events all come before its `turn.finished`; a subagent's
 * come whenever, a subagent's own timeline among them.
 */
export type DriverEvent =
  | ItemEvent
  | SubagentEvent
  /** The mode and the model the session runs in, as the provider names them. */
  | { readonly type: "session.ready"; readonly mode: Mode | null; readonly model: string | null }
  | { readonly type: "turn.started"; readonly promptId: string }
  /** The agent started a turn no prompt asked for, as a subagent finished: it runs from now. */
  | { readonly type: "turn.adopted" }
  /** How many tasks the agent has running in the background now, which it goes on from as they
   * end: shells, Monitors, subagents past their turn. None once its process ends. */
  | { readonly type: "background"; readonly tasks: number }
  | { readonly type: "usage"; readonly usage: Usage }
  /** What a response said of the account's subscription limits, whatever turn runs. */
  | { readonly type: "limits"; readonly limits: AccountLimits }
  /** Something the agent stopped at for the person without waiting, as a plan: the next
   * message answers it (C14). What the agent waits on comes through `SessionSink.ask`. */
  | { readonly type: "request"; readonly request: RequestPayload }
  | {
    readonly type: "turn.finished";
    readonly outcome: Outcome;
    readonly failure: Failure | null;
    readonly usage: Usage | null;
  }
  | { readonly type: "session.ended"; readonly error: string | null };

/** Where a session reports to. */
export interface SessionSink {
  readonly event: (event: DriverEvent) => void;
  /**
   * Asks the person; `signal` aborts when the agent stops waiting. `bySubagent` when a subagent
   * asks, which it may with no turn running, and whose asking a turn's end doesn't end.
   */
  readonly ask: (
    request: RequestPayload,
    signal: AbortSignal,
    bySubagent: boolean,
  ) => Promise<Response>;
}

/**
 * One live conversation. Its methods return once their work is asked for, never once the
 * agent has acted on it: what follows arrives on the sink.
 */
export interface ProviderSession {
  /** Starts a turn; one runs at a time. */
  readonly start: (turn: TurnInput) => void;
  /** Adds a message to the running turn; false when none runs anymore. */
  readonly steer: (turn: TurnInput) => boolean;
  readonly interrupt: () => Promise<void>;
  /** Asks subagent `key` to stop: its `subagent.ended` says when it has, whatever else happens. */
  readonly stopSubagent: (key: string) => void;
  /**
   * Takes a change, applied before the next turn starts, or says at once that the session
   * must be opened again to take it, so the turn the same step starts never runs on the old
   * settings. A change that fails to apply is the session's own to recover from.
   */
  readonly configure: (change: SessionChange) => "applied" | "reopen";
  /** Ends the session's process now, and with it a turn that wouldn't stop. */
  readonly kill: () => void;
  readonly close: () => Promise<void>;
}

export interface Provider {
  readonly id: ProviderId;
  readonly capabilities: Capabilities;
  /** Takes Koyo's settings, each time they change. */
  readonly setConfig: (config: Config) => void;
  readonly status: (refresh: boolean) => Promise<ProviderStatus>;
  /**
   * What the provider offers in `cwd`; its default mode and model are the provider's own
   * settings'. Without `commands` it doesn't start the provider to learn the folder's.
   */
  readonly catalog: (cwd: string, commands?: boolean) => Promise<Catalog>;
  readonly open: (spec: SessionSpec, sink: SessionSink) => ProviderSession;
  /**
   * The entry of conversation `sessionId` right before the prompt `promptId`, or `null` when that
   * started it; refuses when the conversation doesn't have it.
   */
  readonly entryBefore: (
    sessionId: string,
    cwd: string,
    promptId: string,
  ) => Promise<string | null>;
  /**
   * What putting the files of conversation `sessionId` back as they were when `promptId` was sent
   * would change, changing nothing; `null` when the provider has no record of them then.
   */
  readonly filesAt: (
    sessionId: string,
    cwd: string,
    promptId: string,
  ) => Promise<FileChanges | null>;
  /** Puts them back; refuses when it can't. */
  readonly restoreFiles: (sessionId: string, cwd: string, promptId: string) => Promise<void>;
  /**
   * The model's reply when asked for a short title for a chat that starts with `prompt`, as
   * it came, if one came; the actor makes the title of it (docs/chat.md 4.5.7).
   */
  readonly title: (prompt: string, cwd: string) => Promise<string | undefined>;
}
