// The host's socket (docs/chat.md 4.3): each client's lines read as requests and answered,
// in the order their handlers finish, with a result or an error; a handler that has its
// answer at once is answered at once. A line too long ends the connection, since nothing
// after it can be told apart.

import { chmodSync } from "node:fs";

import { log } from "../log";
import type { Method } from "../protocol/methods";
import {
  errorLine,
  handle,
  type Handlers,
  HostError,
  MAX_LINE_BYTES,
  readLine,
  resultLine,
} from "../protocol/wire";
import { socketAddress } from "../socket";
import { Client } from "./client";

export interface Server {
  /** How many clients are connected. */
  readonly clients: () => number;
  /** Stops listening, and hangs up on every client once what it's owed has gone. */
  readonly stop: () => void;
}

/** Listens at `path`, for this user only, answering with `handlers`. */
export function serve(path: string, handlers: Handlers<Client>): Server {
  const clients = new Set<Client>();
  const listener = Bun.listen<Client>({
    unix: socketAddress(path),
    socket: {
      open(socket) {
        socket.data = new Client(socket);
        clients.add(socket.data);
      },
      data(socket, chunk) {
        const client = socket.data;
        const read = client.receive(chunk);
        if (read.kind === "too_long") {
          const message = `A line is over ${String(MAX_LINE_BYTES)} bytes.`;
          client.send(errorLine(null, { code: "bad_request", message }));
          client.end();
          return;
        }
        for (const line of read.lines) {
          answer(handlers, client, line);
        }
      },
      drain(socket) {
        socket.data.drain();
      },
      close(socket) {
        clients.delete(socket.data);
        socket.data.closed();
      },
      error(socket, error) {
        log.warn("A client's connection failed", error);
        clients.delete(socket.data);
        socket.data.closed();
      },
    },
  });
  chmodSync(path, 0o600);
  return {
    clients: () => clients.size,
    stop: () => {
      listener.stop(false);
      for (const client of clients) {
        client.end();
      }
    },
  };
}

function answer(handlers: Handlers<Client>, client: Client, line: string): void {
  const request = readLine(line);
  if (request.kind === "invalid") {
    client.send(errorLine(request.id, request.error));
    return;
  }
  const { id, method } = request;
  const refusal = client.refusal(method);
  if (refusal !== undefined) {
    client.send(errorLine(id, { code: "not_allowed", message: refusal }));
    return;
  }
  const refuse = (error: unknown): void => {
    client.send(errorLine(id, failure(method, error)));
  };
  const reply = (result: unknown): void => {
    client.send(resultLine(id, result));
  };
  try {
    const result = handle(handlers, method, request.params, client);
    if (result instanceof Promise) {
      result.then(reply, refuse);
    } else {
      reply(result);
    }
  } catch (error) {
    refuse(error);
  }
}

function failure(method: Method, error: unknown): { code: HostError["code"]; message: string } {
  if (error instanceof HostError) {
    return { code: error.code, message: error.message };
  }
  log.error(`${method} failed`, error);
  return { code: "internal", message: "The host failed; its log says why." };
}
