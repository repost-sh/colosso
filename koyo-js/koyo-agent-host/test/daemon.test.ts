// The host's process as its starter sees it: ready once it listens, gone with its socket once
// asked to stop, and leaving its folder to a host that already serves it.

import { existsSync } from "node:fs";

import { afterEach, expect, test } from "bun:test";

import { run } from "../src/daemon/daemon";
import { folder } from "../src/daemon/folder";
import { FakeProvider } from "../src/providers/fake";
import { Store } from "../src/store/store";
import { prompt } from "./support/actor";
import { ignore } from "./support/handlers";
import { LineClient } from "./support/line-client";
import { CREATE } from "./support/records";
import { removeScratch, scratch } from "./support/scratch";

afterEach(removeScratch);

/** A host serving a scratch folder, once it said it's ready. */
async function started(provider = new FakeProvider()): Promise<{
  root: string;
  outcome: Promise<unknown>;
  client: LineClient;
}> {
  const root = scratch();
  const ready = Promise.withResolvers<undefined>();
  const outcome = run({
    root,
    foreground: true,
    providers: [provider],
    configure: ignore,
    ready: () => {
      ready.resolve(undefined);
    },
  });
  await ready.promise;
  const client = await LineClient.connect(folder(root).socket);
  await client.hello();
  return { root, outcome, client };
}

test("serves until asked to stop, then takes its socket away", async () => {
  const { root, outcome, client } = await started();
  client.request(1, "shutdown", {});
  expect(await client.next()).toMatchObject({ line: { id: 1, result: {} } });
  expect(await outcome).toBe("stopped");
  expect(existsSync(folder(root).socket)).toBe(false);
});

test("leaves its folder to the host that serves it", async () => {
  const { root, outcome, client } = await started();
  const another = run({ root, foreground: true, providers: [], configure: ignore, ready: ignore });
  expect(await another).toBe("another");
  client.request(1, "shutdown", {});
  await outcome;
});

test("stopping ends a live run as the host", async () => {
  const provider = new FakeProvider();
  const { root, outcome, client } = await started(provider);
  client.request(1, "threads.create", CREATE);
  const created = await client.next();
  const id = created.kind === "line" ? threadId(created.line) : undefined;
  client.request(2, "turns.send", { threadId: id, prompt: prompt("Find it."), delivery: "auto" });
  await client.next();
  const { session } = provider;
  session.started();
  client.request(3, "shutdown", {});
  // The host asks the turn to stop, and the agent stops it.
  while (session.interrupts === 0) {
    await Bun.sleep(5);
  }
  session.finished("interrupted");
  await outcome;
  expect(session.killed).toBe(false);
  const store = Store.open(folder(root).database);
  const [thread] = store.threads();
  const stopped = thread === undefined ? [] : store.latest(thread.id, 1).runs;
  store.close();
  const byHost = { status: "interrupted", stop: { by: "host", force: false } };
  expect(stopped.at(0)).toMatchObject(byHost);
});

function threadId(line: unknown): unknown {
  if (typeof line === "object" && line !== null && "result" in line) {
    const { result } = line;
    return typeof result === "object" && result !== null && "id" in result ? result.id : undefined;
  }
  return undefined;
}
