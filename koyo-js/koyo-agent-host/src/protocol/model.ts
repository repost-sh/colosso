// The chat records the host keeps and sends (docs/chat.md 4.4): threads, the runs (turns) in
// them, the items their timelines show, and the requests an agent waits on a person for. On
// the wire a missing value is `null`, never an absent field, so both sides read one shape.

import { z } from "zod";

import { ItemId, MessageId, RequestId, RunId, ThreadId } from "./ids";

export const Time = z.iso.datetime();
export type Time = z.infer<typeof Time>;

const Count = z.int().nonnegative();

/** Who runs the agent: "claude", or "fake" in tests. */
export const ProviderId = z.string().min(1);
export type ProviderId = z.infer<typeof ProviderId>;

/** How much the agent may do without asking (C12); the plan toggle is separate. */
export const Mode = z.enum(["ask", "accept_edits", "auto", "full_access"]);
export type Mode = z.infer<typeof Mode>;

export const Effort = z.enum(["low", "medium", "high", "xhigh", "max"]);
export type Effort = z.infer<typeof Effort>;

/** A file the app put in the host's attachments folder, named there. */
export const Attachment = z.object({
  file: z.string().min(1),
  name: z.string(),
  mime: z.string(),
});
export type Attachment = z.infer<typeof Attachment>;

/** What a person sends: the text and what's attached to it. */
export const Prompt = z.object({
  messageId: MessageId,
  text: z.string(),
  attachments: z.array(Attachment),
});
export type Prompt = z.infer<typeof Prompt>;

export const Thread = z.object({
  id: ThreadId,
  cwd: z.string().min(1),
  title: z.string(),
  titleSource: z.enum(["seed", "generated", "user"]),
  provider: ProviderId,
  model: z.string().nullable(),
  effort: Effort.nullable(),
  fast: z.boolean(),
  /** The mode chosen for this chat; `null` leaves it to the provider's own settings. */
  permission: Mode.nullable(),
  plan: z.boolean(),
  /** The provider's conversation, minted before the first turn so a crash still resumes it. */
  sessionId: z.string().min(1).nullable(),
  /**
   * The entry of the conversation the next session goes on from, when a person went back to before
   * its later turns (C11), until a turn has gone on from there.
   */
  resumeAt: z.string().min(1).nullable().default(null),
  /** When the chat last wrote files, as far as anyone knows: a later write is someone else's. */
  filesSettledAt: Time.nullable().default(null),
  /** Queued runs wait for the person after a stop, a failure or a restart. */
  queueHeld: z.boolean(),
  /** A tab shows it, and will again after a restart (C13). */
  tabOpen: z.boolean(),
  /** The row of the thread that started it, for a subagent's: it runs on its own (4.5.10). */
  parent: z.object({ threadId: ThreadId, itemId: ItemId }).nullable().default(null),
  /**
   * How many tasks the agent has running in the background, as its session last said: shells,
   * Monitors, subagents past their turn. It goes on from them as they end, so the chat isn't done.
   * None while no session runs.
   */
  backgroundTasks: Count.default(0),
  createdAt: Time,
  updatedAt: Time,
  readAt: Time.nullable(),
  seq: Count,
});
export type Thread = z.infer<typeof Thread>;

export const RunStatus = z.enum([
  "queued",
  "starting",
  "running",
  "stopping",
  "completed",
  "interrupted",
  "failed",
  /** Taken off the queue, or gone back past (C11); clients show it no more. */
  "removed",
]);
export type RunStatus = z.infer<typeof RunStatus>;

export const FailureKind = z.enum([
  "api",
  "usage_limit",
  "auth",
  "overloaded",
  "process",
  "host",
]);
export type FailureKind = z.infer<typeof FailureKind>;

export const Failure = z.object({
  kind: FailureKind,
  message: z.string(),
  retryAt: Time.nullable(),
});
export type Failure = z.infer<typeof Failure>;

export const Usage = z.object({
  inputTokens: Count,
  outputTokens: Count,
  cacheReadTokens: Count,
  cacheCreationTokens: Count,
  costUsd: z.number().nonnegative().nullable(),
  /** What the conversation holds now, and what the model can hold. */
  contextTokens: Count.nullable(),
  contextWindow: Count.nullable(),
});
export type Usage = z.infer<typeof Usage>;

/** Who asked a run to stop, and whether its process was killed rather than asked. */
export const Stop = z.object({
  requestedAt: Time,
  by: z.enum(["person", "host"]),
  force: z.boolean(),
});
export type Stop = z.infer<typeof Stop>;

export const FileChange = z.object({
  path: z.string(),
  added: Count,
  removed: Count,
});
export type FileChange = z.infer<typeof FileChange>;

/** A turn: one prompt and the work it caused. A queued run is only its prompt. */
export const Run = z.object({
  id: RunId,
  threadId: ThreadId,
  status: RunStatus,
  prompt: Prompt,
  model: z.string().nullable(),
  effort: Effort.nullable(),
  /** The mode in force while it ran, as the provider reported it. */
  permission: Mode.nullable(),
  plan: z.boolean(),
  requestedAt: Time,
  startedAt: Time.nullable(),
  endedAt: Time.nullable(),
  stop: Stop.nullable(),
  failure: Failure.nullable(),
  usage: Usage.nullable(),
  files: z.array(FileChange),
  seq: Count,
});
export type Run = z.infer<typeof Run>;

export const ItemStatus = z.enum([
  "pending",
  "running",
  "completed",
  "failed",
  "declined",
  "interrupted",
]);
export type ItemStatus = z.infer<typeof ItemStatus>;

export const ToolFamily = z.enum([
  "command",
  "read",
  "edit",
  "search",
  "web",
  "mcp",
  "agent",
  "todo",
  "skill",
  "other",
]);
export type ToolFamily = z.infer<typeof ToolFamily>;

/** One hunk of a unified diff, as the provider's structured patch has it. */
export const Hunk = z.object({
  oldStart: Count,
  oldLines: Count,
  newStart: Count,
  newLines: Count,
  lines: z.array(z.string()),
});
export type Hunk = z.infer<typeof Hunk>;

export const Todo = z.object({
  text: z.string(),
  status: z.enum(["pending", "in_progress", "completed"]),
});
export type Todo = z.infer<typeof Todo>;

/** What a subagent has used so far. */
export const SubagentUsage = z.object({ tokens: Count, tools: Count });
export type SubagentUsage = z.infer<typeof SubagentUsage>;

export const NoticeLevel = z.enum(["info", "warning", "error"]);
export type NoticeLevel = z.infer<typeof NoticeLevel>;

const ItemBase = z.object({
  id: ItemId,
  threadId: ThreadId,
  /** Every entry belongs to a turn, so history pages by run. */
  runId: RunId,
  /** Its place in the timeline, fixed when the item is first seen. */
  ordinal: Count,
  status: ItemStatus,
  seq: Count,
});

export const Item = z.discriminatedUnion("kind", [
  ItemBase.extend({
    kind: z.literal("user_message"),
    payload: z.object({
      /** The message's id, so a send repeated is answered from what it became. */
      messageId: MessageId,
      text: z.string(),
      attachments: z.array(Attachment),
      delivered: z.enum(["turn", "steer"]),
    }),
  }),
  ItemBase.extend({ kind: z.literal("assistant_text"), payload: z.object({ text: z.string() }) }),
  ItemBase.extend({ kind: z.literal("reasoning"), payload: z.object({ text: z.string() }) }),
  ItemBase.extend({
    kind: z.literal("tool"),
    payload: z.object({
      name: z.string(),
      family: ToolFamily,
      label: z.string(),
      input: z.json(),
      /** The end of what it printed or returned, bounded. */
      output: z.string().nullable(),
      exitCode: z.int().nullable(),
      patch: z.array(Hunk).nullable(),
      files: z.array(z.string()),
      /** The MCP server, for an MCP tool. */
      server: z.string().nullable(),
    }),
  }),
  ItemBase.extend({ kind: z.literal("todos"), payload: z.object({ todos: z.array(Todo) }) }),
  ItemBase.extend({ kind: z.literal("request"), payload: z.object({ requestId: RequestId }) }),
  ItemBase.extend({
    kind: z.literal("subagent"),
    payload: z.object({
      description: z.string(),
      agentType: z.string().nullable(),
      /** What it reported as it ended. */
      summary: z.string().nullable(),
      /** The thread of its own work, which a row from before they had one doesn't have. */
      threadId: ThreadId.nullable().default(null),
      /** What it says it's doing, while it runs. */
      progress: z.string().nullable().default(null),
      usage: SubagentUsage.nullable().default(null),
      startedAt: Time.nullable().default(null),
      endedAt: Time.nullable().default(null),
    }),
  }),
  ItemBase.extend({
    kind: z.literal("compaction"),
    payload: z.object({
      trigger: z.enum(["manual", "auto"]),
      before: Count.nullable(),
      after: Count.nullable(),
    }),
  }),
  ItemBase.extend({
    kind: z.literal("notice"),
    payload: z.object({ level: NoticeLevel, text: z.string() }),
  }),
]);
export type Item = z.infer<typeof Item>;
export type ItemKind = Item["kind"];

export const Question = z.object({
  id: z.string().min(1),
  header: z.string(),
  question: z.string(),
  multiSelect: z.boolean(),
  options: z.array(z.object({
    label: z.string(),
    description: z.string().nullable(),
    preview: z.string().nullable(),
  })),
});
export type Question = z.infer<typeof Question>;

/** What an agent can wait on a person for (C14). */
export const RequestKind = z.enum(["approval", "question", "plan"]);
export type RequestKind = z.infer<typeof RequestKind>;

export const RequestPayload = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal(RequestKind.enum.approval),
    tool: z.string(),
    family: ToolFamily,
    title: z.string(),
    /** What it's asked of, as a line of work goes by it once it has run. */
    label: z.string(),
    /** What is asked, shown as is: a command, a path, or else the input. */
    detail: z.string(),
    /** An edit's change to the file `detail` names: its lines, each starting `+` or `-`. */
    change: z.array(z.string()).nullable(),
    input: z.json(),
    reason: z.string().nullable(),
    /** What allowing for this chat adds, as a person is offered it, when it can. */
    allowSession: z.object({ label: z.string() }).nullable(),
  }),
  z.object({
    kind: z.literal(RequestKind.enum.question),
    questions: z.array(Question).min(1),
    /** Whether the agent waits for the answer; else the next message carries it. */
    live: z.boolean(),
  }),
  /** A proposed plan. Never live: the agent stopped at it, and the next message refines it. */
  z.object({ kind: z.literal(RequestKind.enum.plan), markdown: z.string() }),
]);
export type RequestPayload = z.infer<typeof RequestPayload>;

export const Decision = z.enum(["allow", "allow_session", "deny", "cancel"]);
export type Decision = z.infer<typeof Decision>;

/** How a person answers a request where it's shown; its kind is the request's. */
export const Response = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal(RequestKind.enum.approval),
    decision: Decision,
    message: z.string().nullable(),
  }),
  z.object({
    kind: z.literal(RequestKind.enum.question),
    /** The chosen labels, or the typed answer, by question id. */
    answers: z.record(z.string(), z.array(z.string())),
  }),
  z.object({ kind: z.literal(RequestKind.enum.plan), decision: z.enum(["implement", "dismiss"]) }),
]);
export type Response = z.infer<typeof Response>;

/** How a request was answered: where it's shown, or by the message sent next (C14). */
export const Answer = z.discriminatedUnion("kind", [
  ...Response.options,
  z.object({ kind: z.literal("message"), messageId: MessageId }),
]);
export type Answer = z.infer<typeof Answer>;

export const Request = z.object({
  id: RequestId,
  threadId: ThreadId,
  runId: RunId,
  status: z.enum(["pending", "answered", "cancelled", "expired"]),
  payload: RequestPayload,
  answer: Answer.nullable(),
  createdAt: Time,
  resolvedAt: Time.nullable(),
  seq: Count,
});
export type Request = z.infer<typeof Request>;

export const Status = z.enum(["idle", "working", "failed"]);
export type Status = z.infer<typeof Status>;

/** A thread as lists show it, with what it needs derived (docs/chat.md 4.4). */
export const ThreadSummary = z.object({
  id: ThreadId,
  cwd: z.string(),
  title: z.string(),
  provider: ProviderId,
  model: z.string().nullable(),
  /** What the chat chose of the rest of what it runs with, as `Thread` has it. */
  effort: Effort.nullable(),
  fast: z.boolean(),
  permission: Mode.nullable(),
  plan: z.boolean(),
  status: Status,
  /** What the oldest pending request asks of the person. */
  needs: RequestKind.nullable(),
  /** Whether the agent has tasks running in the background (`Thread.backgroundTasks`, 4.5.6). */
  background: z.boolean(),
  queued: Count,
  unread: z.boolean(),
  tabOpen: z.boolean(),
  updatedAt: Time,
});
export type ThreadSummary = z.infer<typeof ThreadSummary>;
