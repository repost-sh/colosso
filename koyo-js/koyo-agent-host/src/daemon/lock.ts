// One host per folder: an exclusive `flock` on its lock file, held while the descriptor stays
// open, with the holder's pid written in it, as termd's (`koyo-rs/koyo-termd/src/claim.rs`).
// Bun has no `flock`, so libc's is called through bun:ffi.

import { closeSync, ftruncateSync, openSync, readFileSync, writeSync } from "node:fs";

import { type ConvertFns, dlopen, FFIType, read, type Symbols } from "bun:ffi";

/** LOCK_EX (2) and LOCK_NB (4): exclusive, and an answer at once rather than a wait. */
const LOCK_EX_NOW = 6;
/** macOS's EWOULDBLOCK: another descriptor holds the lock. */
const EWOULDBLOCK = 35;

const LIBC = {
  flock: { args: [FFIType.i32, FFIType.i32], returns: FFIType.i32 },
  // Where this thread's errno lives.
  __error: { args: [], returns: FFIType.ptr },
} satisfies Symbols;

type Libc = ConvertFns<typeof LIBC>;

/** Opened on first use, so nothing that never locks loads it. */
let libc: Libc | undefined;

/** The lock taken, its descriptor kept open for as long as it's held, or who holds it. */
export type Lock = { readonly kind: "held"; readonly fd: number }
  | { readonly kind: "taken"; readonly holder: string };

/** Takes the lock at `path` without waiting, writing this process's pid in it. */
export function takeLock(path: string): Lock {
  libc ??= dlopen("/usr/lib/libSystem.B.dylib", LIBC).symbols;
  // Appending never truncates, so the holder's pid survives a look by a process that fails.
  const fd = openSync(path, "a+", 0o600);
  if (libc.flock(fd, LOCK_EX_NOW) !== 0) {
    const errno = lastErrno(libc);
    closeSync(fd);
    if (errno !== EWOULDBLOCK) {
      throw new Error(`Couldn't lock ${path}: errno ${String(errno)}.`);
    }
    const pid = readFileSync(path, "utf8").trim();
    return { kind: "taken", holder: pid === "" ? "an unknown pid" : `pid ${pid}` };
  }
  ftruncateSync(fd, 0);
  writeSync(fd, String(process.pid));
  return { kind: "held", fd };
}

function lastErrno(symbols: Libc): number {
  const errno = symbols.__error();
  return errno === null ? -1 : read.i32(errno);
}
