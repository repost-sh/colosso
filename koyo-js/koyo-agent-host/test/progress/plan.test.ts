import { describe, expect, test } from "bun:test";

import { after, Board, MINUTE, START } from "./board";

const PLAN = [
  {
    id: "1",
    title: "Filing schema migration",
    detail: "Delivers the filing schema migration.",
    status: "done",
  },
  {
    id: "2",
    title: "Filing repository operations",
    detail: "Delivers the filing repository operations.",
    status: "running",
  },
  { id: "3", title: "Review the filings diff", detail: "Delivers the review the filings diff." },
];

describe("a call of the progress tool", () => {
  test("declares the phases in order, and answers with the whole plan, phase by phase", () => {
    const board = new Board();
    expect(board.text({ steps: PLAN })).toBe([
      "1/3 phases done · 33%. The whole plan:",
      "✓ 1 Filing schema migration (done)",
      "▶ 2 Filing repository operations (running)",
      "· 3 Review the filings diff (pending)",
    ].join("\n"));
    expect(board.statuses())
      .toEqual([["1", "done", []], ["2", "running", []], ["3", "pending", []]]);
  });

  test("later changes only the steps it names", () => {
    const board = new Board();
    board.text({ steps: PLAN });
    board.text({ steps: [{ id: "2", status: "done" }, { id: "3", status: "running" }] });
    expect(board.statuses()).toEqual([["1", "done", []], ["2", "done", []], ["3", "running", []]]);
    expect(board.phase("1").title).toBe("Filing schema migration");
  });

  test("with replace discards a finished plan", () => {
    const board = new Board();
    board.text({ steps: PLAN });
    board.text({ steps: [{ id: "2", status: "done" }, { id: "3", status: "skipped" }] });
    const text = board.text({
      replace: true,
      steps: [{ id: "1", title: "Exchange registry lookup", detail: "Delivers the lookup." }],
    });
    expect(board.statuses()).toEqual([["1", "pending", []]]);
    expect(board.phase("1").title).toBe("Exchange registry lookup");
    expect(text.split("\n"))
      .toEqual(["0/1 phases done · 0%. The whole plan:", "· 1 Exchange registry lookup (pending)"]);
  });

  test("keeps no plan while it has no phases, saying so", () => {
    const board = new Board();
    expect(board.text({ now: "reading the ledger code" })).toBe("No plan.");
    expect(board.plan).toBeNull();
    board.text({ steps: PLAN });
    board.text({ steps: [{ id: "2", status: "done" }, { id: "3", status: "skipped" }] });
    expect(board.text({ replace: true, steps: [] })).toBe("No plan.");
    expect(board.plan).toBeNull();
  });

  test("with a status outside the five is refused, naming the step and the five", () => {
    expect(new Board().refusal({ steps: [{ ...PLAN[0], status: "finished" }] }))
      .toBe("Step 1 has status \"finished\"; use pending, running, done, blocked or skipped.");
  });

  test("with a new step without a title is refused, and nothing of it is kept", () => {
    const board = new Board();
    board.text({ steps: PLAN });
    const before = board.plan;
    expect(board.refusal({ steps: [{ id: "2", status: "done" }, { id: "4", status: "running" }] }))
      .toBe("Step 4 is new and needs a title.");
    expect(board.plan).toBe(before);
  });

  test("with a step without an id is refused", () => {
    expect(new Board().refusal({ steps: [{ title: "Filing schema migration" }] }))
      .toBe("Every step needs an id.");
  });

  test("keeps what the agent says it's doing, until a call says otherwise or clears it", () => {
    const board = new Board();
    board.text({ steps: PLAN, now: "gating the worker report for 2.3" });
    board.text({ steps: [{ id: "2", status: "done" }] });
    expect(board.kept.now).toBe("gating the worker report for 2.3");
    board.text({ now: "" });
    expect(board.kept.now).toBeNull();
  });

  test("says when the plan last changed; a refused one doesn't", () => {
    const board = new Board();
    board.text({ steps: PLAN });
    expect(board.kept.updatedAt).toBe(START);
    board.advance(MINUTE);
    board.refusal({ steps: [{ id: "4", status: "running" }] });
    expect(board.kept.updatedAt).toBe(START);
    board.text({ steps: [{ id: "3", status: "running" }] });
    expect(board.kept.updatedAt).toBe(after(MINUTE));
  });
});

describe("a step's times", () => {
  const TWO = PLAN.slice(1).map((step, index) => ({
    ...step,
    id: String(index + 1),
    status: index === 0 ? "running" : "pending",
  }));

  test("say when it first ran and when it closed, and the plan's when its first step ran", () => {
    const board = new Board();
    board.text({ steps: TWO });
    board.advance(6 * MINUTE);
    board.text({ steps: [{ id: "1", status: "done" }, { id: "2", status: "running" }] });
    expect(board.step("1")).toMatchObject({ startedAt: START, endedAt: after(6 * MINUTE) });
    expect(board.step("2")).toMatchObject({ startedAt: after(6 * MINUTE), endedAt: null });
    expect(board.kept.startedAt).toBe(START);
  });

  test("are none before it starts", () => {
    const board = new Board();
    board.text({ steps: TWO });
    expect(board.step("2")).toMatchObject({ startedAt: null, endedAt: null });
  });

  test("close as each step does, one skipped unstarted with an end alone", () => {
    const board = new Board();
    board.text({ steps: TWO });
    board.advance(6 * MINUTE);
    board.text({ steps: [{ id: "1", status: "done" }, { id: "2", status: "skipped" }] });
    expect(board.step("1").endedAt).toBe(after(6 * MINUTE));
    expect(board.step("2")).toMatchObject({ startedAt: null, endedAt: after(6 * MINUTE) });
  });

  test("keep when it first ran through a wait and a reopening, and go back to pending with it",
    () => {
      const board = new Board();
      board.text({ steps: TWO });
      board.advance(MINUTE);
      board.text({ steps: [{ id: "1", status: "blocked" }] });
      board.text({ steps: [{ id: "1", status: "done" }] });
      board.advance(MINUTE);
      board.text({ steps: [{ id: "1", status: "running" }] });
      expect(board.step("1")).toMatchObject({ startedAt: START, endedAt: null });
      board.text({ steps: [{ id: "1", status: "pending" }] });
      expect(board.step("1")).toMatchObject({ startedAt: null, endedAt: null });
    });

  test("leave a skipped step out of the count and the share", () => {
    const text = new Board().text({
      steps: [...PLAN.slice(0, 2), {
        id: "3",
        title: "Legacy CSV export",
        detail: "Delivers the legacy csv export.",
        status: "skipped",
      }],
    });
    expect(text.split("\n")[0]).toBe("1/2 phases done · 50%. The whole plan:");
  });
});
