// What a run may become and what a thread shows (docs/chat.md 4.4, 4.5): the one function
// every status change goes through, and the status, needs and unread mark derived from the
// rows, never stored, so they can't drift from them.

import type {
  Item,
  Request,
  Run,
  RunStatus,
  Status,
  Thread,
  ThreadSummary,
} from "../protocol/model";
import type { State } from "../store/store";

const MOVES: Readonly<Record<RunStatus, readonly RunStatus[]>> = {
  queued: ["starting", "removed"],
  // A turn may end before it's seen to start: a quick failure, a stop as it starts.
  starting: ["running", "stopping", "completed", "interrupted", "failed"],
  running: ["stopping", "completed", "interrupted", "failed"],
  // The barrier: once a stop is asked for, the run only ends.
  stopping: ["completed", "interrupted", "failed"],
  completed: [],
  interrupted: [],
  failed: [],
  removed: [],
};

/** Whether a run in `from` may move to `to`. */
export function canMove(from: RunStatus, to: RunStatus): boolean {
  return MOVES[from].includes(to);
}

/** Whether a run is past starting and not yet ended: its session works on it. */
export function isLive(run: Run): boolean {
  return run.status === "starting" || run.status === "running" || run.status === "stopping";
}

export function isSettled(run: Run): boolean {
  return MOVES[run.status].length === 0;
}

/** Whether an item may still change: the turn's end closes it. */
export function isOpen(item: Item): boolean {
  return item.status === "pending" || item.status === "running";
}

/** Whether a prompt of `text` compacts the conversation: Koyo's `/compact`. */
export function isCompact(text: string): boolean {
  return /^\/compact(?:\s|$)/u.test(text);
}

/** Whether `run` compacts the conversation, which no message can join (4.5.1). */
export function compacts(run: Run): boolean {
  return isCompact(run.prompt.text);
}

/** What a thread is doing, from its unsettled runs and the latest that ended. */
function status(state: State): Status {
  if (state.unsettled.some(isLive)) {
    return "working";
  }
  return state.ended?.status === "failed" ? "failed" : "idle";
}

/** What the oldest request waiting for the person asks of them. */
function needs(pending: readonly Request[]): ThreadSummary["needs"] {
  return pending.at(0)?.payload.kind ?? null;
}

/** Whether a run ended since the person last looked. */
function unread(thread: Thread, state: State): boolean {
  const ended = state.ended?.endedAt ?? null;
  return ended !== null && (thread.readAt === null || ended > thread.readAt);
}

export function summary(thread: Thread, state: State): ThreadSummary {
  return {
    id: thread.id,
    cwd: thread.cwd,
    title: thread.title,
    provider: thread.provider,
    model: thread.model,
    status: status(state),
    needs: needs(state.pending),
    queued: state.unsettled.filter((run) => run.status === "queued").length,
    unread: unread(thread, state),
    updatedAt: thread.updatedAt,
  };
}
