// What a thread's actor holds of its thread (docs/chat.md 4.5): the rows that decide what it
// does next, read once and then followed through every commit; the turn a session works on,
// which lives as long as its run does; and the clients told of each commit as it's made.

import type { MessageId, RequestId, RunId } from "../protocol/ids";
import type { Delivered, Rows, ThreadSnapshot } from "../protocol/methods";
import type { Item, Mode, Prompt, Request, Run, Thread, ThreadSummary } from "../protocol/model";
import { eventLine, HostError } from "../protocol/wire";
import type { DriverEvent, ItemEvent } from "../providers/provider";
import type { Change, Store } from "../store/store";
import { lost, moved, notice, orphaned, queuedRun, settled, stopBy } from "./records";
import { isLive, isSettled, summary } from "./rules";
import { LiveTurn } from "./turn";

/** Where a thread's changes go: a client that opened it. */
export interface Subscriber {
  readonly send: (line: string) => void;
}

/** What's said of a turn that only its rows follow. */
type Heard = ItemEvent | Extract<DriverEvent, { type: "session.ready" | "turn.started" | "usage" }>;

/** How many runs a first open sends (4.3.2). */
const OPEN_RUNS = 30;

export class ThreadState {
  /** Whether the chat is going back to an earlier message, which holds off what would start. */
  goingBack = false;
  readonly #store: Store;
  readonly #summarized: (summary: ThreadSummary) => void;
  #thread: Thread;
  /** The runs not yet settled, oldest first: the live one, if any, and the queue. */
  #unsettled: readonly Run[];
  /** The latest run that ran and ended. */
  #ended: Run | undefined;
  /** The requests that wait for the person, oldest first. */
  #pending: readonly Request[];
  #ordinal: number;
  /** The mode the provider said its session runs in, for a chat that names none. */
  #mode: Mode | null = null;
  /** The summary watchers were last told. */
  #told: ThreadSummary | undefined;
  readonly #subscribers = new Set<Subscriber>();
  /** The turn a session works on, whose items are newer here than in the store. */
  #turn: LiveTurn | undefined;

  constructor(store: Store, summarized: (summary: ThreadSummary) => void, thread: Thread) {
    this.#store = store;
    this.#summarized = summarized;
    this.#thread = thread;
    const state = store.state(thread.id);
    this.#unsettled = state.unsettled;
    this.#ended = state.ended;
    this.#pending = state.pending;
    this.#ordinal = store.nextOrdinal(thread.id);
  }

  get thread(): Thread {
    return this.#thread;
  }

  get mode(): Mode | null {
    return this.#mode;
  }

  get pending(): readonly Request[] {
    return this.#pending;
  }

  get turn(): LiveTurn | undefined {
    return this.#turn;
  }

  /** Starts the turn of `run`, which ends with the commit that settles the run. */
  startTurn(run: Run): LiveTurn {
    const turn = new LiveTurn(run.id, (item) => {
      this.#store.keepText(item);
    });
    this.#turn = turn;
    return turn;
  }

  summary(): ThreadSummary {
    const state = { unsettled: this.#unsettled, ended: this.#ended, pending: this.#pending };
    return summary(this.#thread, state);
  }

  live(): Run | undefined {
    return this.#unsettled.find(isLive);
  }

  /** The run a thread is on, or was on last. */
  latest(): Run | undefined {
    return this.live() ?? this.#ended;
  }

  /** A new run of `prompt`, unless the chat's going back, which nothing may start under. */
  queue(prompt: Prompt, at: string): Run {
    if (this.goingBack) {
      throw new HostError("conflict", "The chat is going back to an earlier message.");
    }
    return queuedRun(this.#thread, prompt, at);
  }

  /** The run the queue starts next. */
  next(): Run | undefined {
    return this.#unsettled.find((run) => run.status === "queued");
  }

  /** The agent has `tasks` running in the background now. */
  background(tasks: number): void {
    if (tasks !== this.#thread.backgroundTasks) {
      this.commit({ thread: { ...this.#thread, backgroundTasks: tasks } });
    }
  }

  /** The session went, and what ran in it beside its turn: its background, and its subagents'
   * `requests`, released. */
  sessionGone(requests: readonly Request[]): void {
    const thread = this.#quieted(null);
    if (thread !== null || requests.length > 0) {
      this.commit({ thread, requests });
    }
  }

  /** `changed`, or else the thread, with nothing in the background; `changed` as it is when
   * that's so already. */
  #quieted(changed: Thread | null): Thread | null {
    const thread = changed ?? this.#thread;
    return thread.backgroundTasks === 0 ? changed : { ...thread, backgroundTasks: 0 };
  }

  /** The thread with its queue held, when something is queued and it isn't yet. */
  holding(): Thread | null {
    const nothingToHold = this.#thread.queueHeld || this.next() === undefined;
    return nothingToHold ? null : { ...this.#thread, queueHeld: true };
  }

  /** Run `runId` and those after it, or why there's no going back to it. */
  fromRun(runId: RunId): readonly [Run, ...Run[]] {
    const [run, ...after] = this.#store.runsFrom(this.#thread.id, runId);
    if (run?.id !== runId) {
      throw new HostError("run_not_found", "The chat has no such message.");
    }
    return [run, ...after];
  }

  /** The queued run `runId`, or why there's none to change. */
  queued(runId: RunId): Run {
    const run = this.#unsettled.find((each) => each.id === runId);
    if (run?.status === "queued") {
      return run;
    }
    if (this.#store.run(this.#thread.id, runId) === undefined) {
      throw new HostError("run_not_found", "The chat has no such message.");
    }
    throw new HostError("conflict", "That message isn't queued anymore.");
  }

  /** The pending request `requestId`, or why there's none to answer. */
  pendingRequest(requestId: RequestId): Request {
    const request = this.#pending.find((each) => each.id === requestId);
    if (request !== undefined) {
      return request;
    }
    if (this.#store.request(this.#thread.id, requestId) === undefined) {
      throw new HostError("request_not_found", "The chat has no such request.");
    }
    throw new HostError("conflict", "The request was settled already.");
  }

  /** What message `messageId` became, when it was sent before (T3 pitfall 11). */
  sent(messageId: MessageId): { runId: RunId; delivered: Delivered } | undefined {
    const id = this.#thread.id;
    const item = this.#store.userMessage(id, messageId);
    if (item?.kind === "user_message") {
      const steered = item.payload.delivered === "steer";
      return { runId: item.runId, delivered: steered ? "steered" : "started" };
    }
    const run = this.#store.runOf(id, messageId);
    return run === undefined ? undefined : { runId: run.id, delivered: "queued" };
  }

  /** The timeline place of the thread's next item. */
  place(): number {
    const ordinal = this.#ordinal;
    this.#ordinal += 1;
    return ordinal;
  }

  /** Registers `subscriber` and takes the snapshot it starts from, at once (4.3.4). */
  open(subscriber: Subscriber, sinceSeq: number | null): ThreadSnapshot {
    this.#subscribers.add(subscriber);
    const id = this.#thread.id;
    const rows = sinceSeq === null
      ? this.#store.latest(id, OPEN_RUNS)
      : this.#store.since(id, sinceSeq);
    return { thread: this.#thread, ...rows, items: this.#overlay(rows.items, sinceSeq !== null) };
  }

  close(subscriber: Subscriber): void {
    this.#subscribers.delete(subscriber);
  }

  /** Lets go of the subscribers and the turn, for a thread being deleted. */
  dispose(): void {
    this.#subscribers.clear();
    this.#turn?.end();
    this.#turn = undefined;
  }

  history(before: RunId | null, runs: number): Rows {
    const { id } = this.#thread;
    const rows = before === null
      ? this.#store.latest(id, runs)
      : this.#store.before(id, before, runs);
    return { ...rows, items: this.#overlay(rows.items, false) };
  }

  /** Writes one step's change, follows it here, and tells the subscribers and watchers. */
  commit(change: Partial<Change>): void {
    const committed = this.#store.commit(this.#thread.id, change);
    this.#thread = committed.thread ?? this.#thread;
    this.#unsettled = kept(this.#unsettled, committed.runs, (run) => !isSettled(run));
    for (const run of committed.runs) {
      const ran = isSettled(run) && run.status !== "removed";
      if (ran && (run.endedAt ?? "") >= (this.#ended?.endedAt ?? "")) {
        this.#ended = run;
      }
    }
    this.#pending = kept(this.#pending, committed.requests, (row) => row.status === "pending");
    if (committed.runs.some((run) => run.status === "removed")) {
      // What ended last, and what waits for the person, may be among what's gone.
      const state = this.#store.state(this.#thread.id);
      this.#ended = state.ended;
      this.#pending = state.pending;
    }
    const turn = this.#turn;
    turn?.remember(committed.items);
    // Ended once the commit has its items whole, so it keeps nothing stale.
    const ends = (run: Run): boolean => run.id === turn?.runId && isSettled(run);
    if (committed.runs.some(ends)) {
      turn?.end();
      this.#turn = undefined;
    }
    if (this.#subscribers.size > 0) {
      this.broadcast(eventLine("thread.changed", {
        threadId: this.#thread.id,
        seq: committed.seq,
        thread: committed.thread,
        runs: [...committed.runs],
        items: [...committed.items],
        requests: [...committed.requests],
      }));
    }
    // A subagent's thread shows where it started, not in the lists.
    if (this.#thread.parent === null) {
      const shown = this.summary();
      if (!Bun.deepEquals(shown, this.#told)) {
        this.#told = shown;
        this.#summarized(shown);
      }
    }
  }

  /** What's said of the turn, that only its rows follow, while one runs. */
  heard(event: Heard): void {
    const live = this.live();
    const turn = this.#turn;
    if (live === undefined || turn === undefined) {
      return;
    }
    switch (event.type) {
      case "session.ready": {
        this.#mode = event.mode;
        if (live.permission === null && event.mode !== null) {
          this.commit({ runs: [{ ...live, permission: event.mode }] });
        }
        return;
      }
      case "turn.started": {
        if (live.status === "starting" && event.promptId === turn.runId) {
          // Where it went on from is its conversation's latest now, with this turn on it.
          const { resumeAt } = this.#thread;
          const thread = resumeAt === null ? null : { ...this.#thread, resumeAt: null };
          this.commit({ thread, runs: [moved(live, "running")] });
        }
        return;
      }
      case "usage": {
        this.commit({ runs: [{ ...live, usage: event.usage }] });
        return;
      }
      case "item.started":
      case "item.updated": {
        this.commit({ items: [turn.seen(event, live, () => this.place())] });
        return;
      }
      case "item.appended": {
        // Streamed text is told as it comes, and kept by the turn (4.5.8).
        const grown = turn.append(event.key, event.text);
        if (grown !== undefined) {
          const params = { threadId: live.threadId, itemId: grown.id, text: event.text };
          this.broadcast(eventLine("item.appended", params));
        }
        return;
      }
      case "notice": {
        this.commit({ items: [notice(live, this.place(), event.level, event.text)] });
      }
    }
  }

  /** Settles what a host that stopped left live (4.1, startup recovery). */
  recover(at: string): void {
    const runs = this.#unsettled.filter(isLive).map((run) => {
      const stop = run.stop ?? stopBy("host", at);
      return settled(run, "interrupted", [], { at, stop }).run;
    });
    // A background subagent's row outlives its run, so this looks past the runs that were live.
    const items = this.#store.openItems(this.#thread.id).map((item) => lost(item, at));
    const requests = this.#pending.flatMap((request) => orphaned(request, at) ?? []);
    // What ran in the background went with the host's sessions.
    const thread = this.#quieted(this.holding());
    if (runs.length > 0 || items.length > 0 || requests.length > 0 || thread !== null) {
      this.commit({ thread, runs, items, requests });
    }
  }

  broadcast(line: string): void {
    for (const subscriber of this.#subscribers) {
      subscriber.send(line);
    }
  }

  #overlay(items: readonly Item[], all: boolean): Item[] {
    return this.#turn?.overlay(items, all) ?? [...items];
  }
}

/** `list` with `rows` in place of the rows of the same ids, kept where `keep` says so. */
function kept<Row extends { readonly id: string }>(
  list: readonly Row[],
  rows: readonly Row[],
  keep: (row: Row) => boolean,
): readonly Row[] {
  if (rows.length === 0) {
    return list;
  }
  const changed = new Set(rows.map((row) => row.id));
  const rest = list.filter((row) => !changed.has(row.id));
  return [...rest, ...rows.filter(keep)].toSorted(byId);
}

/** Ids are UUIDv7, so their order is the order they were made in. */
function byId(a: { readonly id: string }, b: { readonly id: string }): number {
  if (a.id === b.id) {
    return 0;
  }
  return a.id < b.id ? -1 : 1;
}
