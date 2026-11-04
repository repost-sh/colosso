// Records as the host keeps them, for tests: whole and valid, with what a test cares about
// given and the rest plain.

import { ItemId, MessageId, newId, RequestId, RunId, ThreadId } from "../../src/protocol/ids";
import type { Params } from "../../src/protocol/methods";
import type {
  Item,
  Request,
  RequestPayload,
  Run,
  RunStatus,
  Thread,
} from "../../src/protocol/model";

const AT = "2026-11-03T18:00:00.000Z";

export function thread(): Thread {
  return {
    id: newId(ThreadId),
    cwd: "/Users/ana/code/ledger",
    title: "",
    titleSource: "seed",
    provider: "fake",
    model: null,
    effort: null,
    fast: false,
    permission: null,
    plan: false,
    sessionId: null,
    resumeAt: null,
    queueHeld: false,
    tabOpen: true,
    parent: null,
    createdAt: AT,
    updatedAt: AT,
    readAt: null,
    seq: 0,
  };
}

export function run(of: Thread, status: RunStatus = "completed"): Run {
  return {
    id: newId(RunId),
    threadId: of.id,
    status,
    prompt: { messageId: newId(MessageId), text: "Find the flaky test.", attachments: [] },
    model: null,
    effort: null,
    permission: null,
    plan: false,
    requestedAt: AT,
    startedAt: null,
    endedAt: null,
    stop: null,
    failure: null,
    usage: null,
    files: [],
    seq: 0,
  };
}

export type TextItem = Extract<Item, { kind: "assistant_text" }>;

export function text(of: Run, ordinal: number, words = ""): TextItem {
  return {
    id: newId(ItemId),
    threadId: of.threadId,
    runId: of.id,
    ordinal,
    status: "completed",
    seq: 0,
    kind: "assistant_text",
    payload: { text: words },
  };
}

/** What `threads.create` is asked for a chat in the ledger repository. */
export const CREATE: Params<"threads.create"> = {
  cwd: "/Users/ana/code/ledger",
  provider: "fake",
  model: null,
  effort: null,
  fast: false,
  permission: null,
  plan: false,
};

const PLAN: RequestPayload = { kind: "plan", markdown: "# Plan" };

/** A pending request of `of`'s, a plan unless it says otherwise. */
export function request(of: Run, payload = PLAN): Request {
  return {
    id: newId(RequestId),
    threadId: of.threadId,
    runId: of.id,
    status: "pending",
    payload,
    answer: null,
    createdAt: AT,
    resolvedAt: null,
    seq: 0,
  };
}
