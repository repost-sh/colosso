// A client of the host's socket as tests drive it: raw lines out, as much as the socket takes
// at a time, and each line in parsed, in order.

import type { Socket } from "bun";
import { z } from "zod";

import { Thread } from "../../src/protocol/model";
import { HOST_VERSION } from "../../src/protocol/wire";
import { Queue } from "../../src/queue";
import { LineReader } from "../../src/server/lines";
import { Outbox } from "../../src/server/outbox";

const encoder = new TextEncoder();

/** What a test waits for: the next line, or the end of the connection. */
export type Received = Line | { readonly kind: "closed" };

interface Line {
  readonly kind: "line";
  readonly line: unknown;
}

const Created = z.object({ result: Thread });

/** The thread `threads.create` answered with. */
export function createdThread(received: Received): Thread {
  return Created.parse(received.kind === "line" ? received.line : undefined).result;
}

export class LineClient {
  readonly #socket: Socket;
  readonly #lines: Queue<Line>;
  readonly #outbox: Outbox;

  private constructor(socket: Socket, lines: Queue<Line>, outbox: Outbox) {
    this.#socket = socket;
    this.#lines = lines;
    this.#outbox = outbox;
  }

  static async connect(path: string): Promise<LineClient> {
    const reader = new LineReader(64 * 1024 * 1024);
    const lines = new Queue<Line>();
    const outbox = new Outbox();
    const socket = await Bun.connect({
      unix: path,
      socket: {
        data(_socket, chunk) {
          const read = reader.push(chunk);
          if (read.kind === "lines") {
            for (const line of read.lines) {
              const parsed: unknown = JSON.parse(line);
              lines.push({ kind: "line", line: parsed });
            }
          }
        },
        drain(connection) {
          outbox.drain(connection);
        },
        close() {
          lines.end();
        },
      },
    });
    return new LineClient(socket, lines, outbox);
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
    const next = await this.#lines.next();
    return next.done === true ? { kind: "closed" } : next.value;
  }

  close(): void {
    this.#socket.end();
  }
}
