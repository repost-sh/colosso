// A chat's generated title (docs/chat.md 4.5.7).

import { log } from "../log";
import type { Provider } from "../providers/provider";
import { titleFrom } from "./records";
import type { ThreadState } from "./state";

/**
 * Asks the provider for a title and makes one of its reply, written only while the seed is still
 * the title, by what `step` runs.
 */
export function askForTitle(
  provider: Provider,
  state: ThreadState,
  prompt: string,
  step: (write: () => void) => void,
): void {
  provider.title(prompt, state.thread.cwd).then((reply) => {
    step(() => {
      const { thread } = state;
      const title = reply === undefined ? undefined : titleFrom(reply);
      if (title !== undefined && thread.titleSource === "seed") {
        state.commit({ thread: { ...thread, title, titleSource: "generated" } });
      }
    });
  }, (error: unknown) => {
    log.warn("A title didn't come", error);
  });
}
