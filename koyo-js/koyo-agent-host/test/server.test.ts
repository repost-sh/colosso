import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { eventLine, HOST_VERSION, HostError, MAX_LINE_BYTES } from "../src/protocol/wire";
import { serve, type Server } from "../src/server/server";
import { handlers } from "./support/handlers";
import { LineClient } from "./support/line-client";
import { permissions, removeScratch, scratch } from "./support/scratch";

const THREAD = "0193a2f0-6c1e-7b3a-9f21-4d5e6f708192";

const greeted = handlers({
  "hello": ({ version }, client) => {
    client.greet(version);
    return { version: HOST_VERSION, pid: process.pid, busy: false };
  },
  "shutdown": () => ({}),
  "threads.read": () => ({}),
  // Answered at once, with what a later step tells the client, as an actor's next commit.
  "queue.resume": ({ threadId }, client) => {
    queueMicrotask(() => {
      client.send(eventLine("thread.removed", { threadId }));
    });
    return {};
  },
  "threads.close": () => {
    throw new HostError("thread_not_found", "There is no such thread.");
  },
  "threads.delete": async () => await Promise.reject(new Error("the disk is gone")),
  // A result far larger than a socket takes at once.
  "catalog": () => ({
    models: [],
    commands: Array.from({ length: 6 }, (_, index) => ({
      name: `command-${String(index)}`,
      description: "x".repeat(1024 * 1024),
      argumentHint: null,
    })),
    defaultMode: null,
    defaultModel: null,
  }),
});

const running: { server: Server; clients: LineClient[] }[] = [];
afterEach(() => {
  for (const { server, clients } of running.splice(0)) {
    for (const client of clients) {
      client.close();
    }
    server.stop();
  }
  removeScratch();
});

async function connected(): Promise<{ client: LineClient; socket: string }> {
  const dir = scratch();
  const socket = join(dir, "host.sock");
  const server = serve(socket, greeted);
  const client = await LineClient.connect(socket);
  running.push({ server, clients: [client] });
  return { client, socket };
}

async function hello(client: LineClient, version = HOST_VERSION): Promise<void> {
  expect(await client.hello(version)).toStrictEqual({
    kind: "line",
    line: { id: 0, result: { version: HOST_VERSION, pid: process.pid, busy: false } },
  });
}

/** That the next line refuses request `id` with `code`, whatever it says why. */
async function refused(client: LineClient, id: number | null, code: string): Promise<void> {
  expect(await client.next()).toMatchObject({ kind: "line", line: { id, error: { code } } });
}

describe("serve", () => {
  test("listens for this user only", async () => {
    const { socket } = await connected();
    expect(permissions(socket)).toBe("600");
  });

  test("answers nothing but hello before a hello", async () => {
    const { client } = await connected();
    client.request(1, "threads.read", { threadId: THREAD });
    await refused(client, 1, "not_allowed");
    await hello(client);
    client.request(2, "threads.read", { threadId: THREAD });
    expect(await client.next()).toStrictEqual({ kind: "line", line: { id: 2, result: {} } });
  });

  test("lets a client of another version only shut the host down", async () => {
    const { client } = await connected();
    await hello(client, HOST_VERSION + 1);
    client.request(1, "threads.read", { threadId: THREAD });
    await refused(client, 1, "not_allowed");
    client.request(2, "shutdown", {});
    expect(await client.next()).toStrictEqual({ kind: "line", line: { id: 2, result: {} } });
  });

  test("says why a line isn't a request it can answer", async () => {
    const { client } = await connected();
    await hello(client);
    client.write("not json\n");
    await refused(client, null, "bad_request");
    client.request(1, "threads.nope", {});
    await refused(client, 1, "unknown_method");
    client.request(2, "threads.read", { threadId: "not an id" });
    await refused(client, 2, "bad_request");
  });

  test("keeps a refusal's code and calls any other failure internal", async () => {
    const { client } = await connected();
    await hello(client);
    client.request(1, "threads.close", { threadId: THREAD });
    await refused(client, 1, "thread_not_found");
    client.request(2, "threads.delete", { threadId: THREAD });
    await refused(client, 2, "internal");
  });

  test("sends an answer had at once before what a later step sends", async () => {
    const { client } = await connected();
    await hello(client);
    client.request(1, "queue.resume", { threadId: THREAD });
    expect(await client.next()).toMatchObject({ kind: "line", line: { id: 1 } });
    expect(await client.next()).toMatchObject({ kind: "line", line: { event: "thread.removed" } });
  });

  test("sends a result larger than the socket takes whole", async () => {
    const { client } = await connected();
    await hello(client);
    client.request(1, "catalog", { provider: "claude", cwd: "/tmp" });
    const received = await client.next();
    expect(received.kind).toBe("line");
    expect(JSON.stringify(received).length).toBeGreaterThan(6 * 1024 * 1024);
  });

  test("hangs up on a line over the bound, after saying so", async () => {
    const { client } = await connected();
    await hello(client);
    client.write("x".repeat(MAX_LINE_BYTES + 1));
    await refused(client, null, "bad_request");
    expect(await client.next()).toStrictEqual({ kind: "closed" });
  });
});
