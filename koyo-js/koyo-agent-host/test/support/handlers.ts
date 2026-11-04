// Handlers that answer what a test gives them and refuse every other method, so a test names
// only what it exercises, and a method added to the protocol is added here too.

import type { Handlers } from "../../src/protocol/wire";
import { HostError } from "../../src/protocol/wire";
import type { Client } from "../../src/server/client";

function notHere(): never {
  throw new HostError("internal", "Not in this test.");
}

/** Takes what it's given and does nothing: what a test isn't about. */
export function ignore(): void {
  // Nothing to do.
}

export function handlers(given: Partial<Handlers<Client>>): Handlers<Client> {
  return {
    "hello": notHere,
    "shutdown": notHere,
    "config.set": notHere,
    "providers.status": notHere,
    "catalog": notHere,
    "threads.watch": notHere,
    "threads.create": notHere,
    "threads.open": notHere,
    "threads.history": notHere,
    "threads.close": notHere,
    "threads.update": notHere,
    "threads.read": notHere,
    "threads.delete": notHere,
    "turns.send": notHere,
    "turns.stop": notHere,
    "turns.compact": notHere,
    "queue.remove": notHere,
    "queue.edit": notHere,
    "queue.sendNow": notHere,
    "queue.resume": notHere,
    "threads.rewind": notHere,
    "requests.respond": notHere,
    ...given,
  };
}
