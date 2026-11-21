import type { Plan, Time } from "../protocol/model";
import { apply } from "./apply";
import { parseUpdate } from "./input";
import { report } from "./report";

/**
 * What a call of the progress tool did: the plan it made, none while it has no phases, and its
 * answer, or why it was refused.
 */
export type Used =
  | { readonly refused: true; readonly text: string }
  | { readonly refused: false; readonly text: string; readonly plan: Plan | null };

/** A call of the progress tool, `input` as the agent made it, over `plan` at `at`. */
export function updatePlan(plan: Plan | null, input: unknown, at: Time): Used {
  const update = parseUpdate(input);
  const applied = "refusal" in update ? update : apply(plan, update, at);
  if ("refusal" in applied) {
    return { refused: true, text: applied.refusal };
  }
  const kept = applied.plan.phases.length === 0 ? null : applied.plan;
  return { refused: false, text: report(kept, applied.notes), plan: kept };
}

/**
 * What a call of the progress tool shows in the timeline, from its input: how many phases a call
 * declaring the plan gives it, else the steps it adds and the statuses it sets, else what the
 * agent says it's doing.
 */
export function planLabel(input: unknown): string {
  const update = parseUpdate(input);
  if ("refusal" in update) {
    return "Plan";
  }
  const phases = update.steps.filter((step) => {
    return step.title !== null && (step.parent ?? null) === null;
  }).length;
  if (update.replace || phases > 1) {
    return `Plan: ${String(phases)} ${phases === 1 ? "phase" : "phases"}`;
  }
  const changes = update.steps.flatMap((step) => {
    if (step.status !== null) {
      return [`${step.id} ${step.status}`];
    }
    return step.title === null ? [] : [`${step.id} added`];
  });
  if (changes.length > 0) {
    return `Plan: ${changes.join(", ")}`;
  }
  return typeof update.now === "string" ? `Plan: ${update.now}` : "Plan";
}
