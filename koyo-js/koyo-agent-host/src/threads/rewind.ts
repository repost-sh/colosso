// Going back in a chat (docs/chat.md C11). What a person said then comes back to them, and that
// message and everything after it leave the chat and the conversation, which goes on from what
// came before in a session opened anew. The files stay as they are: what else works in the checkout
// may have changed them since. A removed run stays as a row, for a client that held it to hear
// it's gone. A prompt goes to the session by its run's id, which is how its entry is found.

import type { RunId } from "../protocol/ids";
import type { Prompt, Thread } from "../protocol/model";
import { HostError } from "../protocol/wire";
import type { Provider } from "../providers/provider";
import type { ThreadActor } from "./actor";
import { moved } from "./records";

/** Goes back to before run `runId`, unless something's at work on the chat. */
export async function rewind(
  actor: ThreadActor,
  provider: Provider,
  runId: RunId,
  at: string,
): Promise<Prompt> {
  const { state, thread } = actor;
  refuseWhileBusy(actor);
  const before = thread.sessionId === null
    ? null
    : await provider.entryBefore(thread.sessionId, thread.cwd, runId);
  // Something may have been sent while the conversation was read.
  refuseWhileBusy(actor);
  const runs = state.fromRun(runId);
  actor.closeSession();
  state.commit({
    thread: wentBack(state.thread, before, at),
    runs: runs.map((run) => moved(run, "removed")),
  });
  return runs[0].prompt;
}

function refuseWhileBusy(actor: ThreadActor): void {
  if (actor.thread.parent !== null) {
    throw new HostError("conflict", "A subagent's chat can't go back.");
  }
  if (actor.busy() || actor.state.next() !== undefined) {
    throw new HostError("conflict", "The chat is at work, or has messages waiting to go.");
  }
}

/** With nothing before it, the next turn starts a conversation of its own. */
function wentBack(thread: Thread, before: string | null, updatedAt: string): Thread {
  const sessionId = before === null ? null : thread.sessionId;
  return { ...thread, sessionId, resumeAt: before, updatedAt };
}
