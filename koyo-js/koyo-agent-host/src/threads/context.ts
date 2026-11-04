// What a thread's actor works with, from the threads it's one of.

import type { Thread, ThreadSummary } from "../protocol/model";
import type { Config, Provider } from "../providers/provider";
import type { Store } from "../store/store";
import type { ThreadActor } from "./actor";

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
  /** The actor of a subagent's new thread, working on `prompt`; `stop` asks it to stop. */
  readonly spawn: (thread: Thread, prompt: string, stop: () => void) => ThreadActor;
}
