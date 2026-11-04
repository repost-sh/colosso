import { join } from "node:path";

import { Database } from "bun:sqlite";
import { afterEach, describe, expect, test } from "bun:test";

import { MIGRATIONS } from "../src/store/migrations";
import { Store } from "../src/store/store";
import { userMessage } from "../src/threads/records";
import { request, run, text, thread } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";

const open: Store[] = [];
afterEach(() => {
  for (const opened of open.splice(0)) {
    opened.close();
  }
  removeScratch();
});

function store(path = ":memory:"): Store {
  const opened = Store.open(path);
  open.push(opened);
  return opened;
}

describe("commit", () => {
  test("stamps every row it writes with the thread's next change number", () => {
    const chats = store();
    const chat = thread();
    const first = run(chat);
    const done = chats.commit(chat.id, { thread: chat, runs: [first] });
    expect(done.seq).toBe(1);
    expect(done.thread?.seq).toBe(1);
    expect(done.runs.map((row) => row.seq)).toStrictEqual([1]);
    const second = chats.commit(chat.id, {
      items: [text(first, 0, "Found it.")],
      requests: [request(first)],
    });
    expect(second.seq).toBe(2);
    expect(chats.head(chat.id)).toBe(2);
    // The thread wasn't part of the step, so it keeps the number it last changed at.
    expect(chats.thread(chat.id)?.seq).toBe(1);
    expect(chats.latest(chat.id, 10).items.map((row) => row.seq)).toStrictEqual([2]);
  });

  test("writes all of a step or none of it", () => {
    const chats = store();
    const stray = thread();
    expect(() => chats.commit(stray.id, { runs: [run(stray)] })).toThrow();
    expect(chats.head(stray.id)).toBe(0);
    expect(chats.latest(stray.id, 10).runs).toStrictEqual([]);
  });
});

describe("reading a thread", () => {
  /** A thread of `count` runs, each with two items, the last with a request. */
  function filled(chats: Store, count: number): ReturnType<typeof thread> {
    const chat = thread();
    chats.commit(chat.id, { thread: chat });
    for (let index = 0; index < count; index += 1) {
      const turn = run(chat);
      chats.commit(chat.id, {
        runs: [turn],
        items: [text(turn, index * 2 + 1, "after"), text(turn, index * 2, "before")],
        requests: index === count - 1 ? [request(turn)] : [],
      });
    }
    return chat;
  }

  test("gives the latest runs, oldest first, with their rows in timeline order", () => {
    const chats = store();
    const chat = filled(chats, 5);
    const page = chats.latest(chat.id, 2);
    expect(page.runs).toHaveLength(2);
    expect(page.runs.map((row) => row.id)).toStrictEqual(page.runs.map((row) => row.id).toSorted());
    expect(page.items.map((row) => row.ordinal)).toStrictEqual([6, 7, 8, 9]);
    expect(page.requests).toHaveLength(1);
  });

  test("pages back through history by run", () => {
    const chats = store();
    const chat = filled(chats, 5);
    const latest = chats.latest(chat.id, 2);
    const oldestShown = latest.runs.at(0);
    if (oldestShown === undefined) {
      throw new Error("The thread has runs.");
    }
    const earlier = chats.before(chat.id, oldestShown.id, 2);
    expect(earlier.items.map((row) => row.ordinal)).toStrictEqual([2, 3, 4, 5]);
    expect(earlier.requests).toStrictEqual([]);
    const first = earlier.runs.at(0);
    if (first === undefined) {
      throw new Error("The page has runs.");
    }
    expect(chats.before(chat.id, first.id, 2).items.map((row) => row.ordinal))
      .toStrictEqual([0, 1]);
  });

  test("gives what changed after a change number", () => {
    const chats = store();
    const chat = filled(chats, 3);
    const head = chats.head(chat.id);
    const late = run(chat, "queued");
    chats.commit(chat.id, { runs: [late] });
    expect(chats.since(chat.id, head)).toStrictEqual({
      runs: [{ ...late, seq: head + 1 }],
      items: [],
      requests: [],
    });
  });

  test("keeps streaming text without a change number", () => {
    const chats = store();
    const chat = thread();
    const turn = run(chat, "running");
    const answer = text(turn, 0);
    chats.commit(chat.id, { thread: chat, runs: [turn], items: [answer] });
    const head = chats.head(chat.id);
    chats.keepText({ ...answer, seq: head, payload: { text: "The test shares a folder" } });
    expect(chats.head(chat.id)).toBe(head);
    expect(chats.since(chat.id, head).items).toStrictEqual([]);
    expect(chats.latest(chat.id, 1).items.at(0)?.payload).toStrictEqual({
      text: "The test shares a folder",
    });
  });

  test("forgets a deleted thread and all of its rows", () => {
    const chats = store();
    const chat = filled(chats, 2);
    chats.delete(chat.id);
    expect(chats.thread(chat.id)).toBeUndefined();
    expect(chats.latest(chat.id, 10)).toStrictEqual({ runs: [], items: [], requests: [] });
  });

  test("names the files its messages attach, one that joined a run's too", () => {
    const chats = store();
    const chat = thread();
    const started = run(chat);
    const a = { file: "a.png", name: "a.png", mime: "image/png" };
    const b = { file: "b.png", name: "b.png", mime: "image/png" };
    const first = { ...started, prompt: { ...started.prompt, attachments: [a] } };
    const joined = { ...started.prompt, attachments: [b] };
    chats.commit(chat.id, {
      thread: chat,
      runs: [first],
      items: [userMessage(first, 1, joined, "steer")],
    });
    expect(chats.attachments(chat.id).toSorted()).toStrictEqual(["a.png", "b.png"]);
    expect(chats.allAttachments()).toStrictEqual(new Set(["a.png", "b.png"]));
  });

  test("lists threads, the most recently changed first", () => {
    const chats = store();
    const older = { ...thread(), updatedAt: "2026-11-01T00:00:00.000Z" };
    const newer = { ...thread(), updatedAt: "2026-11-02T00:00:00.000Z" };
    chats.commit(older.id, { thread: older });
    chats.commit(newer.id, { thread: newer });
    expect(chats.threads().map((row) => row.id)).toStrictEqual([newer.id, older.id]);
  });
});

describe("open", () => {
  test("keeps what was written across a reopening", () => {
    const dir = scratch();
    const path = join(dir, "chats.sqlite");
    const chat = thread();
    const first = Store.open(path);
    first.commit(chat.id, { thread: chat });
    first.close();
    expect(store(path).thread(chat.id)).toStrictEqual({ ...chat, seq: 1 });
  });

  test("refuses chats a newer build wrote", () => {
    const dir = scratch();
    const path = join(dir, "chats.sqlite");
    const newer = new Database(path, { create: true });
    newer.run(`PRAGMA user_version = ${String(MIGRATIONS.length + 1)}`);
    newer.close();
    expect(() => Store.open(path)).toThrow(/newer build/u);
  });
});
