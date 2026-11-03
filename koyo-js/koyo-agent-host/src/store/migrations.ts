// The store's schema, as the migrations that build it. Each entry runs once, in order, in a
// transaction; `PRAGMA user_version` counts those that ran. An entry is never edited once
// committed: a change is a new entry.
//
// Every table keeps its record whole as the JSON the protocol sends (`data`), with the
// columns queries need beside it. `message_id` is the id the app gave the message a run
// started from, or a user message's, so a send repeated is found by it. `seq` is the
// thread's change number when the row last changed (docs/chat.md C4); `threads.head` is the
// thread's latest. Ids are UUIDv7, so a thread's runs sort by when they were sent, and
// history pages by run.

export const MIGRATIONS: readonly string[] = [
  `
  CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    cwd TEXT NOT NULL,
    head INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    data TEXT NOT NULL
  ) STRICT;
  CREATE INDEX threads_by_cwd ON threads (cwd, updated_at);

  CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    message_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    data TEXT NOT NULL
  ) STRICT;
  CREATE INDEX runs_by_order ON runs (thread_id, id);
  CREATE INDEX runs_by_message ON runs (thread_id, message_id);
  CREATE INDEX runs_by_change ON runs (thread_id, seq);
  CREATE INDEX runs_unsettled ON runs (status) WHERE status IN
    ('queued', 'starting', 'running', 'stopping');

  CREATE TABLE items (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    message_id TEXT,
    seq INTEGER NOT NULL,
    data TEXT NOT NULL
  ) STRICT;
  CREATE INDEX items_by_run ON items (thread_id, run_id, ordinal);
  CREATE INDEX items_by_message ON items (thread_id, message_id) WHERE message_id IS NOT NULL;
  CREATE INDEX items_by_change ON items (thread_id, seq);

  CREATE TABLE requests (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    status TEXT NOT NULL,
    seq INTEGER NOT NULL,
    data TEXT NOT NULL
  ) STRICT;
  CREATE INDEX requests_pending ON requests (thread_id) WHERE status = 'pending';
  CREATE INDEX requests_by_run ON requests (thread_id, run_id);
  CREATE INDEX requests_by_change ON requests (thread_id, seq);
  `,
];
