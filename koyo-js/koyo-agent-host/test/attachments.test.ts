import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { Store } from "../src/store/store";
import { draftAttachments, sweepAttachments } from "../src/threads/attachments";
import { Threads } from "../src/threads/threads";
import { run, thread } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";

const OVER_A_WEEK_MS = 8 * 24 * 60 * 60 * 1000;

afterEach(removeScratch);

/** A folder holding `files`, and the same again as whether each is still there. */
function folderOf(...files: string[]): { folder: string; left: () => boolean[] } {
  const folder = scratch();
  for (const file of files) {
    writeFileSync(join(folder, file), "");
  }
  return { folder, left: () => files.map((file) => existsSync(join(folder, file))) };
}

/** The app's drafts, as `koyo/src/chat/drafts.rs` writes them, with one for each of `files`. */
function draftsNaming(...files: string[]): string {
  return JSON.stringify(files.map((file) => [
    { thread: file },
    { text: "", cursor: 0, attachments: [{ file, name: file, mime: "image/png" }] },
  ]));
}

describe("sweeping the attachments", () => {
  test("leaves what no message names yet, as it may be on its way into one", () => {
    const { folder, left } = folderOf("unnamed.png");
    sweepAttachments(folder, new Set(), Date.now());
    expect(left()).toStrictEqual([true]);
  });

  test("removes what no message names a week on, and nothing that one does", () => {
    const { folder, left } = folderOf("named.png", "unnamed.png");
    sweepAttachments(folder, new Set(["named.png"]), Date.now() + OVER_A_WEEK_MS);
    expect(left()).toStrictEqual([true, false]);
  });

  test("has nothing to do without a folder", () => {
    const folder = join(scratch(), "not-yet");
    expect(() => {
      sweepAttachments(folder, new Set(), Date.now());
    }).not.toThrow();
  });

  test("happens as a host starts, going by what the store and the drafts name", () => {
    const { folder, left } = folderOf("named.png", "drafted.png", "unnamed.png");
    const drafts = join(scratch(), "chat-drafts.json");
    writeFileSync(drafts, draftsNaming("drafted.png"));
    const store = Store.open(":memory:");
    const chat = thread();
    const started = run(chat);
    const attachments = [{ file: "named.png", name: "named.png", mime: "image/png" }];
    const settled = { ...started, prompt: { ...started.prompt, attachments } };
    store.commit(chat.id, { thread: chat, runs: [settled] });
    const later = new Date(Date.now() + OVER_A_WEEK_MS).toISOString();

    new Threads({ store, providers: [], attachments: folder, drafts, now: () => later }).recover();

    expect(left()).toStrictEqual([true, true, false]);
    store.close();
  });

  test("leaves everything if it can't tell what the drafts name", () => {
    const { folder, left } = folderOf("unnamed.png");
    const drafts = join(scratch(), "chat-drafts.json");
    writeFileSync(drafts, "{ not json");
    const store = Store.open(":memory:");
    const later = new Date(Date.now() + OVER_A_WEEK_MS).toISOString();

    new Threads({ store, providers: [], attachments: folder, drafts, now: () => later }).recover();

    expect(left()).toStrictEqual([true]);
    store.close();
  });
});

describe("what the drafts name", () => {
  test("is nothing before the app has written any", () => {
    expect(draftAttachments(join(scratch(), "not-yet.json"))).toStrictEqual(new Set());
  });

  test("is every file of every draft", () => {
    const drafts = join(scratch(), "chat-drafts.json");
    writeFileSync(drafts, draftsNaming("one.png", "two.txt"));
    expect(draftAttachments(drafts)).toStrictEqual(new Set(["one.png", "two.txt"]));
  });
});
