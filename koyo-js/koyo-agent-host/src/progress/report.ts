import type { Plan, PlanStep, StepStatus } from "../protocol/model";
import { percent, ratioOf, share, shareOf, STATUSES } from "./plan";

const GLYPH: Readonly<Record<StepStatus, string>> = {
  pending: "·",
  running: "▶",
  done: "✓",
  blocked: "!",
  skipped: "–",
};

/** The steps' ids by status, as `done: 1.1 · running: 1.2`. */
function byStatus(steps: readonly PlanStep[]): string {
  return STATUSES.flatMap((status) => {
    const ids = steps.filter((step) => step.status === status).map((step) => step.id);
    return ids.length === 0 ? [] : [`${status}: ${ids.join(", ")}`];
  }).join(" · ");
}

/**
 * The plan as the agent reads it back: how far it has come, the whole plan's phases end to end,
 * each with its sub-steps by status, then `notes` on how the call used the tool.
 */
export function report(plan: Plan | null, notes: readonly string[] = []): string {
  const head = plan === null
    ? "No plan."
    : `${ratioOf(plan.phases)} phases done · ${percent(share(plan))}. The whole plan:`;
  const phases = (plan?.phases ?? []).map((phase) => {
    const subs = phase.steps.length === 0
      ? ""
      : ` [${percent(shareOf(phase))} · sub-steps ${ratioOf(phase.steps)} done · `
        + `${byStatus(phase.steps)}]`;
    return `${GLYPH[phase.status]} ${phase.id} ${phase.title} (${phase.status})${subs}`;
  });
  return [head, ...phases, ...notes].join("\n");
}
