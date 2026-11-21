import { describe, expect, test } from "bun:test";

import { after, Board, declared, MINUTE, PLAN, START } from "./board";

describe("a plan's phases", () => {
  test("hold their sub-steps, each phase counted by them, and the answer counts phases", () => {
    const board = new Board();
    const text = board.text({ steps: PLAN });
    expect(board.statuses()).toEqual([
      ["1", "running", [["1.1", "done"], ["1.2", "running"]]],
      ["2", "pending", [["2.1", "pending"]]],
    ]);
    expect(board.phase("1").detail).toBe("Serves bucketed counts for a query.");
    expect(board.step("1.2").detail).toBe("GET /histogram returns buckets.");
    expect(board.step("2.1").detail).toBeNull();
    expect(text.split("\n")[0]).toBe("0/2 phases done · 25%. The whole plan:");
  });

  test("start by themselves once one of their sub-steps does", () => {
    const board = declared();
    board.advance(MINUTE);
    board.text({ steps: [{ id: "2.1", status: "running" }] });
    expect(board.phase("2")).toMatchObject({ status: "running", startedAt: after(MINUTE) });
  });

  test("count a sub-step added to them at once", () => {
    const board = declared();
    const text = board.text({
      steps: [
        {
          id: "1.3",
          parent: "1",
          title: "QA cycle 1: histogram endpoint on staging",
          status: "running",
        },
        { id: "1.4", parent: "1", title: "Fix empty-bucket overflow found in QA cycle 1" },
      ],
    });
    expect(text).toContain("0/2 phases done · 13%.");
    expect(text).toContain(
      "[25% · sub-steps 1/4 done · done: 1.1 · running: 1.2, 1.3 · pending: 1.4]",
    );
  });

  test("grown on a plan under way have the answer say where follow-up work belongs", () => {
    const board = declared();
    const text = board.text({
      steps: [{ id: "3", title: "Fix the endpoint's QA findings", detail: "Fixes what QA found." }],
    });
    expect(text).toContain(
      "Added phase 3 \"Fix the endpoint's QA findings\" to a plan already under way.",
    );
    expect(text).toContain("sub-step (`parent`)");
  });

  test("are all in the answer, with their sub-steps by status, so the agent keeps them in view",
    () => {
      expect(new Board().text({ steps: PLAN })).toBe([
        "0/2 phases done · 25%. The whole plan:",
        "▶ 1 Histogram endpoint in the explorer API (running) "
        + "[50% · sub-steps 1/2 done · done: 1.1 · running: 1.2]",
        "· 2 Histogram panel in the CLI explorer (pending) "
        + "[0% · sub-steps 0/1 done · pending: 2.1]",
      ].join("\n"));
    });

  test("count an open phase's done sub-steps in full: one done and one at three of four is 88%",
    () => {
      const text = new Board().text({
        steps: [
          { id: "1", title: "Histogram endpoint in the explorer API", detail: "Serves buckets.",
            status: "done" },
          { id: "2", title: "Histogram panel in the CLI explorer", detail: "Draws buckets.",
            status: "running" },
          { id: "2.1", parent: "2", title: "Panel layout and scaling", status: "done" },
          { id: "2.2", parent: "2", title: "Bucket labels and colours", status: "done" },
          { id: "2.3", parent: "2", title: "Empty-state histogram copy", status: "done" },
          { id: "2.4", parent: "2", title: "QA the panel on /explorer", status: "running" },
        ],
      });
      expect(text).toContain("1/2 phases done · 88%.");
      expect(text).toContain("[75% · sub-steps 3/4 done");
    });
});

describe("a phase with sub-steps", () => {
  test("is done once each is done or skipped, and opens again when one is added", () => {
    const board = declared();
    board.advance(MINUTE);
    const closed = board.text({
      steps: [{ id: "1.2", status: "done" }, { id: "2.1", status: "skipped" }],
    });
    expect(board.phase("1")).toMatchObject({ status: "done", endedAt: after(MINUTE) });
    expect(board.phase("2").status).toBe("skipped");
    expect(closed.split("\n")[0]).toBe("1/1 phases done · 100%. The whole plan:");
    const opened = board.text({
      steps: [
        { id: "1.3", parent: "1", title: "Fix bucket overflow found in QA", status: "running" },
      ],
    });
    expect(board.phase("1")).toMatchObject({ status: "running", endedAt: null });
    expect(opened).toContain("[67% · sub-steps 2/3 done");
  });

  test("can't be marked done while one is open, and nothing of the call is kept", () => {
    const board = declared();
    const before = board.plan;
    board.advance(MINUTE);
    const call = { steps: [{ id: "2.1", status: "running" }, { id: "1", status: "done" }] };
    expect(board.refusal(call)).toBe(
      "Phase 1 has open sub-steps 1.2, and a phase with sub-steps is done once they all are: "
      + "mark each done or skipped, and the phase closes itself.",
    );
    expect(board.plan).toBe(before);
  });

  test("skipped skips its open sub-steps and keeps the done ones", () => {
    const board = declared();
    board.text({ steps: [{ id: "1", status: "skipped" }] });
    expect(board.statuses()[0]).toEqual(["1", "done", [["1.1", "done"], ["1.2", "skipped"]]]);
  });

  test("keeps the status they make it, and the answer says why", () => {
    const board = declared();
    expect(board.text({ steps: [{ id: "1", status: "pending" }] }))
      .toContain("Phase 1 is running, not pending: a phase with sub-steps takes its status from "
        + "them.");
    expect(board.phase("1").status).toBe("running");
  });

  test("stays a phase", () => {
    const board = declared();
    expect(board.refusal({ steps: [{ id: "2", parent: "1" }] }))
      .toBe("Step 2 has sub-steps of its own, so it stays a phase.");
  });
});

describe("a step's title", () => {
  test("of generic words alone is refused, saying what a title should name", () => {
    const board = declared();
    expect(board.refusal({ steps: [{ id: "1.3", parent: "1", title: "QA cycle 2" }] })).toBe(
      "Step 1.3's title \"QA cycle 2\" names a kind of work, not the work: say what it builds, "
      + "fixes or checks, as \"QA the empty-state histogram on /explorer\".",
    );
  });

  test("of a phase under three words is refused", () => {
    expect(new Board().refusal({
      steps: [{ id: "1", title: "Histogram API", detail: "Serves buckets." }],
    })).toContain("\"Histogram API\" is too short for a phase: use at least 3 words");
  });
});

describe("a new phase", () => {
  test("without a detail is refused", () => {
    const call = { steps: [{ id: "1", title: "Histogram endpoint in the explorer API" }] };
    expect(new Board().refusal(call))
      .toContain("Phase 1 \"Histogram endpoint in the explorer API\" is new and needs a `detail`");
  });
});

describe("a sub-step", () => {
  test("under a missing phase, or under another sub-step, is refused", () => {
    const board = declared();
    expect(board.refusal({ steps: [{ id: "9.1", parent: "9", title: "Orphan bucket check" }] }))
      .toContain("parent 9, which is not a phase of the plan");
    expect(board.refusal({ steps: [{ id: "1.2.1", parent: "1.2", title: "Handler unit tests" }] }))
      .toContain("parent 1.2, which is itself a sub-step of 1");
  });
});

describe("the plan's clock", () => {
  test("keeps its start when the first step goes back to pending", () => {
    const board = declared();
    board.advance(10 * MINUTE);
    board.text({ steps: [{ id: "1", status: "pending" }, { id: "2", status: "running" }] });
    expect(board.kept.startedAt).toBe(START);
  });
});
