// The host's executable as the app starts it: given its folder, on the fake provider, it says
// `ready` once it listens, answers a message sent to a chat, and exits once asked to stop.

import { join } from "node:path";

import { afterEach, expect, test } from "bun:test";
import { z } from "zod";

import { folder } from "../src/daemon/folder";
import { events } from "../src/protocol/events";
import { LineReader } from "../src/server/lines";
import { prompt } from "./support/actor";
import { createdThread, LineClient } from "./support/line-client";
import { CREATE } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";

afterEach(removeScratch);

const MAIN = join(import.meta.dir, "../src/main.ts");

const Changed = z.object({ event: z.literal("thread.changed"), params: events["thread.changed"] });

/** The host's first line on stdout. */
async function firstLine(stdout: ReadableStream<Uint8Array>): Promise<string | undefined> {
  const reader = new LineReader(1024);
  for await (const chunk of stdout) {
    const read = reader.push(chunk);
    if (read.kind === "lines" && read.lines.length > 0) {
      return read.lines[0];
    }
  }
  return undefined;
}

test("starts in its folder, answers a message and exits once asked to stop", async () => {
  const root = scratch();
  const host = Bun.spawn([process.execPath, MAIN, "--dir", root, "--foreground"], {
    env: { ...process.env, KOYO_AGENT_PROVIDER: "fake" },
    stdout: "pipe",
    stderr: "ignore",
  });
  expect(await firstLine(host.stdout)).toBe("ready");

  const client = await LineClient.connect(folder(root).socket);
  await client.hello();
  client.request(1, "threads.create", CREATE);
  const thread = createdThread(await client.next());
  client.request(2, "threads.open", { threadId: thread.id, sinceSeq: null });
  client.request(3, "turns.send", {
    threadId: thread.id,
    prompt: prompt("Find the ledger's rounding bug."),
    delivery: "auto",
  });
  // The fake says back what was sent, and the run completes.
  let answer: unknown;
  let completed = false;
  while (!completed) {
    const received = await client.next();
    const changed = Changed.safeParse(received.kind === "line" ? received.line : undefined);
    if (changed.success) {
      const { items, runs } = changed.data.params;
      answer = items.find((item) => item.kind === "assistant_text")?.payload ?? answer;
      completed = runs.some((run) => run.status === "completed");
    }
  }
  expect(answer).toStrictEqual({ text: "You said: Find the ledger's rounding bug." });

  client.request(4, "shutdown", {});
  expect(await host.exited).toBe(0);
});

test("refuses a command line without its folder", async () => {
  const host = Bun.spawn([process.execPath, MAIN], { stdout: "ignore", stderr: "pipe" });
  expect(await host.exited).toBe(2);
  expect(await new Response(host.stderr).text()).toContain("usage: koyo-agent-host --dir");
});
