import { describe, expect, test } from "bun:test";

import { after, Board, declared, MINUTE, PLAN } from "./board";

const OTHER = {
  id: "1",
  title: "Exchange registry lookup service",
  detail: "A different task.",
};

describe("a replace", () => {
  test("is never refused, and names the open phases it dropped and where they belong", () => {
    const board = declared();
    const text = board.text({ replace: true, steps: [OTHER] });
    expect(board.statuses()).toEqual([["1", "pending", []]]);
    expect(text.split("\n").at(-1)).toBe(
      "The plan this replaced still had open phases: 1 \"Histogram endpoint in the explorer API\" "
      + "(running), 2 \"Histogram panel in the CLI explorer\" (pending). If this is the same task, "
      + "they belong in the new plan: add them back with their status.",
    );
  });

  test("that carries the open phases over, or of a plan all closed, says nothing of them", () => {
    const board = declared();
    const carried = board.text({ replace: true, steps: PLAN });
    expect(carried).not.toContain("still had open phases");
    board.text({ steps: [{ id: "1.2", status: "done" }, { id: "2", status: "skipped" }] });
    expect(board.text({ replace: true, steps: [OTHER] })).not.toContain("still had open phases");
  });

  test("starts the new plan's clock with its first step, whatever the old one's phases were",
    () => {
      const board = declared();
      board.advance(4 * 60 * MINUTE);
      board.text({ steps: [{ id: "1", status: "skipped" }, { id: "2", status: "skipped" }] });
      board.text({ replace: true, steps: [OTHER] });
      expect(board.kept.startedAt).toBeNull();
      board.advance(MINUTE);
      board.text({ steps: [{ id: "1", status: "running" }] });
      expect(board.kept.startedAt).toBe(after(4 * 60 * MINUTE + MINUTE));
    });
});

describe("a title changed on a step", () => {
  test("to name other work is refused, saying what the step is and what to do instead", () => {
    const board = declared();
    const before = board.plan;
    expect(board.refusal({ steps: [{ id: "1.2", title: "Write the release notes for 4.2" }] }))
      .toBe("Step 1.2 is \"Endpoint handler and contract\", and \"Write the release notes for "
        + "4.2\" reads as different work: give that work a new id, and set step 1.2's status as it "
        + "stands.");
    expect(board.plan).toBe(before);
  });

  test("keeps a fifth of the old title's significant words to be the same work", () => {
    const board = new Board();
    board.text({
      steps: [
        {
          id: "1",
          title: "Cache warmed histogram buckets across regional explorer replicas",
          detail: "Warms the caches.",
        },
        { id: "2", title: "Histogram bucket overflow in regional replicas", detail: "No more." },
      ],
    });
    expect(board.refusal({ steps: [{ id: "1", title: "Regional failover drill for Postgres" }] }))
      .toContain("reads as different work");
    board.text({ steps: [{ id: "2", title: "Histogram docs for the website" }] });
    expect(board.phase("2").title).toBe("Histogram docs for the website");
  });

  test("reworded for the same work is kept", () => {
    const board = declared();
    board.text({ steps: [{ id: "1.2", title: "Endpoint handler and its JSON contract" }] });
    expect(board.step("1.2").title).toBe("Endpoint handler and its JSON contract");
  });
});

describe("what the agent sends", () => {
  test("as a number for an id is taken as its text", () => {
    const board = new Board();
    board.text({
      steps: [
        { id: 1, title: "Histogram endpoint in the explorer API", detail: "Serves buckets." },
        { id: 1.1, parent: 1, title: "Bucket query in ClickHouse", status: "running" },
      ],
    });
    expect(board.statuses()).toEqual([["1", "running", [["1.1", "running"]]]]);
    board.text({ steps: [{ id: 1.1, status: "done" }] });
    expect(board.step("1.1").status).toBe("done");
  });

  test("as null for a field is that field left out", () => {
    const board = declared();
    board.text({
      steps: [{ id: "1.2", title: null, detail: null, parent: null, status: "done" }],
      replace: null,
      now: null,
    });
    expect(board.statuses()[0]).toEqual(["1", "done", [["1.1", "done"], ["1.2", "done"]]]);
    expect(board.step("1.2").title).toBe("Endpoint handler and contract");
  });

  test("as an empty parent makes a sub-step a phase", () => {
    const board = declared();
    board.text({ steps: [{ id: "1.1", parent: "" }] });
    expect(board.statuses().map(([id]) => id)).toEqual(["1", "1.1", "2"]);
  });
});
