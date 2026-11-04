// The host's chats on disk (docs/chat.md C3, C4): one SQLite file in WAL mode, a table per
// kind of record, each row the record's JSON. A thread's actor is the only writer of its
// rows, and each of its steps is one `commit`: a transaction that stamps every changed row
// with the thread's next change number.

import { Database } from "bun:sqlite";
import { z } from "zod";

import { type MessageId, type RequestId, type RunId, ThreadId } from "../protocol/ids";
import type { Rows } from "../protocol/methods";
import { Item, Request, Run, Thread } from "../protocol/model";
import { MIGRATIONS } from "./migrations";

/** What one step of a thread's actor changed. */
export interface Change {
  readonly thread: Thread | null;
  readonly runs: readonly Run[];
  readonly items: readonly Item[];
  readonly requests: readonly Request[];
}

/** A change as written: its rows stamped with the thread's change number, `seq`. */
interface Committed extends Change {
  readonly seq: number;
}

const DataRow = z.object({ data: z.string() });
const HeadRow = z.object({ head: z.int() });
const VersionRow = z.object({ user_version: z.int() });
const IdRow = z.object({ id: ThreadId });
const LastRow = z.object({ last: z.int().nullable() });
const FileRow = z.object({ file: z.string() });

/** What a thread is doing, as `Store.state` reads it. */
export interface State {
  readonly unsettled: readonly Run[];
  readonly ended: Run | undefined;
  readonly pending: readonly Request[];
}

export class Store {
  readonly #db: Database;

  private constructor(db: Database) {
    this.#db = db;
  }

  /** Opens the store at `path` (`:memory:` for a store of its own), migrated to now. */
  static open(path: string): Store {
    const db = new Database(path, { create: true, strict: true });
    db.run("PRAGMA journal_mode = WAL");
    db.run("PRAGMA synchronous = NORMAL");
    db.run("PRAGMA foreign_keys = ON");
    db.run("PRAGMA busy_timeout = 5000");
    migrate(db);
    return new Store(db);
  }

  close(): void {
    this.#db.close();
  }

  /** Every thread, newest first. */
  threads(): Thread[] {
    const rows = this.#db.query("SELECT data FROM threads ORDER BY updated_at DESC").all();
    return rows.map((row) => decode(Thread, row));
  }

  thread(id: ThreadId): Thread | undefined {
    const row = this.#db.query("SELECT data FROM threads WHERE id = ?").get(id);
    return row === null ? undefined : decode(Thread, row);
  }

  /** The thread's latest change number, or 0 for a thread that isn't stored. */
  head(id: ThreadId): number {
    const row = this.#db.query("SELECT head FROM threads WHERE id = ?").get(id);
    return row === null ? 0 : HeadRow.parse(row).head;
  }

  /** The thread's latest `count` runs, queued ones among them, with their rows. */
  latest(id: ThreadId, count: number): Rows {
    return this.#page(this.#query(Run, `${RUNS} ORDER BY id DESC LIMIT ?`, id, count));
  }

  /** The `count` runs sent before run `before`, with their rows. */
  before(id: ThreadId, before: RunId, count: number): Rows {
    const runs = `${RUNS} AND id < ? ORDER BY id DESC LIMIT ?`;
    return this.#page(this.#query(Run, runs, id, before, count));
  }

  /** The rows that changed after change number `since`. */
  since(id: ThreadId, since: number): Rows {
    return {
      runs: this.#query(Run, `${RUNS} AND seq > ? ORDER BY id`, id, since),
      items: this.#query(Item, `${ITEMS} AND seq > ? ORDER BY ordinal`, id, since),
      requests: this.#query(Request, `${REQUESTS} AND seq > ? ORDER BY id`, id, since),
    };
  }

  /** Writes what one actor step changed, as one transaction, and returns it stamped. */
  commit(id: ThreadId, change: Partial<Change>): Committed {
    return this.#db.transaction((): Committed => {
      const seq = this.head(id) + 1;
      const given = change.thread ?? null;
      const thread = given === null ? null : { ...given, seq };
      if (thread === null) {
        this.#db.query("UPDATE threads SET head = ? WHERE id = ?").run(seq, id);
      } else {
        this.#db
          .query(
            `INSERT INTO threads (id, cwd, head, updated_at, data) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET
               cwd = excluded.cwd, head = excluded.head,
               updated_at = excluded.updated_at, data = excluded.data`,
          )
          .run(id, thread.cwd, seq, thread.updatedAt, JSON.stringify(thread));
      }
      const runs = (change.runs ?? []).map((run) => ({ ...run, seq }));
      const items = (change.items ?? []).map((item) => ({ ...item, seq }));
      const requests = (change.requests ?? []).map((request) => ({ ...request, seq }));
      const putRun = this.#db.query(
        `INSERT OR REPLACE INTO runs (id, thread_id, status, message_id, seq, data)
         VALUES (?, ?, ?, ?, ?, ?)`,
      );
      for (const run of runs) {
        const message = run.prompt.messageId;
        putRun.run(run.id, id, run.status, message, seq, JSON.stringify(run));
      }
      const putItem = this.#db.query(
        `INSERT OR REPLACE INTO items (id, thread_id, run_id, ordinal, message_id, seq, data)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      );
      for (const item of items) {
        const message = item.kind === "user_message" ? item.payload.messageId : null;
        putItem.run(item.id, id, item.runId, item.ordinal, message, seq, JSON.stringify(item));
      }
      const putRequest = this.#db.query(
        `INSERT OR REPLACE INTO requests (id, thread_id, run_id, status, seq, data)
         VALUES (?, ?, ?, ?, ?, ?)`,
      );
      for (const request of requests) {
        putRequest.run(request.id, id, request.runId, request.status, seq, JSON.stringify(request));
      }
      return { seq, thread, runs, items, requests };
    }).immediate();
  }

  /**
   * What a thread is doing: its runs not yet settled (queued ones among them), the latest
   * one that ran and ended, and the requests that wait for the person.
   */
  state(id: ThreadId): State {
    const latest = `${RUNS} AND status IN ${ENDED} ORDER BY id DESC LIMIT 1`;
    const ended = this.#query(Run, latest, id);
    return {
      unsettled: this.#query(Run, `${RUNS} AND status IN ${UNSETTLED} ORDER BY id`, id),
      ended: ended.at(0),
      pending: this.#query(Request, `${REQUESTS} AND status = 'pending' ORDER BY id`, id),
    };
  }

  /** The place in the timeline the thread's next item takes. */
  nextOrdinal(id: ThreadId): number {
    const sql = "SELECT MAX(ordinal) AS last FROM items WHERE thread_id = ?";
    const { last } = LastRow.parse(this.#db.query(sql).get(id));
    return last === null ? 0 : last + 1;
  }

  /** The run message `messageId` made, if it made one: a send repeated is answered from it. */
  runOf(id: ThreadId, messageId: MessageId): Run | undefined {
    const sql = `${RUNS} AND message_id = ?`;
    return this.#query(Run, sql, id, messageId).at(0);
  }

  run(id: ThreadId, runId: RunId): Run | undefined {
    return this.#query(Run, `${RUNS} AND id = ?`, id, runId).at(0);
  }

  /** The item of message `messageId`, once it started a run or joined one. */
  userMessage(id: ThreadId, messageId: MessageId): Item | undefined {
    const sql = `${ITEMS} AND message_id = ?`;
    return this.#query(Item, sql, id, messageId).at(0);
  }

  /** A run's items, in timeline order. */
  itemsOf(id: ThreadId, runId: RunId): Item[] {
    return this.#query(Item, `${ITEMS} AND run_id = ? ORDER BY ordinal`, id, runId);
  }

  request(id: ThreadId, requestId: RequestId): Request | undefined {
    return this.#query(Request, `${REQUESTS} AND id = ?`, id, requestId).at(0);
  }

  /** The files the thread's messages attach, in the attachments folder. */
  attachments(id: ThreadId): string[] {
    const sql = `SELECT DISTINCT file FROM (${ATTACHED}) WHERE thread_id = ?`;
    return this.#db.query(sql).all(id).map((row) => FileRow.parse(row).file);
  }

  /** The files any message attaches, in the attachments folder. */
  allAttachments(): Set<string> {
    const rows = this.#db.query(`SELECT DISTINCT file FROM (${ATTACHED})`).all();
    return new Set(rows.map((row) => FileRow.parse(row).file));
  }

  /** The threads with a run that hasn't settled, which a host that stopped left so. */
  unsettledThreads(): ThreadId[] {
    const sql = `SELECT DISTINCT thread_id AS id FROM runs WHERE status IN ${UNSETTLED}`;
    return this.#db.query(sql).all().map((row) => IdRow.parse(row).id);
  }

  /**
   * Keeps a streaming item's text so far, without a change number: clients have it from
   * the live appends, and get it whole when they open the thread (docs/chat.md 4.5.8).
   */
  keepText(item: Item): void {
    this.#db.query("UPDATE items SET data = ? WHERE id = ?").run(JSON.stringify(item), item.id);
  }

  /** Forgets the thread and every row of it. */
  delete(id: ThreadId): void {
    this.#db.query("DELETE FROM threads WHERE id = ?").run(id);
  }

  /** The runs a page holds, newest first as queried, with their rows, all oldest first. */
  #page(newestFirst: Run[]): Rows {
    const runs = newestFirst.toReversed();
    const [oldest, newest] = [runs.at(0), runs.at(-1)];
    if (oldest === undefined || newest === undefined) {
      return { runs, items: [], requests: [] };
    }
    // The page's runs are every run between its oldest and its newest.
    const span = [oldest.threadId, oldest.id, newest.id];
    const inPage = "AND run_id BETWEEN ? AND ?";
    return {
      runs,
      items: this.#query(Item, `${ITEMS} ${inPage} ORDER BY ordinal`, ...span),
      requests: this.#query(Request, `${REQUESTS} ${inPage} ORDER BY id`, ...span),
    };
  }

  #query<T>(schema: z.ZodType<T>, sql: string, ...bindings: (string | number)[]): T[] {
    return this.#db.query(sql).all(...bindings).map((row) => decode(schema, row));
  }
}

const RUNS = "SELECT data FROM runs WHERE thread_id = ?";
const UNSETTLED = "('queued', 'starting', 'running', 'stopping')";
/** Every file a message names: where its run started, or as an item when it joined one. */
const ATTACHED = `
  SELECT thread_id, json_extract(attached.value, '$.file') AS file
    FROM runs, json_each(runs.data, '$.prompt.attachments') AS attached
  UNION
  SELECT thread_id, json_extract(attached.value, '$.file') AS file
    FROM items, json_each(items.data, '$.payload.attachments') AS attached
    WHERE items.message_id IS NOT NULL`;
/** The settled runs that ran: a removed one never did. */
const ENDED = "('completed', 'interrupted', 'failed')";
const ITEMS = "SELECT data FROM items WHERE thread_id = ?";
const REQUESTS = "SELECT data FROM requests WHERE thread_id = ?";

function decode<T>(schema: z.ZodType<T>, row: unknown): T {
  const json: unknown = JSON.parse(DataRow.parse(row).data);
  return schema.parse(json);
}

function migrate(db: Database): void {
  const { user_version: done } = VersionRow.parse(db.query("PRAGMA user_version").get());
  if (done > MIGRATIONS.length) {
    throw new Error(
      `The chats were written by a newer build (schema ${String(done)}, this build knows `
      + `${String(MIGRATIONS.length)}).`,
    );
  }
  db.transaction(() => {
    for (const [index, sql] of MIGRATIONS.entries()) {
      if (index >= done) {
        db.run(sql);
      }
    }
    db.run(`PRAGMA user_version = ${String(MIGRATIONS.length)}`);
  }).immediate();
}
