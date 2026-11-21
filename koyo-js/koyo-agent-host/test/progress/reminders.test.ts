import { describe, expect, test } from "bun:test";

import { promptReminder, STALE_CALLS, staleReminder } from "../../src/progress/reminders";
import { type Board, declared } from "./board";

function finished(): Board {
  const board = declared();
  board.text({ steps: [{ id: "1.2", status: "done" }, { id: "2", status: "skipped" }] });
  return board;
}

describe("a message the person sends", () => {
  test("while the plan has work open tells the agent what runs and waits, and what to do", () => {
    expect(promptReminder(declared().plan)).toBe(
      "The chat's plan still has work open. Running: 1 \"Histogram endpoint in the explorer API\", "
      + "1.2 \"Endpoint handler and contract\". Pending: 2, 2.1. If this message continues that "
      + "task, update the steps it changes; if it starts a different task, replace the plan "
      + "(`replace: true`).",
    );
  });

  test("names what waits on the person", () => {
    const board = declared();
    board.text({ steps: [{ id: "1.2", status: "done" }, { id: "2.1", status: "blocked" }] });
    expect(promptReminder(board.plan)).toBe(
      "The chat's plan still has work open. Blocked: 2 \"Histogram panel in the CLI explorer\", "
      + "2.1 \"Panel layout and scaling\". If this message continues that task, update the steps "
      + "it changes; if it starts a different task, replace the plan (`replace: true`).",
    );
  });

  test("tells nothing without a plan, or with every phase closed", () => {
    expect(promptReminder(null)).toBeNull();
    expect(promptReminder(finished().plan)).toBeNull();
  });
});

describe("a stretch of tool calls without a plan update", () => {
  test("asks for one once it's thirty calls long, naming what runs", () => {
    const { plan } = declared();
    expect(staleReminder(plan, STALE_CALLS - 1)).toBeNull();
    expect(staleReminder(plan, STALE_CALLS)).toBe(
      "30 tool calls went by since the chat's plan last changed. Running: 1 \"Histogram endpoint "
      + "in the explorer API\", 1.2 \"Endpoint handler and contract\". Update the steps whose "
      + "state changed.",
    );
  });

  test("says so when nothing in the plan runs", () => {
    const board = declared();
    board.text({ steps: [{ id: "1.2", status: "blocked" }] });
    expect(staleReminder(board.plan, STALE_CALLS)).toContain("Nothing in it is running.");
  });

  test("with the plan done says it reads done while work goes on", () => {
    expect(staleReminder(finished().plan, STALE_CALLS)).toBe(
      "30 tool calls went by since the chat's plan last changed, and it reads done while the "
      + "work goes on: add this work to it as a step, or replace the plan (`replace: true`) if "
      + "it's a different task.",
    );
  });

  test("asks nothing without a plan", () => {
    expect(staleReminder(null, STALE_CALLS)).toBeNull();
  });
});
