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
        this.#settled(child, event.outcome, event.summary, event.usage);
        this.#counted();
        this.#family.ended();
      }
    }
  }

  /** Their session went, and those still working with it, as the chat's thread says itself. */
  lost(): void {
    for (const child of this.#children.values()) {
      if (child.running) {
        this.#settled(child, "interrupted", null, null);
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
      this.#counted();
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
    this.#counted();
  }

  /** Writes its row first: a host that stops between the two has the row closed for it. */
  #settled(
    child: Child,
    outcome: Outcome,
    summary: string | null,
    usage: SubagentUsage | null,
  ): void {
    child.write(subagentEnded(child.row, outcome, { summary, usage, at: this.#family.now() }));
    child.actor.finish(outcome, summary);
  }

  /** How many of them work changed: the chat's thread says it. */
  #counted(): void {
    const working = [...this.#children.values()].filter((child) => child.running);
    this.#family.root.state.runsBeside({ subagents: working.length });
  }
}
