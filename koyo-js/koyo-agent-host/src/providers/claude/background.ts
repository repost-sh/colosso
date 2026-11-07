// What Claude left running in the background (docs/chat.md 4.7): the shells and Monitors it
// started, and the subagents running past their turn, which it goes on from as they end. The CLI
// lists every live one each time the list changes, whatever turn runs by then; those it says
// aren't activity (`ambient`, as a watcher) don't count. The list is the process's: one that
// starts again has none until it says otherwise.

import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";

import type { DriverEvent } from "../provider";

export class Background {
  readonly #emit: (event: DriverEvent) => void;
  #tasks = 0;

  constructor(emit: (event: DriverEvent) => void) {
    this.#emit = emit;
  }

  /** Takes the CLI's list of what runs in the background, if `frame` is it. */
  hears(frame: SDKMessage): boolean {
    if (frame.type !== "system" || frame.subtype !== "background_tasks_changed") {
      return false;
    }
    this.#count(frame.tasks.filter((task) => task.ambient !== true).length);
    return true;
  }

  /** The process started again, with nothing in the background. */
  reset(): void {
    this.#count(0);
  }

  #count(tasks: number): void {
    if (tasks !== this.#tasks) {
      this.#tasks = tasks;
      this.#emit({ type: "background", tasks });
    }
  }
}
