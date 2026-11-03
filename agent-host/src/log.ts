// The host's log, as termd's (crates/termd/src/logfile.rs): one line a record, to a file that
// starts over at a megabyte keeping the one before as `<file>.old`, or to stderr until a file
// is set (in the foreground and the tests). Nothing else in the host writes to stdout or
// stderr.

import { closeSync, fstatSync, openSync, renameSync, writeSync } from "node:fs";

export type Level = "error" | "warn" | "info" | "debug";

const RANK: Readonly<Record<Level, number>> = { error: 0, warn: 1, info: 2, debug: 3 };
const LIMIT = 1024 * 1024;

let shown: Level = "info";
let file: { readonly path: string; fd: number; size: number } | null = null;

/** Shows records of `level` and above from now on. */
export function setLevel(level: Level): void {
  shown = level;
}

/** Writes the log to `path` from now on, after what it holds already. */
export function logTo(path: string): void {
  const fd = openSync(path, "a", 0o600);
  file = { path, fd, size: fstatSync(fd).size };
}

function write(level: Level, message: string, detail?: unknown): void {
  if (RANK[level] > RANK[shown]) {
    return;
  }
  const rest = detail === undefined ? "" : ` ${describe(detail)}`;
  const text = `${message}${rest}`.replaceAll("\n", "\\n");
  emit(`${new Date().toISOString()} ${level.toUpperCase()} ${text}\n`);
}

function emit(line: string): void {
  if (file === null) {
    process.stderr.write(line);
    return;
  }
  const bytes = Buffer.byteLength(line);
  if (file.size > 0 && file.size + bytes > LIMIT) {
    closeSync(file.fd);
    renameSync(file.path, `${file.path}.old`);
    file.fd = openSync(file.path, "a", 0o600);
    file.size = 0;
  }
  writeSync(file.fd, line);
  file.size += bytes;
}

/** An error as its name, message and causes; anything else as Bun shows it. */
function describe(detail: unknown): string {
  if (typeof detail === "string") {
    return detail;
  }
  if (detail instanceof Error) {
    const cause = detail.cause === undefined ? "" : ` (caused by ${describe(detail.cause)})`;
    return `${detail.name}: ${detail.message}${cause}`;
  }
  return Bun.inspect(detail, { compact: true, depth: 4 });
}

const at = (level: Level) => (message: string, detail?: unknown): void => {
  write(level, message, detail);
};

export const log = { error: at("error"), warn: at("warn"), info: at("info"), debug: at("debug") };
