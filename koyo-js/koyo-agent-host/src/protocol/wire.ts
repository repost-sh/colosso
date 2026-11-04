// The protocol's framing: one JSON object per line (docs/chat.md 4.3). A line from a client
// is parsed into a request for a method the host has, with params of that method's shape;
// what goes back is a result, an error, or an event.

import { z } from "zod";

import type { EventName, EventParams } from "./events";
import { type Method, methods, type Params, type Result } from "./methods";

/**
 * The host's version (docs/chat.md C9), which `script/daemon-version` keeps beside a
 * fingerprint of the host's sources: a change a running host must restart for raises it.
 */
export const HOST_VERSION = 8;

/** The longest line either side accepts. */
export const MAX_LINE_BYTES = 8 * 1024 * 1024;

export const ErrorCode = z.enum([
  "bad_request",
  "unknown_method",
  /** A method before `hello`, or one a client of another version may not call. */
  "not_allowed",
  "thread_not_found",
  "run_not_found",
  "request_not_found",
  "conflict",
  "provider_unavailable",
  "internal",
]);
export type ErrorCode = z.infer<typeof ErrorCode>;

export const WireError = z.object({ code: ErrorCode, message: z.string() });
export type WireError = z.infer<typeof WireError>;

/** A request the host refuses, with the code its client is told. */
export class HostError extends Error {
  override readonly name = "HostError";
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.code = code;
  }
}

const Envelope = z.object({
  id: z.int().nonnegative(),
  method: z.string(),
  params: z.unknown(),
});

interface RequestLine {
  readonly kind: "request";
  readonly id: number;
  readonly method: Method;
  readonly params: unknown;
}

interface InvalidLine {
  readonly kind: "invalid";
  readonly id: number | null;
  readonly error: WireError;
}

/** A line read: the request it holds, or why it holds none. */
export type Inbound = RequestLine | InvalidLine;

/** Whether the host has a method of this name. */
export function isMethod(name: string): name is Method {
  return Object.hasOwn(methods, name);
}

/** Reads one line from a client; its params are checked when it's handled (`handle`). */
export function readLine(line: string): Inbound {
  let json: unknown;
  try {
    json = JSON.parse(line);
  } catch {
    return invalid(null, "bad_request", "The line isn't JSON.");
  }
  const envelope = Envelope.safeParse(json);
  if (!envelope.success) {
    return invalid(null, "bad_request", "A request needs an `id`, a `method` and `params`.");
  }
  const { id, method, params } = envelope.data;
  return isMethod(method)
    ? { kind: "request", id, method, params }
    : invalid(id, "unknown_method", `There is no method \`${method}\`.`);
}

function invalid(id: number | null, code: ErrorCode, message: string): Inbound {
  return { kind: "invalid", id, error: { code, message } };
}

/**
 * What the host does for each method, for the client `C` that asked: the answer at once, or
 * the promise of one. An answer had at once is sent at once, before anything a later step
 * tells the client, so the snapshot `threads.open` takes reaches it ahead of what follows.
 */
export type Handlers<C> = {
  readonly [M in Method]: (params: Params<M>, client: C) => Result<M> | Promise<Result<M>>;
};

type Specs = { readonly [M in Method]: { readonly params: z.ZodType<Params<M>> } };
const specs: Specs = methods;

/** Checks `params` against `method`'s shape and runs its handler. */
export function handle<M extends Method, C>(
  handlers: Handlers<C>,
  method: M,
  params: unknown,
  client: C,
): Result<M> | Promise<Result<M>> {
  const parsed = specs[method].params.safeParse(params);
  if (!parsed.success) {
    throw new HostError("bad_request", `Bad params for \`${method}\`: ${parsed.error.message}`);
  }
  const handler: Handlers<C>[M] = handlers[method];
  return handler(parsed.data, client);
}

export function resultLine(id: number, result: unknown): string {
  return `${JSON.stringify({ id, result })}\n`;
}

export function errorLine(id: number | null, error: WireError): string {
  return `${JSON.stringify({ id, error })}\n`;
}

export function eventLine<E extends EventName>(event: E, params: EventParams<E>): string {
  return `${JSON.stringify({ event, params })}\n`;
}
