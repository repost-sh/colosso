// One thread's actor (docs/chat.md C5, 4.5). Every decision about the thread and every write
// of its rows is a step: a method that runs to its end without awaiting, each commit it makes
// one transaction the thread's subscribers are told of as it's made (a run settling and the
// next one starting are two). What a session reports is handled in a step of its own, queued
// behind the one running, so no step runs inside another, and a snapshot is never taken
// between two commits of one step.

import { errorMessage, log } from "../log";
import type { RequestId, RunId } from "../protocol/ids";
import type { Delivered, Delivery, Rows, ThreadSnapshot } from "../protocol/methods";
import type {
  Answer,
  Attachment,
  Item,
  Mode,
  Prompt,
  Response,
  Run,
  Stop,
  Thread,
  ThreadSummary,
} from "../protocol/model";
import { eventLine, HostError } from "../protocol/wire";
import {
  type Config,
  type DriverEvent,
  type Outcome,
  processFailure,
  type Provider,
} from "../providers/provider";
import type { Store } from "../store/store";
import {
  answered,
  answeredByNextMessage,
  type Ending,
  moved,
  notice,
  queuedRun,
  seedTitle,
  settingsChanged,
  settled,
  stopBy,
  textPrompt,
  type ThreadChange,
  titleFrom,
  turnInput,
  updatedThread,
  userMessage,
} from "./records";
import { Requests } from "./requests";
import { compacts } from "./rules";
import { SessionSlot, specFor } from "./session";
import { type Subscriber, ThreadState } from "./state";

/** What an actor works with, from the threads it's one of. */
export interface Context {
  readonly store: Store;
  /** The provider the thread's agent runs on. */
  readonly provider: Provider;
  /** Koyo's settings for the agents, as they are now. */
  readonly config: () => Config;
  /** The folder the messages' attachments are in. */
  readonly attachments: string;
  readonly now: () => string;
  /** Tells the threads' watchers that what lists show of the thread changed. */
  readonly summarized: (summary: ThreadSummary) => void;
  /** Tells the threads a session ran a turn, so the least recently used one closes first. */
  readonly sessionUsed: (actor: ThreadActor) => void;
}

/** How long a shutdown waits for a stopped turn to end before it ends its process. */
const SHUTDOWN_MS = 5_000;
const IMPLEMENT = "Implement the plan above.";

export class ThreadActor {
  readonly #cx: Context;
  readonly #state: ThreadState;
  readonly #requests: Requests;
  readonly #session: SessionSlot;
  /** The mode the provider said its session runs in, for a chat that names none. */
  #mode: Mode | null = null;
  /** A deleted thread's actor takes no more steps. */
  #disposed = false;

  private constructor(cx: Context, thread: Thread) {
    this.#cx = cx;
    this.#state = new ThreadState(cx.store, cx.summarized, thread);
    const later = (work: () => void): void => {
      queueMicrotask(() => {
        this.#step(work);
      });
    };
    this.#requests = new Requests(this.#state, cx.now, later);
    this.#session = new SessionSlot(cx.provider, later, {
      event: (event) => {
        this.#on(event);
      },
      ask: (payload, signal, waiter) => {
        this.#requests.ask(payload, signal, waiter);
      },
    });
  }

  /** The actor of a new thread, which it writes. */
  static create(cx: Context, thread: Thread): ThreadActor {
    const actor = new ThreadActor(cx, thread);
    actor.#state.commit({ thread });
    return actor;
  }

  /** The actor of a stored thread, which first settles what a host that stopped left live. */
  static load(cx: Context, thread: Thread): ThreadActor {
    const actor = new ThreadActor(cx, thread);
    actor.#state.recover(cx.now());
    return actor;
  }

  get thread(): Thread {
    return this.#state.thread;
  }

  summary(): ThreadSummary {
    return this.#state.summary();
  }

  /** Whether a run is live or the agent waits on the person: what keeps the host up. */
  busy(): boolean {
    return this.#state.live() !== undefined || this.#requests.waiting();
  }

  hasSession(): boolean {
    return this.#session.live !== undefined;
  }

  // What clients see.

  /** Registers `subscriber` and takes the snapshot it starts from, in one step (4.3.4). */
  open(subscriber: Subscriber, sinceSeq: number | null): ThreadSnapshot {
    return this.#state.open(subscriber, sinceSeq);
  }

  close(subscriber: Subscriber): void {
    this.#state.close(subscriber);
  }

  history(before: RunId, runs: number): Rows {
    return this.#state.history(before, runs);
  }

  // Turns (4.5.1 to 4.5.4).

  /** Starts `prompt`, joins it to the running turn, or queues it. */
  send(prompt: Prompt, delivery: Delivery): { runId: RunId; delivered: Delivered } {
    const earlier = this.#state.sent(prompt.messageId);
    if (earlier !== undefined) {
      return earlier;
    }
    const live = this.#state.live();
    if (live === undefined) {
      return { runId: this.#begin(this.#queue(prompt)), delivered: "started" };
    }
    const steer = delivery === "auto" ? this.#steer(live, prompt) : undefined;
    if (steer !== undefined) {
      this.#state.commit({ items: [steer] });
      return { runId: live.id, delivered: "steered" };
    }
    const run = this.#queue(prompt);
    this.#state.commit({ runs: [run] });
    return { runId: run.id, delivered: "queued" };
  }

  /** A turn of the provider's `/compact`, after the one running. */
  compact(): RunId {
    if (!this.#cx.provider.capabilities.compact) {
      throw new HostError("conflict", "This chat's agent can't compact its conversation.");
    }
    return this.send(textPrompt("/compact"), "queue").runId;
  }

  /** Stops the live run; `force` ends its process rather than asking it to stop. */
  stop(force: boolean): void {
    const live = this.#state.live();
    if (live === undefined) {
      return;
    }
    const stop = live.stop ?? stopBy("person", this.#cx.now());
    if (force) {
      this.#session.drop()?.kill();
      this.#settle("interrupted", { stop: { ...stop, force: true } });
    } else if (live.status !== "stopping") {
      this.#stopping(live, stop);
    }
  }

  // The queue (4.5.3).

  removeQueued(runId: RunId): void {
    this.#state.commit({ runs: [moved(this.#state.queued(runId), "removed")] });
  }

  editQueued(runId: RunId, text: string, attachments: Attachment[]): void {
    const run = this.#state.queued(runId);
    this.#state.commit({ runs: [{ ...run, prompt: { ...run.prompt, text, attachments } }] });
  }

  /** Starts a queued run now, or joins it to the running turn, whether the queue is held. */
  sendNow(runId: RunId): Delivered {
    const run = this.#state.queued(runId);
    const live = this.#state.live();
    if (live === undefined) {
      this.#begin(run);
      return "started";
    }
    const steer = this.#steer(live, run.prompt);
    if (steer === undefined) {
      return "queued";
    }
    this.#state.commit({ runs: [moved(run, "removed")], items: [steer] });
    return "steered";
  }

  /** Releases a held queue. */
  resume(): void {
    const { thread } = this.#state;
    if (thread.queueHeld) {
      this.#state.commit({ thread: { ...thread, queueHeld: false } });
    }
    this.#next();
  }

  // Requests (4.5.5).

  respond(requestId: RequestId, response: Response): void {
    this.#requests.answer(requestId, response);
    // Implementing a plan starts a turn in the mode the chat had before planning.
    if (response.kind === "plan" && response.decision === "implement") {
      this.update({ plan: false });
      this.send(textPrompt(IMPLEMENT), "auto");
    }
  }

  // The thread itself.

  /** Changes the title or the settings; a live session takes them before the next turn. */
  update(change: ThreadChange): Thread {
    const before = this.#state.thread;
    const after = updatedThread(before, change, this.#cx.now());
    // Told what already holds, nothing happened, so nobody hears of it.
    if (!Bun.deepEquals(after, before)) {
      this.#state.commit({ thread: after });
      this.#session.configure(settingsChanged(before, after, this.#cx.config().defaultMode));
    }
    return this.#state.thread;
  }

  /** The person has seen what the thread did. */
  read(): void {
    this.#state.commit({ thread: { ...this.#state.thread, readAt: this.#cx.now() } });
  }

  /** Closes the session when nothing uses it; whether it did. */
  closeSession(): boolean {
    if (this.busy()) {
      return false;
    }
    this.#session.close();
    return true;
  }

  /** Stops the live run as the host (4.1), waits a while for it to end, then closes. */
  async shutdown(): Promise<void> {
    const live = this.#state.live();
    if (live !== undefined) {
      const ended = this.#state.turn?.ended;
      if (live.status !== "stopping") {
        this.#stopping(live, stopBy("host", this.#cx.now()));
      }
      await within(ended, SHUTDOWN_MS);
      // A turn that didn't stop in time ends with its process.
      this.stop(true);
    }
    await this.#session.drop()?.close();
  }

  /** Ends everything live, for a thread being deleted. */
  dispose(): void {
    this.#disposed = true;
    this.#requests.abandon();
    this.#session.drop()?.kill();
    this.#state.dispose();
  }

  // Runs.

  #queue(prompt: Prompt): Run {
    return queuedRun(this.#state.thread, prompt, this.#cx.now());
  }

  /** Starts `queued`: writes it starting with its message, then gives the session its turn. */
  #begin(queued: Run): RunId {
    const at = this.#cx.now();
    const { thread } = this.#state;
    const sessionId = thread.sessionId ?? crypto.randomUUID();
    const run = moved(queued, "starting", {
      startedAt: at,
      model: thread.model,
      effort: thread.effort,
      permission: thread.permission ?? this.#mode,
      plan: thread.plan,
    });
    const answer: Answer = { kind: "message", messageId: run.prompt.messageId };
    this.#state.commit({
      thread: {
        ...thread,
        sessionId,
        title: thread.title === "" ? seedTitle(run.prompt.text) : thread.title,
        updatedAt: at,
      },
      runs: [run],
      items: [userMessage(run, this.#state.place(), run.prompt, "turn")],
      requests: this.#state.pending
        .filter(answeredByNextMessage)
        .map((request) => answered(request, answer, at)),
    });
    const turn = this.#state.startTurn(run);
    try {
      const spec = specFor(thread, sessionId, this.#cx.config().defaultMode);
      const session = this.#session.ensure(spec);
      session.start(turnInput(turn.promptId, run.prompt, this.#cx.attachments, "turn"));
    } catch (error) {
      log.error("A turn wouldn't start", error);
      this.#settle("failed", { failure: processFailure(errorMessage(error)) });
      return run.id;
    }
    this.#cx.sessionUsed(this);
    // A message of pictures alone leaves the title to the first words.
    if (thread.title === "" && run.prompt.text !== "") {
      this.#title(run.prompt.text);
    }
    return run.id;
  }

  /** `prompt`'s item, once the running turn took it; `undefined` when it can't join (4.5.2). */
  #steer(live: Run, prompt: Prompt): Item | undefined {
    const session = this.#session.live;
    const steerable = live.status === "running" && !compacts(live)
      && this.#cx.provider.capabilities.steer;
    if (session === undefined || !steerable) {
      return undefined;
    }
    const input = turnInput(crypto.randomUUID(), prompt, this.#cx.attachments, "steer");
    return session.steer(input)
      ? userMessage(live, this.#state.place(), prompt, "steer")
      : undefined;
  }

  /** Moves `live` to stopping, which holds the queue, and asks its session to stop it. */
  #stopping(live: Run, stop: Stop): void {
    const run = moved(live, "stopping", { stop });
    this.#state.commit({ thread: this.#state.holding(), runs: [run] });
    if (!this.#session.interrupt()) {
      this.#settle("interrupted");
    }
  }

  /** Ends the live run as `outcome`, with what it left open, then starts the next. */
  #settle(outcome: Outcome, ending: Ending = {}): void {
    const live = this.#state.live();
    if (live === undefined) {
      return;
    }
    const at = this.#cx.now();
    const ended = settled(live, outcome, this.#state.turn?.open() ?? [], { ...ending, at });
    // A run that didn't complete holds what's queued behind it, for the person to release.
    const thread = (outcome === "completed" ? null : this.#state.holding()) ?? this.#state.thread;
    this.#state.commit({
      thread: { ...thread, updatedAt: at },
      runs: [ended.run],
      items: ended.items,
      requests: this.#requests.release(ended.run, at),
    });
    this.#session.idleSoon(() => this.busy());
    this.#next();
  }

  #next(): void {
    const next = this.#state.next();
    if (next !== undefined && this.#state.live() === undefined && !this.#state.thread.queueHeld) {
      this.#begin(next);
    }
  }

  /**
   * Asks the provider for a title and makes one of its reply, written only while the seed is
   * still the title (4.5.7).
   */
  #title(prompt: string): void {
    this.#cx.provider.title(prompt, this.#state.thread.cwd).then((reply) => {
      this.#step(() => {
        const { thread } = this.#state;
        const title = reply === undefined ? undefined : titleFrom(reply);
        if (title !== undefined && thread.titleSource === "seed") {
          this.#state.commit({ thread: { ...thread, title, titleSource: "generated" } });
        }
      });
    }, (error: unknown) => {
      log.warn("A title didn't come", error);
    });
  }

  // What the session reports.

  #on(event: DriverEvent): void {
    if (event.type === "session.ended") {
      this.#sessionEnded(event.error);
      return;
    }
    const live = this.#state.live();
    const { turn } = this.#state;
    if (live === undefined || turn === undefined) {
      return;
    }
    switch (event.type) {
      case "session.ready": {
        this.#mode = event.mode;
        if (live.permission === null && event.mode !== null) {
          this.#state.commit({ runs: [{ ...live, permission: event.mode }] });
        }
        return;
      }
      case "turn.started": {
        if (live.status === "starting" && event.promptId === turn.promptId) {
          this.#state.commit({ runs: [moved(live, "running")] });
        }
        return;
      }
      case "item.started":
      case "item.updated": {
        this.#state.commit({ items: [turn.seen(event, live, () => this.#state.place())] });
        return;
      }
      case "item.appended": {
        // Streamed text is told as it comes, and kept by the turn (4.5.8).
        const grown = turn.append(event.key, event.text);
        if (grown !== undefined) {
          const params = { threadId: live.threadId, itemId: grown.id, text: event.text };
          this.#state.broadcast(eventLine("item.appended", params));
        }
        return;
      }
      case "notice": {
        const item = notice(live, this.#state.place(), event.level, event.text);
        this.#state.commit({ items: [item] });
        return;
      }
      case "request": {
        this.#requests.open(live, event.request, undefined);
        return;
      }
      case "usage": {
        this.#state.commit({ runs: [{ ...live, usage: event.usage }] });
        return;
      }
      case "turn.finished": {
        this.#settle(event.outcome, { failure: event.failure, usage: event.usage });
      }
    }
  }

  #sessionEnded(error: string | null): void {
    this.#session.drop();
    const live = this.#state.live();
    if (live?.status === "stopping") {
      this.#settle("interrupted");
    } else if (live !== undefined) {
      const failure = processFailure(error ?? "The agent's process ended.");
      this.#settle("failed", { failure });
    }
  }

  /** Runs `work` as a step that, failing, fails alone; a deleted thread takes none. */
  #step(work: () => void): void {
    if (this.#disposed) {
      return;
    }
    try {
      work();
    } catch (error) {
      log.error(`A step of chat ${this.#state.thread.id} failed`, error);
    }
  }
}

/** Waits for `promise`, if there's one, for `ms` at most. */
async function within(promise: Promise<unknown> | undefined, ms: number): Promise<void> {
  const late = Promise.withResolvers<undefined>();
  const timer = setTimeout(late.resolve, ms, undefined);
  await Promise.race([promise ?? Promise.resolve(), late.promise]);
  clearTimeout(timer);
}
