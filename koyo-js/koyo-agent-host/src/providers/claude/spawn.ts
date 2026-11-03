// The CLI's process (docs/chat.md 4.2): started in a process group of its own, so that ending
// it ends what it started too, its MCP servers and its shells (T3 adapter pitfall 2). Closing
// sends the group SIGTERM, then SIGKILL after 5 s; the host's exit ends every group it
// started. The SDK talks to the process through its pipes and signals it through `kill`,
// which reaches the whole group.

import { type ChildProcessByStdio, spawn } from "node:child_process";
import type { Readable, Writable } from "node:stream";

import type { SpawnedProcess, SpawnOptions } from "@anthropic-ai/claude-agent-sdk";

import { log } from "../../log";

/** How long a group has to end on SIGTERM before it's killed. */
const KILL_MS = 5_000;

/** The groups the host started and that may still run, by their leader's pid. */
const live = new Set<number>();

// Every group still running when the host exits is killed, as the exit can't wait for them.
process.once("exit", () => {
  for (const pid of live) {
    signalGroup(pid, "SIGKILL");
  }
});

type Child = ChildProcessByStdio<Writable, Readable, Readable>;
type ExitListener = (code: number | null, signal: NodeJS.Signals | null) => void;
type ErrorListener = (error: Error) => void;

/** A CLI process in its own group, and the ways to end it. */
export class Group {
  readonly #child: Child;
  readonly #exited: Promise<void>;
  #ended = false;

  private constructor(child: Child) {
    this.#child = child;
    const exited = Promise.withResolvers<undefined>();
    this.#exited = exited.promise;
    child.once("exit", () => {
      this.#ended = true;
      // What the CLI left behind in its group ends with it.
      this.#signal("SIGTERM");
      this.#forget();
      exited.resolve(undefined);
    });
    child.once("error", (error) => {
      log.warn("Claude's process failed", error);
      this.#ended = true;
      this.#forget();
      exited.resolve(undefined);
    });
  }

  /**
   * Starts what `options` says in a group of its own; what it writes on stderr goes to
   * `stderr`, so its pipe never fills.
   */
  static start(options: SpawnOptions, stderr: (text: string) => void): Group {
    const child = spawn(options.command, options.args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
      detached: true,
    });
    const group = new Group(child);
    if (child.pid !== undefined) {
      live.add(child.pid);
    }
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (text: string) => {
      stderr(text);
    });
    // The SDK aborts the signal once it gave up on the process ending by itself.
    options.signal.addEventListener("abort", () => {
      group.terminate().catch((error: unknown) => {
        log.warn("Ending Claude's process failed", error);
      });
    }, { once: true });
    return group;
  }

  /** The process as the SDK drives it; its `kill` signals the whole group. */
  get process(): SpawnedProcess {
    const child = this.#child;
    const on = (event: "exit" | "error", listener: ExitListener | ErrorListener): void => {
      child.on(event, listener);
    };
    const once = (event: "exit" | "error", listener: ExitListener | ErrorListener): void => {
      child.once(event, listener);
    };
    const off = (event: "exit" | "error", listener: ExitListener | ErrorListener): void => {
      child.off(event, listener);
    };
    return {
      stdin: child.stdin,
      stdout: child.stdout,
      get killed() {
        return child.killed;
      },
      get exitCode() {
        return child.exitCode;
      },
      get signalCode() {
        return child.signalCode;
      },
      kill: (signal) => this.#signal(signal),
      on,
      once,
      off,
    };
  }

  /** Asks the group to end, and kills it if it hasn't after 5 s; resolves once it's gone. */
  async terminate(): Promise<void> {
    if (this.#ended) {
      return;
    }
    this.#signal("SIGTERM");
    const timer = setTimeout(() => {
      this.#signal("SIGKILL");
    }, KILL_MS);
    await this.#exited;
    clearTimeout(timer);
  }

  /** Ends the group now. */
  kill(): void {
    this.#signal("SIGKILL");
  }

  #signal(signal: NodeJS.Signals): boolean {
    const { pid } = this.#child;
    return pid === undefined ? this.#child.kill(signal) : signalGroup(pid, signal);
  }

  #forget(): void {
    const { pid } = this.#child;
    if (pid !== undefined) {
      live.delete(pid);
    }
  }
}

/**
 * The groups one query started, each a CLI process, ended together. What they write on
 * stderr is logged at debug as `label`'s, and handed to `heard` too.
 */
export class Groups {
  readonly #label: string;
  readonly #heard: ((text: string) => void) | undefined;
  readonly #groups: Group[] = [];

  constructor(label: string, heard?: (text: string) => void) {
    this.#label = label;
    this.#heard = heard;
  }

  /** Starts a process as the SDK asks, the query's `spawnClaudeCodeProcess`. */
  readonly spawn = (options: SpawnOptions): SpawnedProcess => {
    const group = Group.start(options, (text) => {
      log.debug(`${this.#label} printed: ${text.trim()}`);
      this.#heard?.(text);
    });
    this.#groups.push(group);
    return group.process;
  };

  /** Ends them all, as `Group.terminate` does each; resolves once they're gone. */
  async terminate(): Promise<void> {
    await Promise.all(this.#groups.map(async (group) => {
      await group.terminate();
    }));
  }

  /** Ends them all now. */
  kill(): void {
    for (const group of this.#groups) {
      group.kill();
    }
  }
}

/**
 * Sends `signal` to the process group led by `pid`, so it reaches what the leader started
 * too; false when nothing of the group is left.
 */
export function signalGroup(pid: number, signal: NodeJS.Signals): boolean {
  try {
    process.kill(-pid, signal);
    return true;
  } catch {
    return false;
  }
}
