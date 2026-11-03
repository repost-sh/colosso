// A client of the host's socket as tests drive it: raw lines out, as much as the socket takes
// at a time, and each line in parsed, in order.

import type { Socket } from "bun";

import { HOST_VERSION } from "../../src/protocol/wire";
import { LineReader } from "../../src/server/lines";
import { Outbox } from "../../src/server/outbox";

const encoder = new TextEncoder();

/** What a test waits for: the next line, or the end of the connection. */
export type Received = { readonly kind: "line"; readonly line: unknown }
  | { readonly kind: "closed" };

class Mailbox {
  readonly #received: Received[] = [];
  readonly #waiting: ((received: Received) => void)[] = [];

  arrive(received: Received): void {
    const waiter = this.#waiting.shift();
    if (waiter === undefined) {
      this.#received.push(received);
    } else {
      waiter(received);
    }
  }

  async next(): Promise<Received> {
    const received = this.#received.shift();
    if (received !== undefined) {
      return received;
    }
    const { promise, resolve } = Promise.withResolvers<Received>();
    this.#waiting.push(resolve);
    return await promise;
  }
}

export class LineClient {
  readonly #socket: Socket;
  readonly #mailbox: Mailbox;
  readonly #outbox: Outbox;

  private constructor(socket: Socket, mailbox: Mailbox, outbox: Outbox) {
    this.#socket = socket;
    this.#mailbox = mailbox;
    this.#outbox = outbox;
  }

  static async connect(path: string): Promise<LineClient> {
    const reader = new LineReader(64 * 1024 * 1024);
    const mailbox = new Mailbox();
    const outbox = new Outbox();
    const socket = await Bun.connect({
      unix: path,
      socket: {
        data(_socket, chunk) {
          const read = reader.push(chunk);
          if (read.kind === "lines") {
            for (const line of read.lines) {
              const parsed: unknown = JSON.parse(line);
              mailbox.arrive({ kind: "line", line: parsed });
            }
          }
        },
        drain(connection) {
          outbox.drain(connection);
        },
        close() {
          mailbox.arrive({ kind: "closed" });
        },
      },
    });
    return new LineClient(socket, mailbox, outbox);
  }

  /** Sends `text` as it is: a test adds the newline. */
  write(text: string): void {
    this.#outbox.push(encoder.encode(text), this.#socket);
  }

  request(id: number, method: string, params: unknown): void {
    this.write(`${JSON.stringify({ id, method, params })}\n`);
  }

  /** Says hello as a test, as request 0, and gives the reply. */
  async hello(version = HOST_VERSION): Promise<Received> {
    this.request(0, "hello", { client: { kind: "test" }, version });
    return await this.next();
  }

  async next(): Promise<Received> {
    return await this.#mailbox.next();
  }

  close(): void {
    this.#socket.end();
  }
}
