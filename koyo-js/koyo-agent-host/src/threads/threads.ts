// The host's threads (docs/chat.md 4.1, 4.3.2): each thread's actor, made the first time it's
// wanted and kept; the clients that watch the list of them; and the live sessions, of which at
// most four stay open, the least recently used closed first when a fifth is wanted.

import type { ThreadId } from "../protocol/ids";
import type { Params } from "../protocol/methods";
import type { ProviderId, Thread, ThreadSummary } from "../protocol/model";
import { eventLine, HostError } from "../protocol/wire";
import type { Provider } from "../providers/provider";
import type { Store } from "../store/store";
import { type Context, ThreadActor } from "./actor";
import { removeAttachments } from "./attachments";
import { newThread } from "./records";
import { summary } from "./rules";
import type { Subscriber } from "./state";

/** How many sessions stay open at once (4.1). */
const MAX_SESSIONS = 4;

export interface Options {
  readonly store: Store;
  readonly providers: readonly Provider[];
  /** The folder the messages' attachments are in. */
  readonly attachments: string;
  readonly now: () => string;
}

export class Threads {
  readonly #options: Options;
  readonly #providers: ReadonlyMap<ProviderId, Provider>;
  readonly #actors = new Map<ThreadId, ThreadActor>();
  readonly #watchers = new Set<Subscriber>();
  /** The actors with a session open, the least recently used first. */
  #sessions: ThreadActor[] = [];

  constructor(options: Options) {
    this.#options = options;
    this.#providers = new Map(options.providers.map((provider) => [provider.id, provider]));
  }

  /** Settles what a host that stopped left live, as this one starts (4.1). */
  recover(): void {
    for (const id of this.#options.store.unsettledThreads()) {
      this.actor(id);
    }
  }

  provider(id: ProviderId): Provider {
    const provider = this.#providers.get(id);
    if (provider === undefined) {
      throw noProvider(id);
    }
    return provider;
  }

  /**
   * Every thread as lists show it, and from now on each change to what they show. A thread
   * no one has worked on since the host started is summed up from the store as it is.
   */
  watch(watcher: Subscriber): ThreadSummary[] {
    this.#watchers.add(watcher);
    const { store } = this.#options;
    return store.threads().map((thread) =>
      this.#actors.get(thread.id)?.summary() ?? summary(thread, store.state(thread.id)));
  }

  create(params: Params<"threads.create">): Thread {
    this.provider(params.provider);
    const thread = newThread(params, this.#options.now());
    const actor = ThreadActor.create(this.#context(thread), thread);
    this.#actors.set(thread.id, actor);
    return actor.thread;
  }

  /** The thread's actor, or `thread_not_found`. */
  actor(id: ThreadId): ThreadActor {
    const known = this.#actors.get(id);
    if (known !== undefined) {
      return known;
    }
    const thread = this.#options.store.thread(id);
    if (thread === undefined) {
      throw new HostError("thread_not_found", "There is no such chat.");
    }
    const actor = ThreadActor.load(this.#context(thread), thread);
    this.#actors.set(id, actor);
    return actor;
  }

  /** Ends the thread's live work and forgets it, its rows and its files. */
  delete(id: ThreadId): void {
    const actor = this.actor(id);
    const { store, attachments } = this.#options;
    const files = store.attachments(id);
    actor.dispose();
    store.delete(id);
    this.#actors.delete(id);
    this.#sessions = this.#sessions.filter((each) => each !== actor);
    removeAttachments(attachments, files);
    this.#tell(eventLine("thread.removed", { threadId: id }));
  }

  /** Whether any thread has a live run or waits on its person. */
  busy(): boolean {
    return [...this.#actors.values()].some((actor) => actor.busy());
  }

  /** Forgets a client that went: it watches nothing and has nothing open. */
  forget(client: Subscriber): void {
    this.#watchers.delete(client);
    for (const actor of this.#actors.values()) {
      actor.close(client);
    }
  }

  /** Stops every live run as the host and closes every session (4.1). */
  async shutdown(): Promise<void> {
    const actors = [...this.#actors.values()];
    await Promise.all(actors.map(async (actor) => {
      await actor.shutdown();
    }));
  }

  #context(thread: Thread): Context {
    return {
      store: this.#options.store,
      provider: this.#providers.get(thread.provider) ?? missing(thread.provider),
      attachments: this.#options.attachments,
      now: this.#options.now,
      summarized: (shown) => {
        this.#tell(eventLine("thread.summary", shown));
      },
      sessionUsed: (actor) => {
        this.#used(actor);
      },
    };
  }

  #tell(line: string): void {
    for (const watcher of this.#watchers) {
      watcher.send(line);
    }
  }

  /** Keeps `actor`'s session, closing the least recently used idle ones past the limit. */
  #used(actor: ThreadActor): void {
    const open = [...this.#sessions.filter((each) => each !== actor && each.hasSession()), actor];
    let over = open.length - MAX_SESSIONS;
    const kept: ThreadActor[] = [];
    for (const each of open) {
      if (over > 0 && each !== actor && each.closeSession()) {
        over -= 1;
      } else {
        kept.push(each);
      }
    }
    this.#sessions = kept;
  }
}

function noProvider(id: ProviderId): HostError {
  return new HostError("provider_unavailable", `Koyo has no provider \`${id}\`.`);
}

/** The provider of a thread whose provider this host doesn't have: every turn fails. */
function missing(id: ProviderId): Provider {
  const refusal = noProvider(id);
  return {
    id,
    capabilities: { steer: false, resume: false, images: false, compact: false, plan: false },
    status: async () => await Promise.resolve({
      provider: id,
      state: "missing",
      executable: null,
      version: null,
      account: null,
      message: refusal.message,
    }),
    catalog: async () => await Promise.reject(refusal),
    open: () => {
      throw refusal;
    },
    title: async () => await Promise.reject(refusal),
  };
}
