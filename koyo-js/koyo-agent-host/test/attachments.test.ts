import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { Store } from "../src/store/store";
import { sweepAttachments } from "../src/threads/attachments";
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

  test("happens as a host starts, going by what the store names", () => {
    const { folder, left } = folderOf("named.png", "unnamed.png");
    const store = Store.open(":memory:");
    const chat = thread();
    const started = run(chat);
    const attachments = [{ file: "named.png", name: "named.png", mime: "image/png" }];
    const settled = { ...started, prompt: { ...started.prompt, attachments } };
    store.commit(chat.id, { thread: chat, runs: [settled] });
    const later = new Date(Date.now() + OVER_A_WEEK_MS).toISOString();

    new Threads({ store, providers: [], attachments: folder, now: () => later }).recover();

    expect(left()).toStrictEqual([true, false]);
    store.close();
  });
});
