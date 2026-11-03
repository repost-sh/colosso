// The host's methods over its socket, as a client sees them: what each answers, and the events
// a thread's steps send, in order.

import { mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { folder } from "../src/daemon/folder";
import { handlers } from "../src/host";
import { FakeProvider } from "../src/providers/fake";
import { serve, type Server } from "../src/server/server";
import { Store } from "../src/store/store";
import { Threads } from "../src/threads/threads";
import { prompt, settle } from "./support/actor";
import { ignore } from "./support/handlers";
import { LineClient, type Received } from "./support/line-client";
import { CREATE } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";

interface Running {
  readonly server: Server;
  readonly store: Store;
  readonly threads: Threads;
  readonly provider: FakeProvider;
  readonly dir: string;
  readonly socket: string;
  readonly attachments: string;
  readonly clients: LineClient[];
}

const running: Running[] = [];
afterEach(() => {
  for (const started of running.splice(0)) {
    for (const connected of started.clients) {
      connected.close();
    }
    started.server.stop();
    started.store.close();
  }
  removeScratch();
});

function host(): Running {
  const dir = scratch();
  const { socket, attachments, database } = folder(dir);
  mkdirSync(attachments);
  const store = Store.open(database);
  const provider = new FakeProvider();
  const threads = new Threads({
    store,
    providers: [provider],
    attachments,
    now: () => new Date().toISOString(),
  });
  const server = serve(socket, handlers({
    threads,
    providers: [provider],
    attachments,
    configure: ignore,
    shutdown: ignore,
  }));
  const started = { server, store, threads, provider, dir, socket, attachments, clients: [] };
  running.push(started);
  return started;
}

/** A client that said hello to `to`. */
async function client(to: Running): Promise<LineClient> {
  const connected = await LineClient.connect(to.socket);
  to.clients.push(connected);
  expect(await connected.hello()).toMatchObject({ line: { id: 0, result: { busy: false } } });
  return connected;
}

/** What the next line is: `result`, `error` or the event's name. */
function kind(received: Received): string {
  if (received.kind === "closed") {
    return "closed";
  }
  const { line } = received;
  if (typeof line !== "object" || line === null) {
    return "?";
  }
  if ("event" in line && typeof line.event === "string") {
    return line.event;
  }
  return "result" in line ? "result" : "error";
}

async function kinds(from: LineClient, count: number): Promise<string[]> {
  const seen: string[] = [];
  for (let index = 0; index < count; index += 1) {
    seen.push(kind(await from.next()));
  }
  return seen;
}

describe("the host", () => {
  test("tells a thread's changes in step order, each before the answer that made it", async () => {
    const chats = host();
    const app = await client(chats);
    app.request(1, "threads.watch", {});
    expect(await app.next()).toMatchObject({ line: { id: 1, result: [] } });
    app.request(2, "threads.create", CREATE);
    expect(await kinds(app, 2)).toStrictEqual(["thread.summary", "result"]);
    const [thread] = chats.store.threads();
    if (thread === undefined) {
      throw new Error("The thread is stored.");
    }
    app.request(3, "threads.open", { threadId: thread.id, sinceSeq: null });
    expect(await app.next()).toMatchObject({ line: { id: 3, result: { runs: [] } } });
    app.request(4, "turns.send", {
      threadId: thread.id,
      prompt: prompt("Find the flaky test."),
      delivery: "auto",
    });
    expect(await kinds(app, 3)).toStrictEqual(["thread.changed", "thread.summary", "result"]);
    chats.provider.session.started();
    chats.provider.session.startsText("m1:0");
    chats.provider.session.report({ type: "item.appended", key: "m1:0", text: "Found it." });
    expect(await kinds(app, 3))
      .toStrictEqual(["thread.changed", "thread.changed", "item.appended"]);
  });

  test("refuses an attachment outside its folder", async () => {
    const chats = host();
    const app = await client(chats);
    const thread = chats.threads.create(CREATE);
    writeFileSync(join(chats.dir, "secret"), "");
    symlinkSync(join(chats.dir, "secret"), join(chats.attachments, "link.png"));
    const outside: [number, string][] = [[1, "../secret"], [2, "link.png"], [3, "missing.png"]];
    for (const [id, file] of outside) {
      app.request(id, "turns.send", {
        threadId: thread.id,
        prompt: { ...prompt("Look."), attachments: [{ file, name: file, mime: "image/png" }] },
        delivery: "auto",
      });
      expect(await app.next()).toMatchObject({ line: { id, error: { code: "bad_request" } } });
    }
  });

  test("forgets a client that went", async () => {
    const chats = host();
    const thread = chats.threads.create(CREATE);
    const gone = await client(chats);
    gone.request(1, "threads.open", { threadId: thread.id, sinceSeq: null });
    await gone.next();
    gone.close();
    await settle();
    const app = await client(chats);
    app.request(1, "turns.send", {
      threadId: thread.id,
      prompt: prompt("Find the flaky test."),
      delivery: "auto",
    });
    expect(await kinds(app, 1)).toStrictEqual(["result"]);
  });

  test("says a thread it doesn't have isn't there", async () => {
    const chats = host();
    const app = await client(chats);
    app.request(1, "threads.open", {
      threadId: "0193a2f0-6c1e-7b3a-9f21-4d5e6f708192",
      sinceSeq: null,
    });
    const refused = { line: { id: 1, error: { code: "thread_not_found" } } };
    expect(await app.next()).toMatchObject(refused);
  });
});
