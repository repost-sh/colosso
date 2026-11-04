// The requests a client makes of the host, each with the shape of its params and of its
// result (docs/chat.md 4.3). The map is the contract: the server's handlers are typed from
// it, and the protocol tests hold a fixture for every method.

import { z } from "zod";

import { RequestId, RunId, ThreadId } from "./ids";
import {
  Attachment,
  Effort,
  Item,
  Mode,
  Prompt,
  ProviderId,
  Request,
  Response,
  Run,
  Thread,
  ThreadSummary,
} from "./model";

const Empty = z.object({});
/** The host's version (C9): a client meeting another restarts the host, or asks to. */
const Version = z.int().positive();
const ThreadParams = z.object({ threadId: ThreadId });
const QueuedRunParams = z.object({ threadId: ThreadId, runId: RunId });

export const ProviderStatus = z.object({
  provider: ProviderId,
  state: z.enum(["ready", "missing", "signed_out", "error"]),
  executable: z.string().nullable(),
  version: z.string().nullable(),
  account: z.object({ email: z.string().nullable(), plan: z.string().nullable() }).nullable(),
  message: z.string().nullable(),
});
export type ProviderStatus = z.infer<typeof ProviderStatus>;

export const Model = z.object({
  id: z.string().min(1),
  name: z.string(),
  label: z.string(),
  runs: z.string().nullable().default(null),
  efforts: z.array(Effort),
  fast: z.boolean(),
  auto: z.boolean(),
});
export type Model = z.infer<typeof Model>;

export const Command = z.object({
  name: z.string().min(1),
  description: z.string(),
  argumentHint: z.string().nullable(),
});
export type Command = z.infer<typeof Command>;

export const Catalog = z.object({
  models: z.array(Model),
  commands: z.array(Command),
  /** The mode a chat that names none runs in, from the provider's settings. */
  defaultMode: Mode.nullable(),
});
export type Catalog = z.infer<typeof Catalog>;

/** Runs with their timeline entries and requests. */
export const Rows = z.object({
  runs: z.array(Run),
  items: z.array(Item),
  requests: z.array(Request),
});
export type Rows = z.infer<typeof Rows>;

/** A thread with its latest runs' rows, or with the rows changed after `sinceSeq`. */
export const ThreadSnapshot = Rows.extend({ thread: Thread });
export type ThreadSnapshot = z.infer<typeof ThreadSnapshot>;

/** `auto` starts a turn, or steers the running one; `queue` waits for it to end. */
export const Delivery = z.enum(["auto", "queue"]);
export type Delivery = z.infer<typeof Delivery>;

export const Delivered = z.enum(["started", "queued", "steered"]);
export type Delivered = z.infer<typeof Delivered>;

export const methods = {
  "hello": {
    params: z.object({
      client: z.object({ kind: z.enum(["app", "cli", "test"]) }),
      version: Version,
    }),
    /**
     * `busy` while a run is live, a request waits or another client is here (another build's
     * app uses it), so another build waits to replace it.
     */
    result: z.object({ version: Version, pid: z.int().positive(), busy: z.boolean() }),
  },
  "shutdown": { params: Empty, result: Empty },
  "config.set": {
    params: z.object({
      claudePath: z.string().min(1).nullable(),
      defaultMode: Mode.nullable(),
    }),
    result: Empty,
  },
  "providers.status": {
    params: z.object({ refresh: z.boolean() }),
    result: z.array(ProviderStatus),
  },
  "catalog": {
    /** `provider` null is the host's first, which the app takes until a person can choose. */
    params: z.object({ provider: ProviderId.nullable(), cwd: z.string().min(1) }),
    result: Catalog,
  },
  "threads.watch": { params: Empty, result: z.array(ThreadSummary) },
  "threads.create": {
    params: z.object({
      cwd: z.string().min(1),
      provider: ProviderId.nullable(),
      model: z.string().nullable(),
      effort: Effort.nullable(),
      fast: z.boolean(),
      permission: Mode.nullable(),
      plan: z.boolean(),
    }),
    result: Thread,
  },
  "threads.open": {
    params: z.object({ threadId: ThreadId, sinceSeq: z.int().nonnegative().nullable() }),
    result: ThreadSnapshot,
  },
  "threads.history": {
    params: z.object({ threadId: ThreadId, before: RunId.nullable(), runs: z.int().positive() }),
    result: Rows,
  },
  "threads.close": { params: ThreadParams, result: Empty },
  "threads.update": {
    params: z.object({
      threadId: ThreadId,
      title: z.string().min(1).optional(),
      model: z.string().nullable().optional(),
      effort: Effort.nullable().optional(),
      fast: z.boolean().optional(),
      permission: Mode.nullable().optional(),
      plan: z.boolean().optional(),
      tabOpen: z.boolean().optional(),
    }),
    result: Thread,
  },
  "threads.read": { params: ThreadParams, result: Empty },
  "threads.delete": { params: ThreadParams, result: Empty },
  /** Goes back to before run `runId`, and says what the person said then (C11). */
  "threads.rewind": {
    params: z.object({ threadId: ThreadId, runId: RunId }),
    result: z.object({ prompt: Prompt }),
  },
  "turns.send": {
    params: z.object({ threadId: ThreadId, prompt: Prompt, delivery: Delivery }),
    result: z.object({ runId: RunId, delivered: Delivered }),
  },
  "turns.stop": {
    params: z.object({ threadId: ThreadId, force: z.boolean() }),
    result: Empty,
  },
  "turns.compact": { params: ThreadParams, result: z.object({ runId: RunId }) },
  "queue.remove": { params: QueuedRunParams, result: Empty },
  "queue.edit": {
    params: QueuedRunParams.extend({ text: z.string(), attachments: z.array(Attachment) }),
    result: Empty,
  },
  "queue.sendNow": { params: QueuedRunParams, result: z.object({ delivered: Delivered }) },
  "queue.resume": { params: ThreadParams, result: Empty },
  "requests.respond": {
    params: z.object({ threadId: ThreadId, requestId: RequestId, response: Response }),
    result: Empty,
  },
} satisfies Record<string, { params: z.ZodType; result: z.ZodType }>;

export type Method = keyof typeof methods;
export type Params<M extends Method> = z.infer<(typeof methods)[M]["params"]>;
export type Result<M extends Method> = z.infer<(typeof methods)[M]["result"]>;
