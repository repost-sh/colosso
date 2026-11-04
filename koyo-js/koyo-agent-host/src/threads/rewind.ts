// Going back in a chat (docs/chat.md C11). What a person said then comes back to them, and that
// message and everything after it leave the chat and the conversation, which goes on from what
// came before in a session opened anew. A removed run stays as a row, for a client that held it
// to hear it's gone. A prompt goes to the session by its run's id, which is how its entry is
// found.
//
// The files stay as they are unless the person asks for them back, which is offered only while
// what the chat wrote is still what's there. Putting them back writes over whatever's there and
// its dry run can't tell, so the host looks: a file written since the chat last worked is someone
// else's work, and another chat at work in the checkout may be about to do the same.

import { statSync } from "node:fs";
import { basename, relative, resolve } from "node:path";

import type { RunId } from "../protocol/ids";
import type { FilesBack } from "../protocol/methods";
import type { Prompt, Thread } from "../protocol/model";
import { HostError } from "../protocol/wire";
import type { Provider } from "../providers/provider";
import type { ThreadActor } from "./actor";
import { moved } from "./records";

/** How a chat goes back. */
interface Going {
  /** Whether the files go back too. */
  readonly restoreFiles: boolean;
  /** Whether another chat is at work in the same checkout. */
  readonly othersAtWork: () => boolean;
  readonly now: () => string;
}

/**
 * Goes back to before run `runId`, unless something's at work on the chat, and nothing may start
 * while it does. The files go first, as Claude forgets what it has of them once the conversation
 * goes on from there, and if they fail the chat stays as it was.
 */
export async function rewind(
  actor: ThreadActor,
  provider: Provider,
  runId: RunId,
  { restoreFiles, othersAtWork, now }: Going,
): Promise<Prompt> {
  const { state, thread } = actor;
  refuseWhileBusy(actor);
  state.goingBack = true;
  try {
    const before = thread.sessionId === null
      ? null
      : await provider.entryBefore(thread.sessionId, thread.cwd, runId);
    const runs = state.fromRun(runId);
    actor.closeSession();
    if (restoreFiles) {
      await putFilesBack(actor, provider, runId, othersAtWork);
    }
    const at = now();
    state.commit({
      thread: {
        ...wentBack(state.thread, before, at),
        filesSettledAt: restoreFiles ? at : state.thread.filesSettledAt,
      },
      runs: runs.map((run) => moved(run, "removed")),
    });
    return runs[0].prompt;
  } finally {
    state.goingBack = false;
  }
}

/** What going back to before run `runId` could do to the files, changing none of them. */
export async function filesBack(
  actor: ThreadActor,
  provider: Provider,
  runId: RunId,
  othersAtWork: () => boolean,
): Promise<FilesBack | null> {
  const { sessionId, cwd } = actor.thread;
  refuseWhileBusy(actor);
  actor.state.fromRun(runId);
  if (sessionId === null) {
    return null;
  }
  const changes = await provider.filesAt(sessionId, cwd, runId);
  if (changes === null || changes.files.length === 0) {
    return null;
  }
  const files = changes.files.map((file) => relative(cwd, resolve(cwd, file)));
  const blockedBy = othersAtWork()
    ? "Another chat is at work in this checkout."
    : changedSince(cwd, files, actor.thread.filesSettledAt);
  return { ...changes, files, blocked: blockedBy };
}

/** Puts the files back as they were before run `runId`, if that's still safe. */
async function putFilesBack(
  actor: ThreadActor,
  provider: Provider,
  runId: RunId,
  othersAtWork: () => boolean,
): Promise<void> {
  const back = await filesBack(actor, provider, runId, othersAtWork);
  const { sessionId, cwd } = actor.thread;
  if (back === null || sessionId === null) {
    throw new HostError("conflict", "There are no files to put back.");
  }
  if (back.blocked !== null) {
    throw new HostError("conflict", back.blocked);
  }
  await provider.restoreFiles(sessionId, cwd, runId);
}

/**
 * What's written later than `since`, as a person reads it, by name as the line may be cut short.
 * Not knowing when takes everything for someone else's. A file that isn't there has nothing to
 * lose, whoever took it away.
 */
function changedSince(cwd: string, files: readonly string[], since: string | null): string | null {
  const settled = since === null ? 0 : Date.parse(since);
  const [first, ...more] = files.filter((file) => {
    const written = statSync(resolve(cwd, file), { throwIfNoEntry: false })?.mtimeMs ?? 0;
    // To the millisecond, as `since` is: a write in the moment before it isn't later than it.
    return Math.trunc(written) > settled;
  });
  if (first === undefined) {
    return null;
  }
  return more.length === 0
    ? `${basename(first)} changed since this chat last worked on it.`
    : `${basename(first)} and ${String(more.length)} more changed since this chat last worked on `
      + "them.";
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
