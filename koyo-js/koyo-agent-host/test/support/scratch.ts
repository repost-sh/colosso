// Folders tests write in: inside the checkout, in `.superpowers/t/`, which git ignores, and
// named short, as a socket's path can't pass 104 bytes. Each is removed by `removeScratch`,
// which a test file runs after each test.

import { mkdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";

/** This file is at koyo-js/koyo-agent-host/test/support/ in the checkout. */
const ROOT = join(import.meta.dir, "../../../../.superpowers/t");

let count = 0;
const made: string[] = [];

/** A new, empty folder of this test process's own, until the test ends. */
export function scratch(): string {
  count += 1;
  const dir = join(ROOT, `ah${String(process.pid)}-${String(count)}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  made.push(dir);
  return dir;
}

/** Removes every folder `scratch` made. */
export function removeScratch(): void {
  for (const dir of made.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** The permission bits of `path`, in octal. */
export function permissions(path: string): string {
  return (statSync(path).mode % 0o1000).toString(8);
}
