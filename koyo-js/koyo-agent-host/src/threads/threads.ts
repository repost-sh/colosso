// The host's threads (docs/chat.md 4.1, 4.3.2): each thread's actor, made the first time it's
// wanted and kept; the clients that watch the list of them; and the live sessions, of which at
// most four stay open, the least recently used closed first when a fifth is wanted.

import { log } from "../log";
import type { RunId, ThreadId } from "../protocol/ids";
import type { FilesBack, Params } from "../protocol/methods";
import type { Prompt, ProviderId, Thread, ThreadSummary } from "../protocol/model";
import { eventLine, HostError } from "../protocol/wire";
import type { Config, Provider } from "../providers/provider";
import type { Store } from "../store/store";
import { ThreadActor } from "./actor";
import { draftAttachments, removeAttachments, sweepAttachments } from "./attachments";
import type { Context } from "./context";
import { newThread } from "./records";
import { filesBack, rewind } from "./rewind";
import { summary } from "./rules";
import type { Subscriber } from "./state";

/** How many sessions stay open at once (4.1). */
const MAX_SESSIONS = 4;

export interface Options {
  readonly store: Store;
  readonly providers: readonly Provider[];
  /** The folder the messages' attachments are in. */
  readonly attachments: string;
  /** The app's drafts, whose attachments no message names yet. */
  readonly drafts: string;
  readonly now: () => string;
}

export class Threads {
  readonly #options: Options;
  readonly #providers: ReadonlyMap<ProviderId, Provider>;
  readonly #actors = new Map<ThreadId, ThreadActor>();
  readonly #watchers = new Set<Subscriber>();
  /** The actors with a session open, the least recently used first. */
  #sessions: ThreadActor[] = [];
  /** Koyo's settings for the agents, as the app last gave them. */
  #config: Config = { claudePath: null, defaultMode: null, koyoTools: null };

  constructor(options: Options) {
    this.#options = options;
    this.#providers = new Map(options.providers.map((provider) => [provider.id, provider]));
  }

  /** Settles what a host that stopped left live, as this one starts (4.1). */
  recover(): void {
    const { store, attachments, drafts, now } = this.#options;
    for (const id of store.unsettledThreads()) {
      // The thread with its row first: a host that stops between the two has this to do again.
      const parent = store.thread(id)?.parent;
      if (parent !== null && parent !== undefined) {
        this.actor(parent.threadId);
      }
      this.actor(id);
    }
    try {
      const named = new Set([...store.allAttachments(), ...draftAttachments(drafts)]);
      sweepAttachments(attachments, named, Date.parse(now()));
    } catch (error) {
      log.warn("Attachments no message names weren't swept", error);
    }
  }

  get config(): Config {
    return this.#config;
  }

  /** Keeps Koyo's settings, which the next session a thread opens follows. */
  configure(config: Config): void {
    this.#config = config;
  }

  /** The provider `id`, or the host's first for `null`. */
  provider(id: ProviderId | null): Provider {
    const [first] = this.#providers.values();
    const provider = id === null ? first : this.#providers.get(id);
    if (provider === undefined) {
      throw noProvider(id);
    }
    return provider;
  }

  /**
   * Every thread as lists show it, and from now on each change to what they show: not a
   * subagent's, which shows where it started. A thread no one has worked on since the host
   * started is summed up from the store as it is.
   */
  watch(watcher: Subscriber): ThreadSummary[] {
    this.#watchers.add(watcher);
    const { store } = this.#options;
    return store.chats().map((thread) =>
      this.#actors.get(thread.id)?.summary() ?? summary(thread, store.state(thread.id)));
  }

  create(params: Params<"threads.create">): Thread {
    const { id } = this.provider(params.provider);
    const thread = newThread(params, id, this.#options.now());
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

  /** Ends the thread's live work and forgets it, its rows, its files and its subagents'. */
  delete(id: ThreadId): void {
    const actor = this.actor(id);
    const { store, attachments } = this.#options;
    const files = store.attachments(id);
    actor.dispose();
    // Once they ended with its session, so that nothing writes them again.
    for (const child of store.children(id)) {
      this.delete(child);
    }
    store.delete(id);
    this.#actors.delete(id);
    this.#sessions = this.#sessions.filter((each) => each !== actor);
    removeAttachments(attachments, files);
    this.#tell(eventLine("thread.removed", { threadId: id }));
  }

  /**
   * What going back to before run `runId` of thread `id` could do to the files, if anything: only
   * ever an offer, so what goes wrong finding out leaves the person going back without it.
   */
  async filesBack(id: ThreadId, runId: RunId): Promise<FilesBack | null> {
    const actor = this.actor(id);
    const othersAtWork = (): boolean => this.#othersAtWork(actor);
    try {
      return await filesBack(actor, this.#providerOf(actor.thread), runId, othersAtWork);
    } catch (error) {
      log.warn("Couldn't find out what going back would do to the files", error);
      return null;
    }
  }

  /**
   * Goes back to before run `runId` of thread `id`, its files too if `restoreFiles`, and says what
   * the person said then (C11).
   */
  async rewind(id: ThreadId, runId: RunId, restoreFiles: boolean): Promise<Prompt> {
    const actor = this.actor(id);
    const prompt = await rewind(actor, this.#providerOf(actor.thread), runId, {
      restoreFiles,
      othersAtWork: () => this.#othersAtWork(actor),
      now: () => this.#options.now(),
    });
    for (const child of this.#options.store.orphans(id)) {
      this.delete(child);
    }
    return prompt;
  }

  /** Whether another chat is at work in the checkout of `actor`'s. */
  #othersAtWork(actor: ThreadActor): boolean {
    const { cwd } = actor.thread;
    return [...this.#actors.values()].some((other) => {
      return other !== actor && other.thread.cwd === cwd && other.busy();
    });
  }

  /** Whether any thread has a live run, runs a task in the background or waits on its person. */
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

  #providerOf(thread: Thread): Provider {
    return this.#providers.get(thread.provider) ?? missing(thread.provider);
  }

  #context(thread: Thread): Context {
    return {
      store: this.#options.store,
      provider: this.#providerOf(thread),
      config: () => this.#config,
      attachments: this.#options.attachments,
      now: this.#options.now,
      summarized: (shown) => {
        this.#tell(eventLine("thread.summary", shown));
      },
      sessionUsed: (actor) => {
        this.#used(actor);
      },
      spawn: (child, prompt, stop) => {
        const actor = ThreadActor.subagent(this.#context(child), child, prompt, stop);
        this.#actors.set(child.id, actor);
        return actor;
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

function noProvider(id: ProviderId | null): HostError {
  const named = id === null ? "" : ` \`${id}\``;
  return new HostError("provider_unavailable", `Koyo has no provider${named}.`);
}

/** The provider of a thread whose provider this host doesn't have: every turn fails. */
function missing(id: ProviderId): Provider {
  const refusal = noProvider(id);
  return {
    id,
    capabilities: { steer: false, resume: false, images: false, compact: false, plan: false },
    setConfig: () => {
      // It has nothing to follow them with.
    },
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
    entryBefore: async () => await Promise.reject(refusal),
    filesAt: async () => await Promise.reject(refusal),
    restoreFiles: async () => { await Promise.reject(refusal); },
    title: async () => await Promise.reject(refusal),
  };
}
