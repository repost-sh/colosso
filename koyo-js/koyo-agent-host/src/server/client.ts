// One connection to the host: what it says, cut into lines, and what the host sends it, queued
// while the socket can't take more (`Outbox`). A client that lets more wait than it could ever
// need is cut off: it reconnects and catches up from where it was (docs/chat.md 5.1). A
// client says hello before anything else; one of another version may only shut the host
// down (docs/chat.md C9).

import type { Socket } from "bun";

import { log } from "../log";
import type { Method } from "../protocol/methods";
import { HOST_VERSION, MAX_LINE_BYTES } from "../protocol/wire";
import { LineReader, type Lines } from "./lines";
import { Outbox } from "./outbox";

const encoder = new TextEncoder();

/** How much may wait for a client before it's cut off: a few of the largest lines. */
const MAX_WAITING = 4 * MAX_LINE_BYTES;

type Greeting = "none" | "this_version" | "other_version";

export class Client {
  readonly #socket: Socket<Client>;
  readonly #reader = new LineReader(MAX_LINE_BYTES);
  readonly #outbox = new Outbox();
  #greeting: Greeting = "none";
  /** Ends the connection once what waits has gone. */
  #ending = false;
  #closed = false;
  readonly #onClose: (() => void)[] = [];

  constructor(socket: Socket<Client>) {
    this.#socket = socket;
  }

  receive(chunk: Uint8Array): Lines {
    return this.#reader.push(chunk);
  }

  /** The client said hello with `version`. */
  greet(version: number): void {
    this.#greeting = version === HOST_VERSION ? "this_version" : "other_version";
  }

  /** Why `method` isn't answered for this client now, if it isn't. */
  refusal(method: Method): string | undefined {
    if (method === "hello") {
      return undefined;
    }
    switch (this.#greeting) {
      case "none": {
        return "Say hello first.";
      }
      case "other_version": {
        return method === "shutdown"
          ? undefined
          : "This host is another version; only `shutdown` is answered.";
      }
      case "this_version": {
        return undefined;
      }
    }
  }

  /** Sends a line, after what waits. Nothing goes to a client that's closed or ending. */
  send(line: string): void {
    if (this.#closed || this.#ending) {
      return;
    }
    this.#outbox.push(encoder.encode(line), this.#socket);
    if (this.#outbox.waiting > MAX_WAITING) {
      log.warn("A client stopped reading; it's cut off.");
      this.#socket.terminate();
    }
  }

  /** Writes what waits, as much as the socket takes; it calls again when it takes more. */
  drain(): void {
    this.#outbox.drain(this.#socket);
    if (this.#ending && this.#outbox.waiting === 0) {
      this.#socket.end();
    }
  }

  /** Runs `action` once the client is gone, for what it subscribed to. */
  onClose(action: () => void): void {
    this.#onClose.push(action);
  }

  /** Ends the connection once what waits for it has gone. */
  end(): void {
    this.#ending = true;
    this.drain();
  }

  closed(): void {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    this.#outbox.clear();
    for (const action of this.#onClose.splice(0)) {
      action();
    }
  }
}
