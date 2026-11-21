import { everyStep } from "../../src/progress/plan";
import { updatePlan, type Used } from "../../src/progress/tool";
import type { Plan, PlanPhase, PlanStep } from "../../src/protocol/model";

export const START = "2026-11-03T18:00:00.000Z";
export const MINUTE = 60_000;

/** The time `ms` after `START`. */
export function after(ms: number): string {
  return new Date(Date.parse(START) + ms).toISOString();
}

/** A chat's plan as calls of the progress tool keep it, on a clock the test moves. */
export class Board {
  plan: Plan | null = null;
  #ms = 0;

  get at(): string {
    return after(this.#ms);
  }

  advance(ms: number): void {
    this.#ms += ms;
  }

  /** Calls the tool with `input` now: the plan changes unless the call is refused. */
  call(input: unknown): Used {
    const used = updatePlan(this.plan, input, this.at);
    if (!used.refused) {
      this.plan = used.plan;
    }
    return used;
  }

  /** The answer of a call that mustn't be refused. */
  text(input: unknown): string {
    const used = this.call(input);
    if (used.refused) {
      throw new Error(`The call was refused: ${used.text}`);
    }
    return used.text;
  }

  /** The text of a call that must be refused. */
  refusal(input: unknown): string {
    const used = this.call(input);
    if (!used.refused) {
      throw new Error("The call wasn't refused.");
    }
    return used.text;
  }

  get kept(): Plan {
    if (this.plan === null) {
      throw new Error("No plan is kept.");
    }
    return this.plan;
  }

  phase(id: string): PlanPhase {
    const phase = this.kept.phases.find((each) => each.id === id);
    if (phase === undefined) {
      throw new Error(`No phase ${id}.`);
    }
    return phase;
  }

  /** Step `id`, a phase or a sub-step. */
  step(id: string): PlanStep {
    const step = everyStep(this.kept).find((each) => each.id === id);
    if (step === undefined) {
      throw new Error(`No step ${id}.`);
    }
    return step;
  }

  /** Each phase's id and status, with its sub-steps'. */
  statuses(): [string, string, [string, string][]][] {
    return this.kept.phases.map((phase) => [
      phase.id,
      phase.status,
      phase.steps.map((step) => [step.id, step.status]),
    ]);
  }
}

/** A plan of two phases under way, each with sub-steps, as the tool's input. */
export const PLAN = [
  {
    id: "1",
    title: "Histogram endpoint in the explorer API",
    detail: "Serves bucketed counts for a query.",
    status: "running",
  },
  { id: "1.1", parent: "1", title: "Bucket query in ClickHouse", status: "done" },
  {
    id: "1.2",
    parent: "1",
    title: "Endpoint handler and contract",
    detail: "GET /histogram returns buckets.",
    status: "running",
  },
  {
    id: "2",
    title: "Histogram panel in the CLI explorer",
    detail: "Draws the buckets above the log list.",
  },
  { id: "2.1", parent: "2", title: "Panel layout and scaling" },
];

/** A board that has declared `PLAN`. */
export function declared(): Board {
  const board = new Board();
  board.text({ steps: PLAN });
  return board;
}
