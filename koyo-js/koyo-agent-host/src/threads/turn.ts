// The turn a session works on (docs/chat.md 4.5.8): its items by the provider's names for
// them, as they are now. Streaming text grows here first; the store keeps it at most every
// half second without a change number, since clients have it from the appends, and the step
// that closes an item writes it whole. The thread's state starts a turn with its run and ends
// it with the commit that settles the run.

import type { UUID } from "node:crypto";

import { log } from "../log";
import type { ItemId, RunId } from "../protocol/ids";
import type { Item, Run } from "../protocol/model";
import type { DriverEvent } from "../providers/provider";
import { appended, newItem } from "./records";
import { isOpen } from "./rules";

/** A provider's report that an item was seen or changed. */
type ItemReport = Extract<DriverEvent, { type: "item.started" | "item.updated" }>;

/** How long text that streamed in may wait before the store keeps it. */
const KEEP_MS = 500;

export class LiveTurn {
  readonly runId: RunId;
  /** The id the session was given the turn's prompt with. */
  readonly promptId: UUID = crypto.randomUUID();
  readonly #ids = new Map<string, ItemId>();
  readonly #items = new Map<ItemId, Item>();
  /** Items whose text grew since the store last had it. */
  readonly #unkept = new Set<ItemId>();
  readonly #keep: (item: Item) => void;
  #keeping: Timer | undefined;
  readonly #ended = Promise.withResolvers<undefined>();

  constructor(runId: RunId, keep: (item: Item) => void) {
    this.runId = runId;
    this.#keep = keep;
  }

  /** Resolves once the turn has ended. */
  get ended(): Promise<undefined> {
    return this.#ended.promise;
  }

  /**
   * The row `report` writes: the item it changed, or the item first seen, placed at the
   * timeline place `place` gives (docs/chat.md 4.4) and named for the reports after it.
   */
  seen(report: ItemReport, live: Run, place: () => number): Item {
    const known = this.#item(report.key);
    const status = report.type === "item.updated" ? report.status : known?.status ?? "running";
    if (known !== undefined) {
      return { ...known, ...report.item, status };
    }
    const parentKey = report.type === "item.started" ? report.parentKey : null;
    const parentId = parentKey === null ? null : this.#ids.get(parentKey) ?? null;
    const item = newItem(live, place(), report.item, { parentId, status });
    this.#ids.set(report.key, item.id);
    this.#items.set(item.id, item);
    return item;
  }

  /** Follows the rows a step wrote, which the store has whole. */
  remember(items: readonly Item[]): void {
    for (const item of items) {
      if (this.#items.has(item.id)) {
        this.#items.set(item.id, item);
        this.#unkept.delete(item.id);
      }
    }
  }

  /** `key`'s item grown by `text`; `undefined` when no item that streams has that name. */
  append(key: string, text: string): Item | undefined {
    const item = this.#item(key);
    const grown = item === undefined ? undefined : appended(item, text);
    if (grown === undefined) {
      log.warn(`Text came for no item that streams (${key}).`);
      return undefined;
    }
    this.#items.set(grown.id, grown);
    this.#unkept.add(grown.id);
    this.#keeping ??= setTimeout(() => {
      this.#keepAll();
    }, KEEP_MS);
    return grown;
  }

  /** The items not yet closed, which the turn's end closes. */
  open(): Item[] {
    return [...this.#items.values()].filter(isOpen);
  }

  /**
   * `rows` with the turn's items as they are now; with `all`, its open items too, whatever
   * their change number, since a client catching up missed the text they grew by.
   */
  overlay(rows: readonly Item[], all: boolean): Item[] {
    const current = rows.map((row) => this.#items.get(row.id) ?? row);
    if (!all) {
      return current;
    }
    const shown = new Set(rows.map((row) => row.id));
    const missed = this.open().filter((item) => !shown.has(item.id));
    return [...current, ...missed].toSorted((a, b) => a.ordinal - b.ordinal);
  }

  /** Keeps what's left to keep, stops keeping, and says the turn ended. */
  end(): void {
    this.#keepAll();
    this.#ended.resolve(undefined);
  }

  #item(key: string): Item | undefined {
    const id = this.#ids.get(key);
    return id === undefined ? undefined : this.#items.get(id);
  }

  #keepAll(): void {
    clearTimeout(this.#keeping);
    this.#keeping = undefined;
    for (const id of this.#unkept) {
      const item = this.#items.get(id);
      if (item === undefined) {
        continue;
      }
      try {
        this.#keep(item);
      } catch (error) {
        log.error("Keeping streamed text failed", error);
      }
    }
    this.#unkept.clear();
  }
}
