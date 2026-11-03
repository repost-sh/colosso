import { closeSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import { claim, folder } from "../src/daemon/folder";
import { type Lock, takeLock } from "../src/daemon/lock";
import { permissions, removeScratch, scratch } from "./support/scratch";

afterEach(removeScratch);

const dir = scratch;

function release(lock: Lock): void {
  if (lock.kind === "held") {
    closeSync(lock.fd);
  }
}

describe("takeLock", () => {
  test("is one holder's at a time, its pid written in it", () => {
    const path = join(dir(), "host.lock");
    const first = takeLock(path);
    expect(first.kind).toBe("held");
    expect(readFileSync(path, "utf8")).toBe(String(process.pid));
    expect(takeLock(path)).toStrictEqual({ kind: "taken", holder: `pid ${String(process.pid)}` });
    // A look that failed left the pid as it was.
    expect(readFileSync(path, "utf8")).toBe(String(process.pid));
    release(first);
    const again = takeLock(path);
    expect(again.kind).toBe("held");
    release(again);
  });
});

describe("claim", () => {
  test("keeps the folder to this user and clears a socket a dead host left", () => {
    const root = join(dir(), "agents");
    const paths = folder(root);
    const first = claim(paths);
    expect(first.kind).toBe("held");
    expect(permissions(root)).toBe("700");
    release(first);
    writeFileSync(paths.socket, "");
    const second = claim(paths);
    expect(second.kind).toBe("held");
    expect(() => statSync(paths.socket)).toThrow();
    release(second);
  });

  test("leaves the socket to the host that holds the lock", () => {
    const paths = folder(join(dir(), "agents"));
    const first = claim(paths);
    expect(first.kind).toBe("held");
    writeFileSync(paths.socket, "");
    expect(claim(paths).kind).toBe("taken");
    expect(statSync(paths.socket).isFile()).toBe(true);
    release(first);
  });
});
