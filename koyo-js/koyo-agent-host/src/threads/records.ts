// The rows a thread's actor writes, made and changed by pure functions: a new run or item, a
// run moved through the one transition function, a run settled with what it left open.

import { join } from "node:path";

import { ItemId, MessageId, newId, RequestId, RunId, ThreadId } from "../protocol/ids";
import type { Params } from "../protocol/methods";
import type {
  Answer,
  Failure,
  Item,
  NoticeLevel,
  Prompt,
  Request,
  RequestPayload,
  Run,
  RunStatus,
  Stop,
  Thread,
  Usage,
} from "../protocol/model";
import type { ItemBody, Outcome, SessionChange, TurnInput } from "../providers/provider";
import { canMove, isOpen } from "./rules";

/** The longest seed title: the prompt's first line, cut (docs/chat.md 4.5.7). */
const SEED_TITLE = 60;

/** A new chat, as `threads.create` asks for it; its title comes with its first prompt. */
export function newThread(params: Params<"threads.create">, at: string): Thread {
  return {
    id: newId(ThreadId),
    ...params,
    title: "",
    titleSource: "seed",
    fast: false,
    sessionId: null,
    queueHeld: false,
    createdAt: at,
    updatedAt: at,
    readAt: null,
    seq: 0,
  };
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

/** A failure of the agent's process, as a person reads it. */
export function processFailure(message: string): Failure {
  return { kind: "process", message, retryAt: null };
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
  const status = outcome === "completed" ? "completed" : "interrupted";
  return {
    run: moved(run, outcome, {
      endedAt: ending.at,
      failure: ending.failure ?? null,
      usage: ending.usage ?? run.usage,
      stop: ending.stop ?? run.stop,
    }),
    items: open.filter(isOpen).map((item) => ({ ...item, status })),
  };
}

/** An item of `run` at `ordinal`, from what a provider or the actor says it is. */
export function newItem(
  run: Run,
  ordinal: number,
  body: ItemBody,
  place: { readonly parentId: ItemId | null; readonly status: Item["status"] },
): Item {
  return {
    id: newId(ItemId),
    threadId: run.threadId,
    runId: run.id,
    ordinal,
    parentId: place.parentId,
    status: place.status,
    seq: 0,
    ...body,
  };
}

/** An item of `run` at `ordinal` that's whole as it's written: a message, a notice. */
function closedItem(run: Run, ordinal: number, body: ItemBody): Item {
  return newItem(run, ordinal, body, { parentId: null, status: "completed" });
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

/** `prompt` as a session takes it, with its files where `folder` keeps them. */
export function turnInput(promptId: string, prompt: Prompt, folder: string): TurnInput {
  return {
    promptId,
    text: prompt.text,
    attachments: prompt.attachments.map((attachment) => ({
      path: join(folder, attachment.file),
      mime: attachment.mime,
    })),
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
  return {
    ...thread,
    title: change.title ?? thread.title,
    titleSource: change.title === undefined ? thread.titleSource : "user",
    model: change.model === undefined ? thread.model : change.model,
    effort: change.effort === undefined ? thread.effort : change.effort,
    fast: change.fast ?? thread.fast,
    permission: change.permission === undefined ? thread.permission : change.permission,
    plan: change.plan ?? thread.plan,
    updatedAt: at,
  };
}

/** What changed between `before` and `after` that a live session takes. */
export function settingsChanged(before: Thread, after: Thread): SessionChange {
  return {
    ...(after.model === before.model ? {} : { model: after.model }),
    ...(after.effort === before.effort ? {} : { effort: after.effort }),
    ...(after.fast === before.fast ? {} : { fast: after.fast }),
    ...(after.permission === before.permission ? {} : { permission: after.permission }),
    ...(after.plan === before.plan ? {} : { plan: after.plan }),
  };
}

/** The title a chat gets from its first prompt until a better one comes. */
export function seedTitle(text: string): string {
  const line = text.trim().split("\n", 1).at(0) ?? "";
  return line.length > SEED_TITLE ? `${line.slice(0, SEED_TITLE - 1).trimEnd()}…` : line;
}
