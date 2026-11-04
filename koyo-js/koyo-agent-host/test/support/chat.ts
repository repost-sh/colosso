// A chat on a fake provider, from the top of the host's threads down.

import type { RunId } from "../../src/protocol/ids";
import type { Thread } from "../../src/protocol/model";
import { FakeProvider } from "../../src/providers/fake";
import { Store } from "../../src/store/store";
import { Threads } from "../../src/threads/threads";
import { prompt } from "./actor";
import { CREATE } from "./records";
import { scratch } from "./scratch";
import { settle } from "./wait";

export interface Chat {
  readonly store: Store;
  readonly provider: FakeProvider;
  readonly threads: Threads;
  readonly thread: Thread;
}

/** A chat in `cwd`. */
export function chat(cwd = "/work"): Chat {
  const store = Store.open(":memory:");
  const provider = new FakeProvider();
  const threads = new Threads({
    store,
    providers: [provider],
    attachments: scratch(),
    drafts: "/not/here/chat-drafts.json",
    now: () => new Date().toISOString(),
  });
  return { store, provider, threads, thread: threads.create({ ...CREATE, cwd }) };
}

/** A turn of `text`, from start to finish. */
export async function said(on: Chat, text: string): Promise<RunId> {
  const { runId } = on.threads.actor(on.thread.id).send(prompt(text), "auto");
  on.provider.session.started();
  await settle();
  on.provider.session.finished();
  await settle();
  return runId;
}

/** What the chat shows the person said. */
export function shown(on: Chat): string[] {
  return on.store.latest(on.thread.id, 10).runs.map((run) => run.prompt.text);
}
