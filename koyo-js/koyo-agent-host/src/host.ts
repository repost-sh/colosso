// What the host does for each method (docs/chat.md 4.3), over its threads and providers. A
// method about a thread is a step of that thread's actor, answered at once; only what waits
// on a provider's process answers later.

import { type Handlers, HOST_VERSION } from "./protocol/wire";
import type { Provider } from "./providers/provider";
import type { Client } from "./server/client";
import { checkAttachments } from "./threads/attachments";
import type { Threads } from "./threads/threads";

export interface Host {
  readonly threads: Threads;
  readonly providers: readonly Provider[];
  /** The folder the messages' attachments are in. */
  readonly attachments: string;
  /** Stops the host: its reply goes first, as the stop starts after the step that asks. */
  readonly shutdown: () => void;
}

export function handlers(host: Host): Handlers<Client> {
  const { threads } = host;
  return {
    "hello": ({ version }, client) => {
      client.greet(version);
      client.onClose(() => {
        threads.forget(client);
      });
      return { version: HOST_VERSION, pid: process.pid, busy: threads.busy() };
    },
    "shutdown": () => {
      host.shutdown();
      return {};
    },
    // What Koyo's settings say of the agents: kept for the sessions threads open, and handed to
    // every provider to follow.
    "config.set": (config) => {
      threads.configure(config);
      for (const provider of host.providers) {
        provider.setConfig(config);
      }
      return {};
    },
    "providers.status": async ({ refresh }) =>
      await Promise.all(host.providers.map(async (provider) => await provider.status(refresh))),
    // A new chat starts in Koyo's default mode, when it names one, else the provider's (C12).
    "catalog": async ({ provider, cwd }) => {
      const catalog = await threads.provider(provider).catalog(cwd);
      return { ...catalog, defaultMode: threads.config.defaultMode ?? catalog.defaultMode };
    },
    "threads.watch": (_params, client) => threads.watch(client),
    "threads.create": (params) => threads.create(params),
    "threads.open": ({ threadId, sinceSeq }, client) =>
      threads.actor(threadId).open(client, sinceSeq),
    "threads.history": ({ threadId, before, runs }) =>
      threads.actor(threadId).history(before, runs),
    "threads.close": ({ threadId }, client) => {
      threads.actor(threadId).close(client);
      return {};
    },
    "threads.update": ({ threadId, ...change }) => threads.actor(threadId).update(change),
    "threads.read": ({ threadId }) => {
      threads.actor(threadId).read();
      return {};
    },
    "threads.delete": ({ threadId }) => {
      threads.delete(threadId);
      return {};
    },
    "turns.send": ({ threadId, prompt, delivery }) => {
      const actor = threads.actor(threadId);
      checkAttachments(host.attachments, prompt.attachments);
      return actor.send(prompt, delivery);
    },
    "turns.stop": ({ threadId, force }) => {
      threads.actor(threadId).stop(force);
      return {};
    },
    "turns.compact": ({ threadId }) => ({ runId: threads.actor(threadId).compact() }),
    "queue.remove": ({ threadId, runId }) => {
      threads.actor(threadId).removeQueued(runId);
      return {};
    },
    "queue.edit": ({ threadId, runId, text, attachments }) => {
      const actor = threads.actor(threadId);
      checkAttachments(host.attachments, attachments);
      actor.editQueued(runId, text, attachments);
      return {};
    },
    "queue.sendNow": ({ threadId, runId }) => ({
      delivered: threads.actor(threadId).sendNow(runId),
    }),
    "queue.resume": ({ threadId }) => {
      threads.actor(threadId).resume();
      return {};
    },
    "requests.respond": ({ threadId, requestId, response }) => {
      threads.actor(threadId).respond(requestId, response);
      return {};
    },
  };
}
