import type { Plan, PlanPhase, PlanStep, StepStatus } from "../protocol/model";

/** The statuses a step can have, in the order a report lists a phase's sub-steps by. */
export const STATUSES: readonly StepStatus[] = ["done", "running", "blocked", "pending", "skipped"];

/** Whether a step is over: done, or dropped. */
export function isClosed(step: Pick<PlanStep, "status">): boolean {
  return step.status === "done" || step.status === "skipped";
}

/** Whether a step is under way: running, or waiting on the person. */
export function isOpen(step: Pick<PlanStep, "status">): boolean {
  return step.status === "running" || step.status === "blocked";
}

/** The steps that count toward a share: a skipped one is no part of the total. */
function counted<S extends PlanStep>(steps: readonly S[]): S[] {
  return steps.filter((step) => step.status !== "skipped");
}

function doneOf(steps: readonly PlanStep[]): number {
  return steps.filter((step) => step.status === "done").length;
}

/** Done over the steps that count. */
export function ratioOf(steps: readonly PlanStep[]): string {
  return `${String(doneOf(steps))}/${String(counted(steps).length)}`;
}

/** A phase's share of done work: whole once done, else its done sub-steps over those that count. */
export function shareOf(phase: PlanPhase): number {
  if (phase.status === "done") {
    return 1;
  }
  const steps = counted(phase.steps);
  return steps.length === 0 ? 0 : doneOf(steps) / steps.length;
}

/** The task's done share: every phase that counts weighs the same; none while none does. */
export function share(plan: Plan): number {
  const phases = counted(plan.phases);
  const sum = phases.map(shareOf).reduce((total, each) => total + each, 0);
  return phases.length === 0 ? 0 : sum / phases.length;
}

export function percent(value: number): string {
  return `${String(Math.round(value * 100))}%`;
}

/** A step as the agent is told of it: its id and its title. */
export function described(step: PlanStep): string {
  return `${step.id} "${step.title}"`;
}

/** Every step of the plan, each phase before its sub-steps. */
export function everyStep(plan: Plan): PlanStep[] {
  return plan.phases.flatMap((phase) => [phase, ...phase.steps]);
}
