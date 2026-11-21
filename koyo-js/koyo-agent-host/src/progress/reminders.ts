import type { Plan, PlanStep, StepStatus } from "../protocol/model";
import { described, everyStep, isClosed } from "./plan";

/** How many of the agent's tool calls without a plan update earn it a reminder. */
export const STALE_CALLS = 30;

function withStatus(plan: Plan, status: StepStatus): PlanStep[] {
  return everyStep(plan).filter((step) => step.status === status);
}

function namedList(steps: readonly PlanStep[]): string {
  return steps.map(described).join(", ");
}

/**
 * What the agent is told as the person sends a message while the plan has work not closed: what
 * runs, waits and is to come, and to update the plan or replace it. Null with nothing open.
 */
export function promptReminder(plan: Plan | null): string | null {
  if (plan === null || plan.phases.every(isClosed)) {
    return null;
  }
  const running = withStatus(plan, "running");
  const blocked = withStatus(plan, "blocked");
  const pending = withStatus(plan, "pending");
  return [
    "The chat's plan still has work open.",
    ...(running.length === 0 ? [] : [`Running: ${namedList(running)}.`]),
    ...(blocked.length === 0 ? [] : [`Blocked: ${namedList(blocked)}.`]),
    ...(pending.length === 0 ? [] : [`Pending: ${pending.map((step) => step.id).join(", ")}.`]),
    "If this message continues that task, update the steps it changes; if it starts a different "
    + "task, replace the plan (`replace: true`).",
  ].join(" ");
}

/**
 * What the agent is told once `calls` of its tool calls went by since the plan last changed, at
 * `STALE_CALLS` or more: to update what changed, or, with the plan done while work goes on, to
 * add that work or replace the plan. Null before then, and without a plan.
 */
export function staleReminder(plan: Plan | null, calls: number): string | null {
  if (calls < STALE_CALLS || plan === null) {
    return null;
  }
  const since = `${String(calls)} tool calls went by since the chat's plan last changed`;
  if (plan.phases.every(isClosed)) {
    return `${since}, and it reads done while the work goes on: add this work to it as a step, `
      + "or replace the plan (`replace: true`) if it's a different task.";
  }
  const running = withStatus(plan, "running");
  const named = running.length === 0
    ? "Nothing in it is running."
    : `Running: ${namedList(running)}.`;
  return `${since}. ${named} Update the steps whose state changed.`;
}
