import { chmodSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, test } from "bun:test";

import {
  candidates,
  findClaude,
  loginEnvironment,
  parseEnv,
  Versions,
} from "../../src/providers/claude/environment";
import { removeScratch, scratch } from "../support/scratch";

afterEach(removeScratch);

// A file written and run straight away waits on macOS's check of a new program, which can
// take many seconds; the programs run here are the system's own, under names of the test's.

describe("the login environment", () => {
  test("is what the login shell prints, without the variables a Claude session sets", async () => {
    const home = scratch();
    writeFileSync(join(home, ".profile"), [
      // A login rc file that prints, as some do.
      "echo 'Welcome back.'",
      "export PATH=\"/login/bin:$PATH\" KEEP=yes CLAUDECODE=1",
      "export NOTE='two",
      "lines'",
    ].join("\n"));
    const env = await loginEnvironment({
      SHELL: "/bin/sh",
      HOME: home,
      PATH: "/usr/bin:/bin",
      CLAUDE_CODE_ENTRYPOINT: "sdk-ts",
    });
    expect(env["PATH"]?.startsWith("/login/bin:")).toBe(true);
    expect(env["KEEP"]).toBe("yes");
    expect(env["NOTE"]).toBe("two\nlines");
    expect(env["HOME"]).toBe(home);
    expect(env["CLAUDECODE"]).toBeUndefined();
    expect(env["CLAUDE_CODE_ENTRYPOINT"]).toBeUndefined();
  });

  test("is the host's, with the usual places on PATH, when the login shell fails", async () => {
    const env = await loginEnvironment({
      SHELL: "/usr/bin/false",
      PATH: "/custom/bin:/usr/bin",
      CLAUDECODE: "1",
      KEEP: "yes",
    });
    expect(env["PATH"]?.split(":")).toEqual([
      "/custom/bin",
      "/usr/bin",
      "/opt/homebrew/bin",
      "/usr/local/bin",
      "/bin",
      "/usr/sbin",
      "/sbin",
    ]);
    expect(env["KEEP"]).toBe("yes");
    expect(env["CLAUDECODE"]).toBeUndefined();
  });

  test("takes each variable from env -0's output, rc noise before it dropped", () => {
    expect(parseEnv("motd\nPATH=/usr/bin\0EMPTY=\0EQ=a=b\0=bad\0")).toEqual({
      PATH: "/usr/bin",
      EMPTY: "",
      EQ: "a=b",
    });
  });
});

describe("claude", () => {
  test("is looked for in Koyo's setting, the login PATH, then where its installers put it", () => {
    expect(candidates("/custom/claude", { PATH: "/a:/b", HOME: "/Users/dev" })).toEqual([
      "/custom/claude",
      "/a/claude",
      "/b/claude",
      "/Users/dev/.local/bin/claude",
      "/Users/dev/.claude/local/claude",
      "/opt/homebrew/bin/claude",
      "/usr/local/bin/claude",
    ]);
  });

  test("is the first of them that's a program", () => {
    const dir = scratch();
    const first = join(dir, "first");
    const second = join(dir, "second");
    writeFileSync(join(dir, "note"), "");
    program(join(dir, "claude"));
    expect(findClaude(null, { PATH: `${first}:${dir}:${second}`, HOME: dir }))
      .toBe(join(dir, "claude"));
    expect(findClaude(join(dir, "note"), { PATH: dir, HOME: dir })).toBe(join(dir, "claude"));
    const mine = program(join(dir, "mine"));
    expect(findClaude(mine, { PATH: dir, HOME: dir })).toBe(mine);
  });

  test("has its version read once for each path, and again after a failed read", async () => {
    const dir = scratch();
    const claude = join(dir, "claude");
    const versions = new Versions();
    symlinkSync("/usr/bin/false", claude);
    expect(await versions.of(claude, { PATH: "/usr/bin:/bin" })).toBeNull();
    rmSync(claude);
    // The system's shell prints a version, as `claude --version` does.
    symlinkSync("/bin/sh", claude);
    const version = await versions.of(claude, { PATH: "/usr/bin:/bin" });
    expect(version).toMatch(/^\d+\.\d+\.\d+/u);
    rmSync(claude);
    symlinkSync("/usr/bin/false", claude);
    expect(await versions.of(claude, { PATH: "/usr/bin:/bin" })).toBe(version);
  });
});

/** A file `findClaude` takes for a program, which it never runs. */
function program(path: string): string {
  writeFileSync(path, "#!/bin/sh\n");
  chmodSync(path, 0o755);
  return path;
}
