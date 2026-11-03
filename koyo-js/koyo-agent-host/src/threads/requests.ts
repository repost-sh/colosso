// What a thread's agent waits on its person for (docs/chat.md 4.5.5, C14). A request is
// written pending with its place in the timeline; while the agent waits on it, the actor holds
// the promise the provider's answer comes through. The answer is checked against the row and
// written before it's handed over, so a turn ending as the person answers can't erase it.

import type { RequestId } from "../protocol/ids";
import type { Request, RequestPayload, Response, Run } from "../protocol/model";
import { HostError } from "../protocol/wire";
import { answered, cancelled, newRequest, orphaned } from "./records";
import type { ThreadState } from "./state";

/** An answer the agent waits on, to hand over or refuse. */
export interface Waiter {
  readonly resolve: (response: Response) => void;
  readonly reject: (error: Error) => void;
}

export class Requests {
  readonly #state: ThreadState;
  readonly #now: () => string;
  /** Runs work as a step of its own, after the one running. */
  readonly #later: (work: () => void) => void;
  readonly #waiters = new Map<RequestId, Waiter>();

  constructor(state: ThreadState, now: () => string, later: (work: () => void) => void) {
    this.#state = state;
    this.#now = now;
    this.#later = later;
  }

  /** Whether the agent waits on the person now. */
  waiting(): boolean {
    return this.#waiters.size > 0;
  }

  /** What the agent asks of the person during its turn, until it answers or withdraws. */
  ask(payload: RequestPayload, signal: AbortSignal, waiter: Waiter): void {
    const live = this.#state.live();
    if (live === undefined || signal.aborted) {
      waiter.reject(new Error("No turn waits for an answer."));
      return;
    }
    const request = this.open(live, payload, waiter);
    signal.addEventListener("abort", () => {
      this.#later(() => {
        this.#withdraw(request.id);
      });
    }, { once: true });
  }

  /** Writes a request and its place in the timeline; `waiter` when the agent waits on it. */
  open(live: Run, payload: RequestPayload, waiter: Waiter | undefined): Request {
    const place = { ordinal: this.#state.place(), at: this.#now() };
    const { request, item } = newRequest(live, payload, waiter !== undefined, place);
    if (waiter !== undefined) {
      this.#waiters.set(request.id, waiter);
    }
    this.#state.commit({ items: [item], requests: [request] });
    return request;
  }

  /** Writes the person's answer to a pending request, then hands it to the agent. */
  answer(requestId: RequestId, response: Response): void {
    const request = this.#state.pendingRequest(requestId);
    if (request.payload.kind !== response.kind) {
      const kind = request.payload.kind;
      throw new HostError("bad_request", `The request takes a response of kind \`${kind}\`.`);
    }
    const waiter = this.#waiters.get(requestId);
    // A plan is answered where it's shown; a question or an approval only while it's waited on.
    if (waiter === undefined && response.kind !== "plan") {
      throw new HostError("conflict", "The agent stopped waiting; send the answer as a message.");
    }
    this.#waiters.delete(requestId);
    this.#state.commit({ requests: [answered(request, response, this.#now())] });
    waiter?.resolve(response);
  }

  /**
   * The requests `run` waited on, now that it ended, each waiter told no answer comes: what
   * a stop by the host leaves is kept as a restart leaves it (4.1), the rest cancelled.
   */
  release(run: Run, at: string): Request[] {
    const byHost = run.stop?.by === "host";
    return this.#state.pending.flatMap((request) => {
      const waiter = this.#waiters.get(request.id);
      if (request.runId !== run.id || waiter === undefined) {
        return [];
      }
      this.#waiters.delete(request.id);
      waiter.reject(new Error("The turn ended."));
      return (byHost ? orphaned(request, at) : undefined) ?? cancelled(request, at);
    });
  }

  /** Tells every waiter no answer comes, for a thread being deleted. */
  abandon(): void {
    for (const waiter of this.#waiters.values()) {
      waiter.reject(new Error("The chat was deleted."));
    }
    this.#waiters.clear();
  }

  /** The agent stopped waiting on a request (the tool withdrawn, the turn interrupted). */
  #withdraw(requestId: RequestId): void {
    const waiter = this.#waiters.get(requestId);
    const request = this.#state.pending.find((each) => each.id === requestId);
    if (waiter === undefined || request === undefined) {
      return;
    }
    this.#waiters.delete(requestId);
    this.#state.commit({ requests: [cancelled(request, this.#now())] });
    waiter.reject(new Error("The agent withdrew the request."));
  }
}
