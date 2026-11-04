import type { PermissionUpdate } from "@anthropic-ai/claude-agent-sdk";
import { describe, expect, test } from "bun:test";

import type { RequestPayload, Response } from "../../src/protocol/model";
import {
  answerTool,
  approval,
  approved,
  type Asking,
  PLAN_CAPTURED,
  type ToolRequest,
} from "../../src/providers/claude/approvals";
import type { DriverEvent } from "../../src/providers/provider";
import { QUESTIONS } from "../fixtures/claude/sessions";
import { toolRequest } from "./support";

const INPUT = { command: "bun test", description: "Run the tests" };

/** What the CLI suggested for the recorded Write: accept edits for the rest of the session. */
const SUGGESTED: PermissionUpdate[] = [
  { type: "setMode", mode: "acceptEdits", destination: "session" },
  {
    type: "addRules",
    rules: [{ toolName: "Bash", ruleContent: "bun test:*" }],
    behavior: "allow",
    destination: "localSettings",
  },
];

function request(change: Partial<ToolRequest> = {}): ToolRequest {
  return toolRequest({ suggestions: SUGGESTED, displayName: "Bash", ...change });
}

/** An approval the person gave. */
function decided(
  decision: "allow" | "allow_session" | "deny" | "cancel",
  message: string | null = null,
): Response {
  return { kind: "approval", decision, message };
}

/** A session's side of the requests, answering each with `answer`. */
class Person implements Asking {
  readonly asked: RequestPayload[] = [];
  readonly events: DriverEvent[] = [];
  readonly startedIds: string[] = [];
  readonly declinedIds: string[] = [];
  readonly #answer: (payload: RequestPayload, signal: AbortSignal) => Promise<Response>;

  constructor(answer: (payload: RequestPayload, signal: AbortSignal) => Promise<Response>) {
    this.#answer = answer;
  }

  readonly ask = async (payload: RequestPayload, signal: AbortSignal): Promise<Response> => {
    this.asked.push(payload);
    return await this.#answer(payload, signal);
  };

  readonly event = (event: DriverEvent): void => {
    this.events.push(event);
  };

  readonly started = (toolUseId: string): void => {
    this.startedIds.push(toolUseId);
  };

  readonly declined = (toolUseId: string): void => {
    this.declinedIds.push(toolUseId);
  };
}

/** The actor's side of a request nobody answers: refused once the CLI stops waiting. */
async function withdrawn(_payload: RequestPayload, signal: AbortSignal): Promise<Response> {
  const refused = Promise.withResolvers<Response>();
  const refuse = (): void => {
    refused.reject(new Error("The agent withdrew the request."));
  };
  if (signal.aborted) {
    refuse();
  }
  signal.addEventListener("abort", refuse, { once: true });
  return await refused.promise;
}

describe("an approval", () => {
  test("allowed runs the tool with its input as asked", () => {
    expect(approved(decided("allow"), INPUT, SUGGESTED)).toEqual({
      behavior: "allow",
      updatedInput: INPUT,
    });
  });

  test("allowed for the chat keeps the CLI's suggestions for this session only", () => {
    expect(approved(decided("allow_session"), INPUT, SUGGESTED)).toEqual({
      behavior: "allow",
      updatedInput: INPUT,
      updatedPermissions: [
        { type: "setMode", mode: "acceptEdits", destination: "session" },
        {
          type: "addRules",
          rules: [{ toolName: "Bash", ruleContent: "bun test:*" }],
          behavior: "allow",
          destination: "session",
        },
      ],
    });
  });

  test("denied tells Claude the person's words, or that they declined", () => {
    expect(approved(decided("deny", "Use the fake provider."), INPUT, [])).toEqual({
      behavior: "deny",
      message: "Use the fake provider.",
    });
    expect(approved(decided("deny", "  "), INPUT, [])).toEqual({
      behavior: "deny",
      message: "The user declined this tool call.",
    });
  });

  test("cancelled denies the tool and stops the turn", () => {
    expect(approved(decided("cancel"), INPUT, [])).toEqual({
      behavior: "deny",
      message: "The user stopped this turn.",
      interrupt: true,
    });
  });

  test("never answered is withdrawn", () => {
    expect(approved(null, INPUT, [])).toEqual({
      behavior: "deny",
      message: "The request was withdrawn.",
    });
  });

  test("shows the command, what Claude said of it, and what allowing it for the chat adds", () => {
    const reason = "\u001B[1mbun\u001B[0m needs approval";
    const shown = approval("Bash", INPUT, request({ decisionReason: reason }));
    expect(shown).toEqual({
      kind: "approval",
      tool: "Bash",
      family: "command",
      title: "Run a command",
      label: "bun test",
      detail: "bun test",
      change: null,
      input: INPUT,
      reason: "bun needs approval",
      allowSession: { label: "Allow every edit, Bash(bun test:*) for this chat" },
    });
  });

  test("offers nothing for the chat with no rule to keep, or when the CLI must ask", () => {
    expect(approval("Bash", INPUT, request({ suggestions: [] })))
      .toMatchObject({ allowSession: null });
    expect(approval("Bash", INPUT, request({ suppressAlwaysAllowRule: true })))
      .toMatchObject({ allowSession: null });
  });
});

describe("canUseTool", () => {
  test("asks the person, starting the call's item first, and answers as they did", async () => {
    const person = new Person(async () => await Promise.resolve(decided("allow_session")));
    const result = await answerTool(person, "Bash", INPUT, request());
    expect(person.startedIds).toEqual(["toolu_01HPrUmGikhvkmnhPGJdRvse"]);
    expect(person.asked.map((payload) => payload.kind)).toEqual(["approval"]);
    expect(result).toMatchObject({ behavior: "allow", updatedInput: INPUT });
  });

  test("a denied call reads as declined", async () => {
    const person = new Person(async () => await Promise.resolve(decided("deny")));
    await answerTool(person, "Bash", INPUT, request());
    expect(person.declinedIds).toEqual(["toolu_01HPrUmGikhvkmnhPGJdRvse"]);
  });

  test("is withdrawn when the CLI stops waiting", async () => {
    const stop = new AbortController();
    const person = new Person(withdrawn);
    const answer = answerTool(person, "Bash", INPUT, request({ signal: stop.signal }));
    stop.abort();
    expect(await answer).toEqual({ behavior: "deny", message: "The request was withdrawn." });
  });

  test("is withdrawn when the CLI stopped waiting before it was asked", async () => {
    const stop = new AbortController();
    stop.abort();
    const person = new Person(withdrawn);
    expect(await answerTool(person, "Bash", INPUT, request({ signal: stop.signal }))).toEqual({
      behavior: "deny",
      message: "The request was withdrawn.",
    });
  });

  test("AskUserQuestion is a question, answered by question text, choices joined", async () => {
    const asked = { questions: [...QUESTIONS.questions, {
      question: "Which editors?",
      header: "Editors",
      multiSelect: true,
      options: [{ label: "Zed", description: "" }, { label: "Vim", description: "" }],
    }] };
    const person = new Person(async () => await Promise.resolve({
      kind: "question",
      answers: { "Do you prefer tabs or spaces?": ["Tabs"], "Which editors?": ["Zed", "Vim"] },
    }));
    const result = await answerTool(person, "AskUserQuestion", asked, request());
    expect(person.asked[0]).toMatchObject({
      kind: "question",
      live: true,
      questions: [
        { id: "Do you prefer tabs or spaces?", header: "Indentation", multiSelect: false },
        { id: "Which editors?", multiSelect: true },
      ],
    });
    expect(result).toEqual({
      behavior: "allow",
      updatedInput: {
        questions: asked.questions,
        answers: { "Do you prefer tabs or spaces?": "Tabs", "Which editors?": "Zed, Vim" },
      },
    });
  });

  test("a question answered with nothing is skipped, which declines the tool", async () => {
    const person = new Person(async () => await Promise.resolve({ kind: "question", answers: {} }));
    expect(await answerTool(person, "AskUserQuestion", QUESTIONS, request())).toEqual({
      behavior: "deny",
      message: "The user skipped the question",
    });
  });

  test("ExitPlanMode is a plan, captured, and the tool denied so Claude stops at it", async () => {
    const person = new Person(async () => await Promise.reject(new Error("Not asked.")));
    const plan = { plan: "# Plan\n1. Read.\n2. Write." };
    const result = await answerTool(person, "ExitPlanMode", plan, request());
    expect(person.events).toEqual([{
      type: "request",
      request: { kind: "plan", markdown: "# Plan\n1. Read.\n2. Write." },
    }]);
    expect(person.asked).toEqual([]);
    expect(result).toEqual({ behavior: "deny", message: PLAN_CAPTURED });
  });

  test("an empty plan is no request: Claude is told to write it", async () => {
    const person = new Person(async () => await Promise.reject(new Error("Not asked.")));
    for (const input of [{ plan: " \n" }, {}]) {
      const result = await answerTool(person, "ExitPlanMode", input, request());
      expect(result).toEqual({
        behavior: "deny",
        message: "The plan came through empty. Write your plan, then call ExitPlanMode again.",
      });
    }
    expect(await answerTool(person, "ExitPlanMode", { plan: 3 }, request())).toEqual({
      behavior: "deny",
      message: "The plan couldn't be read.",
    });
    expect(person.events).toEqual([]);
  });
});
