// Going back with the files (docs/chat.md C11): what's offered, what stands in its way, and that
// the conversation and the files go together or not at all.

import { utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { HostError } from "../src/protocol/wire";
import type { FileChanges } from "../src/providers/provider";
import { prompt } from "./support/actor";
import { type Chat, chat, said, shown } from "./support/chat";
import { CREATE } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";
import { settle } from "./support/wait";

afterEach(removeScratch);

const HOUR_MS = 60 * 60 * 1000;

/** A chat in a folder of its own. */
type InFolder = Chat & { readonly cwd: string };

function inFolder(): InFolder {
  const cwd = scratch();
  return { ...chat(cwd), cwd };
}

/** A file in the chat's folder, written as long ago or as far ahead as `fromNowMs` says. */
function fileOf(on: InFolder, name: string, fromNowMs: number): string {
  const path = join(on.cwd, name);
  writeFileSync(path, "words");
  const at = new Date(Date.now() + fromNowMs);
  utimesSync(path, at, at);
  return path;
}

/** What the provider says of putting back `files`. */
function changing(...files: string[]): FileChanges {
  return { files, insertions: 1, deletions: 1 };
}

describe("what going back offers for the files", () => {
  test("is what the provider says it would change, from the chat's folder", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    const file = fileOf(on, "a.txt", -HOUR_MS);
    on.provider.changes = { files: [file, join(on.cwd, "gone.txt")], insertions: 1, deletions: 2 };
    expect(await on.threads.filesBack(on.thread.id, run)).toEqual({
      files: ["a.txt", "gone.txt"],
      insertions: 1,
      deletions: 2,
      blocked: null,
    });
  });

  test("is nothing when the provider has no record, or nothing would change", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    expect(await on.threads.filesBack(on.thread.id, run)).toBeNull();
    on.provider.changes = { files: [], insertions: 0, deletions: 0 };
    expect(await on.threads.filesBack(on.thread.id, run)).toBeNull();
  });

  test("is nothing when finding out fails, as going back doesn't need it", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = new Error("The CLI went.");
    expect(await on.threads.filesBack(on.thread.id, run)).toBeNull();
  });

  test("names a file written since the chat last worked, which it won't overwrite", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS), fileOf(on, "b.txt", HOUR_MS));
    const back = await on.threads.filesBack(on.thread.id, run);
    expect(back?.blocked).toBe("b.txt changed since this chat last worked on it.");
  });

  test("says how many more were written since", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", HOUR_MS), fileOf(on, "b.txt", HOUR_MS));
    const back = await on.threads.filesBack(on.thread.id, run);
    expect(back?.blocked).toBe("a.txt and 1 more changed since this chat last worked on them.");
  });

  test("waits for another chat at work in the same checkout, not one elsewhere", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));

    const elsewhere = on.threads.create({ ...CREATE, cwd: scratch() });
    on.threads.actor(elsewhere.id).send(prompt("Elsewhere"), "auto");
    await settle();
    expect((await on.threads.filesBack(on.thread.id, run))?.blocked).toBeNull();

    const beside = on.threads.create({ ...CREATE, cwd: on.cwd });
    on.threads.actor(beside.id).send(prompt("Beside"), "auto");
    await settle();
    expect((await on.threads.filesBack(on.thread.id, run))?.blocked).toBe(
      "Another chat is at work in this checkout.",
    );
  });
});

describe("going back with the files", () => {
  test("puts them back from the message, and takes it out of the chat", async () => {
    const on = inFolder();
    await said(on, "First");
    const run = await said(on, "Second");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));
    const back = await on.threads.rewind(on.thread.id, run, true);
    expect(back.text).toBe("Second");
    expect(on.provider.restored).toEqual([run]);
    expect(shown(on)).toEqual(["First"]);
  });

  test("leaves them alone unless asked", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));
    await on.threads.rewind(on.thread.id, run, false);
    expect(on.provider.restored).toEqual([]);
    expect(shown(on)).toEqual([]);
  });

  test("is refused, the chat as it was, when someone else wrote one since", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", HOUR_MS));
    expect(on.threads.rewind(on.thread.id, run, true)).rejects.toThrow(HostError);
    await settle();
    expect(on.provider.restored).toEqual([]);
    expect(shown(on)).toEqual(["First"]);
  });

  test("is refused, the chat as it was, when there's nothing to put back", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    expect(on.threads.rewind(on.thread.id, run, true)).rejects.toThrow(HostError);
    await settle();
    expect(shown(on)).toEqual(["First"]);
  });

  test("keeps the chat as it was when the provider fails to", async () => {
    const on = inFolder();
    const run = await said(on, "First");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));
    on.provider.onRestore = () => {
      throw new Error("The CLI went.");
    };
    expect(on.threads.rewind(on.thread.id, run, true)).rejects.toThrow("The CLI went.");
    await settle();
    expect(shown(on)).toEqual(["First"]);
  });

  test("doesn't take its own writes for someone else's the next time", async () => {
    const on = inFolder();
    const first = await said(on, "First");
    const second = await said(on, "Second");
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));
    on.provider.entry = "an entry";
    await Bun.sleep(5);
    on.provider.onRestore = () => {
      writeFileSync(join(on.cwd, "a.txt"), "as it was");
    };
    await on.threads.rewind(on.thread.id, second, true);
    expect((await on.threads.filesBack(on.thread.id, first))?.blocked).toBeNull();
  });

  test("counts a run since removed as the chat's own work", async () => {
    const on = inFolder();
    const first = await said(on, "First");
    await Bun.sleep(5);
    const second = await said(on, "Second");
    on.provider.entry = "an entry";
    // Written by the run that's going, later than the one that's left ended.
    const file = fileOf(on, "a.txt", -1);
    await on.threads.rewind(on.thread.id, second, false);
    on.provider.changes = changing(file);
    expect((await on.threads.filesBack(on.thread.id, first))?.blocked).toBeNull();
  });

  test("keeps when it last wrote with the chat, for after a restart", async () => {
    const on = inFolder();
    const first = await said(on, "First");
    const second = await said(on, "Second");
    on.provider.entry = "an entry";
    on.provider.changes = changing(fileOf(on, "a.txt", -HOUR_MS));
    await on.threads.rewind(on.thread.id, second, true);
    expect(on.store.thread(on.thread.id)?.filesSettledAt).not.toBeNull();
    expect((await on.threads.filesBack(on.thread.id, first))?.blocked).toBeNull();
  });
});
