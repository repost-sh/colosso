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
