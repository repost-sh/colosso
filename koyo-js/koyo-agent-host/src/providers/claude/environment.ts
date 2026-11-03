// Where Claude runs and which Claude (docs/chat.md 4.2): the person's login environment, read
// once from their shell, so a Finder launch's bare PATH is never what the agent sees; and
// their own installed `claude` (C7), found where Koyo's setting points or where installers
// put it, with its version read once for each path.

import { spawn } from "node:child_process";
import { accessSync, constants, statSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";

import { log } from "../../log";
import { signalGroup } from "./spawn";

/** An environment as the CLI is given it, whole. */
export type Env = Readonly<Record<string, string>>;

/** How long the login shell may take to print its environment. */
const LOGIN_MS = 5_000;
/** How long `claude --version` may take. */
const VERSION_MS = 10_000;
/** The most a shell may print before it's taken as broken. */
const OUTPUT_LIMIT = 4 * 1024 * 1024;

/** Where binaries live that a bare `PATH` lacks, for when the login shell can't be read. */
const USUAL_PATH = [
  "/opt/homebrew/bin",
  "/usr/local/bin",
  "/usr/bin",
  "/bin",
  "/usr/sbin",
  "/sbin",
];

/**
 * What a Claude Code session sets for the programs it runs. A host started from inside one
 * (a developer's terminal) would hand them on, and the CLI would take itself for that
 * session's child (T3 adapter pitfall 43); none of them is a setting of the person's.
 */
const INHERITED = [
  "CLAUDECODE",
  "CLAUDE_CODE_ENTRYPOINT",
  "CLAUDE_CODE_SESSION_ID",
  "CLAUDE_CODE_CHILD_SESSION",
  "CLAUDE_CODE_SESSION_ATTENDED",
  "CLAUDE_CODE_BRIDGE_SESSION_ID",
  "CLAUDE_CODE_MESSAGING_SOCKET",
  "CLAUDE_CODE_MESSAGING_TOKEN",
  "CLAUDE_CODE_EXECPATH",
  "CLAUDE_CODE_PLUGIN_DIRS",
  "CLAUDE_AGENT_SDK_VERSION",
  "CLAUDE_EFFORT",
  "CLAUDE_PID",
];

/**
 * The login shell's environment, as `$SHELL -l -c 'env -0'` prints it: a login shell, not an
 * interactive one, whose rc files would print, prompt or hang. Failing that, the host's own,
 * with the usual places on its PATH.
 */
export async function loginEnvironment(base: NodeJS.ProcessEnv = process.env): Promise<Env> {
  const shell = base["SHELL"] ?? "/bin/zsh";
  try {
    const printed = await output(shell, ["-l", "-c", "env -0"], { env: base, timeoutMs: LOGIN_MS });
    const env = parseEnv(printed);
    if (env["PATH"] === undefined) {
      throw new Error("The login shell's environment has no PATH.");
    }
    return withoutSession(env);
  } catch (error) {
    log.warn("The login shell's environment couldn't be read; using the host's", error);
    return withoutSession(fallback(base));
  }
}

/** The variables `env -0` printed: each `NAME=value`, ended by a NUL. */
export function parseEnv(printed: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const entry of printed.split("\0")) {
    const equals = entry.indexOf("=");
    if (equals <= 0) {
      continue;
    }
    // What an rc file printed before `env` ran sticks to the first entry, up to its last line.
    const name = entry.slice(0, equals).split("\n").at(-1) ?? "";
    if (/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
      env[name] = entry.slice(equals + 1);
    }
  }
  return env;
}

/** The host's environment, with the usual places added to its PATH. */
function fallback(base: NodeJS.ProcessEnv): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [name, value] of Object.entries(base)) {
    if (value !== undefined) {
      env[name] = value;
    }
  }
  env["PATH"] = [...new Set([...dirsOf(base["PATH"]), ...USUAL_PATH])].join(delimiter);
  return env;
}

/** The folders of a `PATH`, in order. */
function dirsOf(path: string | undefined): string[] {
  return (path ?? "").split(delimiter).filter((dir) => dir !== "");
}

function withoutSession(env: Record<string, string>): Env {
  const kept = { ...env };
  for (const name of INHERITED) {
    // A key is removed from a copy the host made itself, never from its own environment.
    Reflect.deleteProperty(kept, name);
  }
  return kept;
}

/** The person's home: the login environment's, which is the host's (HOME is never changed). */
function homeOf(env: Env): string {
  return env["HOME"] ?? homedir();
}

/**
 * Where `claude` may be, in the order it's looked for: Koyo's setting, the login PATH, then
 * where its installers put it.
 */
export function candidates(setting: string | null, env: Env): string[] {
  const home = homeOf(env);
  const onPath = dirsOf(env["PATH"]).map((dir) => join(dir, "claude"));
  return [...new Set([
    ...(setting === null ? [] : [setting]),
    ...onPath,
    join(home, ".local/bin/claude"),
    join(home, ".claude/local/claude"),
    "/opt/homebrew/bin/claude",
    "/usr/local/bin/claude",
  ])];
}

/** The first place `claude` is, as a file the person may run; `undefined` when it's nowhere. */
export function findClaude(setting: string | null, env: Env): string | undefined {
  const found = candidates(setting, env).find(isExecutable);
  if (setting !== null && found !== setting) {
    log.warn(`Koyo's Claude path ${setting} isn't a program; looking where installers put it.`);
  }
  return found;
}

function isExecutable(path: string): boolean {
  try {
    accessSync(path, constants.X_OK);
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

/** Each executable's version, read once (`claude --version`); a failed read is tried again. */
export class Versions {
  readonly #read = new Map<string, Promise<string | null>>();

  async of(path: string, env: Env): Promise<string | null> {
    const known = this.#read.get(path);
    if (known !== undefined) {
      return await known;
    }
    const reading = readVersion(path, env);
    this.#read.set(path, reading);
    const version = await reading;
    if (version === null) {
      this.#read.delete(path);
    }
    return version;
  }
}

async function readVersion(path: string, env: Env): Promise<string | null> {
  try {
    const printed = (await output(path, ["--version"], { env, timeoutMs: VERSION_MS })).trim();
    return /\d+\.\d+\.\d+\S*/u.exec(printed)?.[0] ?? (printed === "" ? null : printed);
  } catch (error) {
    log.warn(`Reading the version of ${path} failed`, error);
    return null;
  }
}

/**
 * What a program prints on stdout when it ends well, within `timeoutMs`. It runs in a process
 * group of its own, with nothing on stdin, so a timeout ends it and whatever it started.
 */
async function output(
  command: string,
  args: readonly string[],
  run: { readonly env: NodeJS.ProcessEnv | Env; readonly timeoutMs: number },
): Promise<string> {
  const child = spawn(command, args, {
    env: run.env,
    stdio: ["ignore", "pipe", "ignore"],
    detached: true,
  });
  const done = Promise.withResolvers<string>();
  const chunks: Buffer[] = [];
  let size = 0;
  const timer = setTimeout(() => {
    endGroup(child.pid);
    done.reject(new Error(`${command} took longer than ${String(run.timeoutMs)} ms.`));
  }, run.timeoutMs);
  child.stdout.on("data", (chunk: Buffer) => {
    size += chunk.length;
    if (size > OUTPUT_LIMIT) {
      endGroup(child.pid);
      done.reject(new Error(`${command} printed more than it should.`));
      return;
    }
    chunks.push(chunk);
  });
  child.once("error", (error) => {
    done.reject(error);
  });
  child.once("close", (code) => {
    if (code === 0) {
      done.resolve(Buffer.concat(chunks).toString("utf8"));
    } else {
      done.reject(new Error(`${command} exited with ${String(code)}.`));
    }
  });
  try {
    return await done.promise;
  } finally {
    clearTimeout(timer);
  }
}

function endGroup(pid: number | undefined): void {
  if (pid !== undefined) {
    signalGroup(pid, "SIGKILL");
  }
}
