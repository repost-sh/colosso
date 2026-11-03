// The ids of the host's records. Each kind is its own branded type, so a run's id can't be
// passed where a thread's is wanted; a new one is a UUIDv7, which sorts by creation time.

import { randomUUIDv7 } from "bun";
import { z } from "zod";

export const ThreadId = z.uuid().brand<"ThreadId">();
export type ThreadId = z.infer<typeof ThreadId>;

export const RunId = z.uuid().brand<"RunId">();
export type RunId = z.infer<typeof RunId>;

export const ItemId = z.uuid().brand<"ItemId">();
export type ItemId = z.infer<typeof ItemId>;

export const RequestId = z.uuid().brand<"RequestId">();
export type RequestId = z.infer<typeof RequestId>;

/** The id a client gives a message it sends, so a repeated send is recognised. */
export const MessageId = z.uuid().brand<"MessageId">();
export type MessageId = z.infer<typeof MessageId>;

/** A new id of the kind `schema` describes. */
export function newId<Id extends z.ZodType>(schema: Id): z.output<Id> {
  return schema.parse(randomUUIDv7());
}
