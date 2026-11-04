// A thread's subagents (docs/chat.md 4.5.10). A subagent works on a thread of its own, whose
// timeline has what it did, and shows as a row in the timeline it was started from; it runs on
// its own, past the run that started it if it's in the background, until its session says it
// ended or goes. The actor of the thread with the session keeps them all, a subagent's own
// subagents too, whose rows are in the timeline of the subagent that started them.

import { newId, ThreadId } from "../protocol/ids";
import type { SubagentUsage, Thread } from "../protocol/model";
import type { Outcome, SubagentEvent } from "../providers/provider";
import type { ThreadActor } from "./actor";
import {
  subagentAgain,
  subagentChanged,
  subagentEnded,
  type SubagentRow,
  subagentRow,
  subagentThread,
} from "./records";
import type { ThreadState } from "./state";

/** What a thread's subagents work with, from its actor. */
export interface Family {
  readonly root: ThreadActor;
  readonly now: () => string;
  readonly spawn: (thread: Thread, prompt: string, stop: () => void) => ThreadActor;
  /** Asks the session to stop subagent `key`. */
  readonly stop: (key: string) => void;
  /** One of them ended, so the session may be done with. */
  readonly ended: () => void;
}

class Child {
  readonly actor: ThreadActor;
  /** The thread whose timeline its row is in. */
  readonly #owner: ThreadState;
  #row: SubagentRow;

  constructor(actor: ThreadActor, owner: ThreadState, row: SubagentRow) {
    this.actor = actor;
    this.#owner = owner;
    this.#row = row;
    owner.commit({ items: [row] });
  }

  get row(): SubagentRow {
    return this.#row;
  }

  get running(): boolean {
    return this.#row.status === "running";
  }

  /** Writes what its row is now. */
  write(row: SubagentRow): void {
    this.#row = row;
    this.#owner.commit({ items: [row] });
  }
}

export class Subagents {
  readonly #family: Family;
  /** Every subagent the session started, by its name for them: one may be started again. */
  readonly #children = new Map<string, Child>();

  constructor(family: Family) {
    this.#family = family;
  }

  /** Whether any of them is working. */
  live(): boolean {
    return [...this.#children.values()].some((child) => child.running);
  }

  /** Takes what the session said of one of them. */
  hear(event: SubagentEvent): void {
    if (event.type === "subagent.started") {
      this.#started(event);
      return;
    }
    const child = this.#children.get(event.key);
    if (child === undefined) {
      return;
    }
    switch (event.type) {
      case "subagent.changed": {
        if (event.change.model !== undefined) {
          child.actor.update({ model: event.change.model });
        }
        child.write(subagentChanged(child.row, event.change));
        return;
      }
      case "subagent.event": {
        child.actor.state.heard(event.event);
        return;
      }
      case "subagent.ended": {
        this.#ended(child, event.outcome, event.summary, event.usage);
      }
    }
  }

  /** Their session went, and those still working with it. */
  lost(): void {
    for (const child of this.#children.values()) {
      if (child.running) {
        this.#ended(child, "interrupted", null, null);
      }
    }
  }

  #started(event: Extract<SubagentEvent, { type: "subagent.started" }>): void {
    const { key, parentKey, subagent } = event;
    const { root, now, spawn, stop } = this.#family;
    const at = now();
    const known = this.#children.get(key);
    if (known !== undefined) {
      known.write(subagentAgain(known.row));
      known.actor.takeUp(subagent.prompt);
      return;
    }
    const starter = parentKey === null ? undefined : this.#children.get(parentKey)?.actor;
    const owner = (starter ?? root).state;
    const run = owner.latest();
    if (run === undefined) {
      return;
    }
    const threadId = newId(ThreadId);
    const row = subagentRow(run, { ordinal: owner.place(), at }, threadId, subagent);
    const ids = { threadId, itemId: row.id };
    const thread = subagentThread(owner.thread, ids, subagent, at);
    const actor = spawn(thread, subagent.prompt, () => {
      stop(key);
    });
    this.#children.set(key, new Child(actor, owner, row));
  }

  /** Writes its row first: a host that stops between the two has the row closed for it. */
  #ended(
    child: Child,
    outcome: Outcome,
    summary: string | null,
    usage: SubagentUsage | null,
  ): void {
    child.write(subagentEnded(child.row, outcome, { summary, usage, at: this.#family.now() }));
    child.actor.finish(outcome, summary);
    this.#family.ended();
  }
}
