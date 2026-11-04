// The rows a thread's actor writes, made and changed by pure functions: a new run or item, a
// run moved through the one transition function, a run settled with what it left open.

import type { UUID } from "node:crypto";
import { join } from "node:path";

import { ItemId, MessageId, newId, RequestId, RunId, ThreadId } from "../protocol/ids";
import type { Params } from "../protocol/methods";
import type {
  Answer,
  Failure,
  Item,
  Mode,
  NoticeLevel,
  Prompt,
  ProviderId,
  Request,
  RequestPayload,
  Run,
  RunStatus,
  Stop,
  SubagentUsage,
  Thread,
  Usage,
} from "../protocol/model";
import type {
  ItemBody,
  Outcome,
  SessionChange,
  SubagentChange,
  SubagentStart,
  TurnInput,
} from "../providers/provider";
import { canMove, isCompact, isOpen } from "./rules";

/** The longest seed title: the prompt's first line, cut (docs/chat.md 4.5.7). */
const SEED_TITLE = 60;
/** The most words a generated title keeps (4.5.7). */
const TITLE_WORDS = 6;

/** A new run already running, as no person asked for it: it was never queued or starting. */
export function runningRun(thread: Thread, prompt: Prompt, at: string): Run {
  const queued = queuedRun(thread, prompt, at);
  return moved(moved(queued, "starting", { startedAt: at }), "running");
}

/** The row a subagent has in the timeline of the thread that started it. */
export type SubagentRow = Extract<Item, { kind: "subagent" }>;

/** A new chat on `provider`, as `threads.create` asks; its title comes with its first prompt. */
export function newThread(
  params: Params<"threads.create">,
  provider: ProviderId,
  at: string,
): Thread {
  return {
    id: newId(ThreadId),
    ...params,
    provider,
    title: "",
    titleSource: "seed",
    sessionId: null,
    resumeAt: null,
    filesSettledAt: null,
    queueHeld: false,
    tabOpen: true,
    parent: null,
    createdAt: at,
    updatedAt: at,
    readAt: null,
    seq: 0,
  };
}

/** The thread of subagent `subagent`, which row `itemId` of thread `parent` shows (4.5.10). */
export function subagentThread(
  parent: Thread,
  ids: { readonly threadId: ThreadId; readonly itemId: ItemId },
  subagent: SubagentStart,
  at: string,
): Thread {
  const { threadId, itemId } = ids;
  return {
    ...parent,
    id: threadId,
    title: subagent.description,
    titleSource: "seed",
    model: subagent.model ?? parent.model,
    effort: subagent.effort ?? parent.effort,
    fast: false,
    plan: false,
    sessionId: null,
    resumeAt: null,
    filesSettledAt: null,
    queueHeld: false,
    tabOpen: false,
    parent: { threadId: parent.id, itemId },
    createdAt: at,
    updatedAt: at,
    readAt: null,
    seq: 0,
  };
}

/** The row of subagent `subagent`, of `run`, whose thread is `threadId`. */
export function subagentRow(
  run: Run,
  place: { readonly ordinal: number; readonly at: string },
  threadId: ThreadId,
  subagent: SubagentStart,
): SubagentRow {
  const payload = {
    description: subagent.description,
    agentType: subagent.agentType,
    summary: null,
    threadId,
    progress: null,
    usage: null,
    startedAt: place.at,
    endedAt: null,
  };
  return newItem(run, place.ordinal, { kind: "subagent", payload }, "running");
}

/** `row` started again, as a subagent can be. */
export function subagentAgain(row: SubagentRow): SubagentRow {
  const payload = { ...row.payload, summary: null, progress: null, endedAt: null };
  return { ...row, status: "running", payload };
}

/** `row` with what's new of its subagent. */
export function subagentChanged(row: SubagentRow, change: SubagentChange): SubagentRow {
  const { payload } = row;
  return {
    ...row,
    payload: {
      ...payload,
      progress: change.progress ?? payload.progress,
      usage: change.usage ?? payload.usage,
    },
  };
}

/** `row` as its subagent ended. */
export function subagentEnded(
  row: SubagentRow,
  outcome: Outcome,
  ending: {
    readonly summary: string | null;
    readonly usage: SubagentUsage | null;
    readonly at: string;
  },
): SubagentRow {
  const { payload } = row;
  return {
    ...row,
    status: outcome,
    payload: {
      ...payload,
      summary: ending.summary ?? payload.summary,
      usage: ending.usage ?? payload.usage,
      progress: null,
      endedAt: ending.at,
    },
  };
}

/** What a host that stopped left open, which went with it (4.1). */
export function lost(item: Item, at: string): Item {
  return item.kind === "subagent"
    ? subagentEnded(item, "interrupted", { summary: null, usage: null, at })
    : { ...item, status: "interrupted" };
}

/** A prompt of `text` alone, as the host sends one of its own. */
export function textPrompt(text: string): Prompt {
  return { messageId: newId(MessageId), text, attachments: [] };
}

export function queuedRun(thread: Thread, prompt: Prompt, at: string): Run {
  return {
    id: newId(RunId),
    threadId: thread.id,
    status: "queued",
    prompt,
    model: thread.model,
    effort: thread.effort,
    permission: thread.permission,
    plan: thread.plan,
    requestedAt: at,
    startedAt: null,
    endedAt: null,
    stop: null,
    failure: null,
    usage: null,
    files: [],
    seq: 0,
  };
}

/** `run` moved to `to`, or an error naming the move it may not make. */
export function moved(run: Run, to: RunStatus, change: Partial<Run> = {}): Run {
  if (!canMove(run.status, to)) {
    throw new Error(`A run that is ${run.status} can't become ${to}.`);
  }
  return { ...run, ...change, status: to };
}

/** Who asked a run to stop, now, asking rather than killing its process. */
export function stopBy(by: Stop["by"], at: string): Stop {
  return { requestedAt: at, by, force: false };
}

/** What a run's end is told, each defaulting to what the run had: why it failed, what it
 * used, who stopped it. */
export interface Ending {
  readonly failure?: Failure | null;
  readonly usage?: Usage | null;
  readonly stop?: Stop | null;
}

/** `run` ended as `outcome` at `ending.at`, and its items that were still open, closed. */
export function settled(
  run: Run,
  outcome: Outcome,
  open: readonly Item[],
  ending: Ending & { readonly at: string },
): { readonly run: Run; readonly items: readonly Item[] } {
  return {
    run: moved(run, outcome, {
      endedAt: ending.at,
      failure: ending.failure ?? null,
      usage: ending.usage ?? run.usage,
      stop: ending.stop ?? run.stop,
    }),
    items: open.filter(isOpen).map((item) => ({ ...item, status: closed(item, outcome) })),
  };
}

/**
 * What an item left open becomes when its run ends as `outcome` (docs/chat.md 4.5): after a
 * run that completed, a tool or a subagent no result came for can't be said to have run, and
 * a compaction that never said it ended didn't, so they fail, and the rest completes; after
 * any other end, all of it was interrupted.
 */
function closed(item: Item, outcome: Outcome): Item["status"] {
  if (outcome !== "completed") {
    return "interrupted";
  }
  switch (item.kind) {
    case "tool":
    case "subagent":
    case "compaction": {
      return "failed";
    }
    case "user_message":
    case "assistant_text":
    case "reasoning":
    case "todos":
    case "request":
    case "notice": {
      return "completed";
    }
  }
}

/** An item of `run` at `ordinal`, from what a provider or the actor says it is. */
export function newItem<Said extends ItemBody>(
  run: Run,
  ordinal: number,
  body: Said,
  status: Item["status"],
): Omit<SubagentRow, keyof ItemBody> & Said {
  return {
    id: newId(ItemId),
    threadId: run.threadId,
    runId: run.id,
    ordinal,
    status,
    seq: 0,
    ...body,
  };
}

/** An item of `run` at `ordinal` that's whole as it's written: a message, a notice. */
function closedItem(run: Run, ordinal: number, body: ItemBody): Item {
  return newItem(run, ordinal, body, "completed");
}

/** The item of the message that started or steered `run`. */
export function userMessage(
  run: Run,
  ordinal: number,
  prompt: Prompt,
  delivered: "turn" | "steer",
): Item {
  const { messageId, text, attachments } = prompt;
  const payload = { messageId, text, attachments, delivered };
  return closedItem(run, ordinal, { kind: "user_message", payload });
}

/**
 * `prompt` as a session takes it, as a turn or a message joining one, with its files where
 * `folder` keeps them.
 */
export function turnInput(
  promptId: UUID,
  prompt: Prompt,
  folder: string,
  delivered: "turn" | "steer",
): TurnInput {
  return {
    promptId,
    text: prompt.text,
    attachments: prompt.attachments.map((attachment) => ({
      path: join(folder, attachment.file),
      mime: attachment.mime,
    })),
    compact: delivered === "turn" && isCompact(prompt.text),
  };
}

/** A note in the timeline from the provider, such as a retry or a warning. */
export function notice(run: Run, ordinal: number, level: NoticeLevel, text: string): Item {
  return closedItem(run, ordinal, { kind: "notice", payload: { level, text } });
}

/**
 * A request of `run`'s, pending since `at`, and its place in the timeline at `ordinal`. A
 * question is live while the agent waits on it (`waited`); else the next message answers it.
 */
export function newRequest(
  run: Run,
  payload: RequestPayload,
  waited: boolean,
  { ordinal, at }: { readonly ordinal: number; readonly at: string },
): { readonly request: Request; readonly item: Item } {
  const request: Request = {
    id: newId(RequestId),
    threadId: run.threadId,
    runId: run.id,
    status: "pending",
    payload: payload.kind === "question" ? { ...payload, live: waited } : payload,
    answer: null,
    createdAt: at,
    resolvedAt: null,
    seq: 0,
  };
  const item = closedItem(run, ordinal, { kind: "request", payload: { requestId: request.id } });
  return { request, item };
}

/** `item` grown by `text`, when it's an item that streams. */
export function appended(item: Item, text: string): Item | undefined {
  switch (item.kind) {
    case "assistant_text":
    case "reasoning": {
      return { ...item, payload: { text: item.payload.text + text } };
    }
    case "user_message":
    case "tool":
    case "todos":
    case "request":
    case "subagent":
    case "compaction":
    case "notice": {
      return undefined;
    }
  }
}

export function answered(request: Request, answer: Answer, at: string): Request {
  return { ...request, status: "answered", answer, resolvedAt: at };
}

/** A request no answer is wanted for anymore: its turn ended, or the agent withdrew it. */
export function cancelled(request: Request, at: string): Request {
  return { ...request, status: "cancelled", resolvedAt: at };
}

/**
 * A live request once the host that held it has stopped (docs/chat.md 4.1): an approval
 * expires, a question waits for the next message; `undefined` when nothing changes.
 */
export function orphaned(request: Request, at: string): Request | undefined {
  switch (request.payload.kind) {
    case "approval": {
      return { ...request, status: "expired", resolvedAt: at };
    }
    case "question": {
      return request.payload.live
        ? { ...request, payload: { ...request.payload, live: false } }
        : undefined;
    }
    case "plan": {
      return undefined;
    }
  }
}

/** Whether the agent stopped waiting on `request`, so the next message answers it. */
export function answeredByNextMessage(request: Request): boolean {
  switch (request.payload.kind) {
    case "plan": {
      return true;
    }
    case "question": {
      return !request.payload.live;
    }
    case "approval": {
      return false;
    }
  }
}

export type ThreadChange = Omit<Params<"threads.update">, "threadId">;

/** `thread` with what `change` names changed; a title given is the person's. */
export function updatedThread(thread: Thread, change: ThreadChange, at: string): Thread {
  const settings: Thread = {
    ...thread,
    title: change.title ?? thread.title,
    titleSource: change.title === undefined ? thread.titleSource : "user",
    model: change.model === undefined ? thread.model : change.model,
    effort: change.effort === undefined ? thread.effort : change.effort,
    fast: change.fast ?? thread.fast,
    permission: change.permission === undefined ? thread.permission : change.permission,
    plan: change.plan ?? thread.plan,
  };
  return {
    ...settings,
    tabOpen: change.tabOpen ?? thread.tabOpen,
    // A tab coming or going is nothing done in the chat, and lists go by when one last was.
    updatedAt: Bun.deepEquals(settings, thread) ? thread.updatedAt : at,
  };
}

/**
 * What changed between `before` and `after` that a live session takes; a chat's mode that
 * changed to none is Koyo's `defaultMode`, as a session is opened with (4.5.6).
 */
export function settingsChanged(
  before: Thread,
  after: Thread,
  defaultMode: Mode | null,
): SessionChange {
  const permission = after.permission ?? defaultMode;
  return {
    ...(after.model === before.model ? {} : { model: after.model }),
    ...(after.effort === before.effort ? {} : { effort: after.effort }),
    ...(after.fast === before.fast ? {} : { fast: after.fast }),
    ...(after.permission === before.permission ? {} : { permission }),
    ...(after.plan === before.plan ? {} : { plan: after.plan }),
  };
}

/** The title a chat gets from its first prompt until a better one comes. */
export function seedTitle(text: string): string {
  const line = text.trim().split("\n", 1).at(0) ?? "";
  return line.length > SEED_TITLE ? `${line.slice(0, SEED_TITLE - 1).trimEnd()}…` : line;
}

/**
 * The title a model's `reply` makes: its first line that isn't empty, without the quotes and
 * markdown around it, at most six words; `undefined` when nothing is left.
 */
export function titleFrom(reply: string): string | undefined {
  const line = reply.split("\n").map((each) => each.trim()).find((each) => each !== "") ?? "";
  const bare = line.replace(/^[\s"'`*#]+|[\s"'`*.]+$/gu, "");
  const words = bare.split(/\s+/u).filter((word) => word !== "").slice(0, TITLE_WORDS);
  return words.length === 0 ? undefined : words.join(" ");
}
