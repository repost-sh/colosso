// What the host tells its clients without being asked (docs/chat.md 4.3.4).

import { z } from "zod";

import { ItemId, ThreadId } from "./ids";
import { Rows } from "./methods";
import { NoticeLevel, Thread, ThreadSummary } from "./model";

export const events = {
  /** A thread as lists show it, to clients that watch threads, when what they show changed. */
  "thread.summary": ThreadSummary,
  /** A thread deleted, to clients that watch threads. */
  "thread.removed": z.object({ threadId: ThreadId }),
  /** The rows one step of a thread's actor changed, whole, to clients that opened it; the
   * thread itself only when it changed. */
  "thread.changed": Rows.extend({
    threadId: ThreadId,
    seq: z.int().nonnegative(),
    thread: Thread.nullable(),
  }),
  /** Text a streaming item grew by, in order after the step that made the item. */
  "item.appended": z.object({ threadId: ThreadId, itemId: ItemId, text: z.string() }),
  "host.notice": z.object({ level: NoticeLevel, message: z.string() }),
} satisfies Record<string, z.ZodType>;

export type EventName = keyof typeof events;
export type EventParams<E extends EventName> = z.infer<(typeof events)[E]>;
