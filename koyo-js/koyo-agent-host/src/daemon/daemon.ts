// The host's process (docs/chat.md 4.1): it takes its folder's lock or leaves the folder to
// the host that holds it, settles what a host before it left live, serves its socket, says
// it's ready once it listens, and stops on `shutdown`, SIGTERM or SIGINT, or after ten minutes
// with no client, no live run and no question waiting on a person.

import { mkdirSync, rmSync } from "node:fs";

import { handlers } from "../host";
import { log, logTo } from "../log";
import { HOST_VERSION } from "../protocol/wire";
import type { Provider } from "../providers/provider";
import { serve } from "../server/server";
import { Store } from "../store/store";
import { Threads } from "../threads/threads";
import { claim, folder } from "./folder";

/** How long the host waits with nothing to do before it exits. */
const IDLE_MS = 10 * 60_000;
/** How often it looks. */
const IDLE_CHECK_MS = 60_000;

export interface Options {
  /** The host's folder, `settings::data_dir()/agents`. */
  readonly root: string;
  /** Logs to stderr rather than the folder's log. */
  readonly foreground: boolean;
  readonly providers: readonly Provider[];
  /** Says the host listens: the `ready` line on stdout its starter waits for. */
  readonly ready: () => void;
}

/** What `run` came to: serving until stopped, or nothing, as another host serves. */
export type Served = "stopped" | "another";

/** Serves until the host stops; resolves once it has, with what was live settled. */
export async function run(options: Options): Promise<Served> {
  const dir = folder(options.root);
  const lock = claim(dir);
  if (lock.kind === "taken") {
    log.info(`Another host serves ${dir.root} (${lock.holder}).`);
    return "another";
  }
  if (!options.foreground) {
    logTo(dir.log);
  }
  mkdirSync(dir.attachments, { recursive: true, mode: 0o700 });
  const store = Store.open(dir.database);
  const threads = new Threads({
    store,
    providers: options.providers,
    attachments: dir.attachments,
    drafts: dir.drafts,
    now: () => new Date().toISOString(),
  });
  threads.recover();

  const stopped = Promise.withResolvers<undefined>();
  let stopping = false;
  const stop = (why: string): void => {
    if (stopping) {
      return;
    }
    stopping = true;
    log.info(`Stopping: ${why}.`);
    stopped.resolve(undefined);
  };
  const server = serve(dir.socket, handlers({
    threads,
    providers: options.providers,
    attachments: dir.attachments,
    shutdown: () => {
      stop("a client asked");
    },
  }));
  const onSignal = (signal: NodeJS.Signals): void => {
    stop(signal);
  };
  process.on("SIGTERM", onSignal);
  process.on("SIGINT", onSignal);
  const idle = watchIdle(() => server.clients() === 0 && !threads.busy(), () => {
    stop("nothing to do for ten minutes");
  });
  log.info(`Host ${String(HOST_VERSION)} (pid ${String(process.pid)}) serves ${dir.socket}.`);
  options.ready();

  await stopped.promise;
  clearInterval(idle);
  process.off("SIGTERM", onSignal);
  process.off("SIGINT", onSignal);
  server.stop();
  await threads.shutdown();
  store.close();
  rmSync(dir.socket, { force: true });
  log.info("Stopped.");
  return "stopped";
}

/** Calls `done` once `idle` has held for `IDLE_MS`, looking every minute. */
function watchIdle(idle: () => boolean, done: () => void): Timer {
  let since: number | undefined;
  return setInterval(() => {
    if (!idle()) {
      since = undefined;
      return;
    }
    since ??= Date.now();
    if (Date.now() - since >= IDLE_MS) {
      done();
    }
  }, IDLE_CHECK_MS);
}
