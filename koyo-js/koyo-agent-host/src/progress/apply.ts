import type { Plan, PlanPhase, PlanStep, StepStatus, Time } from "../protocol/model";
import type { Patch, Refusal, Update } from "./input";
import { described, isClosed, isOpen } from "./plan";
import { titleRefusal } from "./titles";

/** A plan the call applied to, with what the agent should hear about how it used the tool. */
export interface Applied {
  readonly plan: Plan;
  readonly notes: readonly string[];
}

/** A step of the plan as a call works on it: flat, a sub-step naming its phase. */
type Flat = PlanStep & { readonly parent: string | null };

function flatten(plan: Plan): Flat[] {
  return plan.phases.flatMap(({ steps, ...phase }) => [
    { ...phase, parent: null },
    ...steps.map((step) => ({ ...step, parent: phase.id })),
  ]);
}

function plain({ parent: _parent, ...step }: Flat): PlanStep {
  return step;
}

function childrenOf(steps: readonly Flat[], id: string): Flat[] {
  return steps.filter((step) => step.parent === id);
}

function phaseOf(steps: readonly Flat[], id: string): Flat | undefined {
  return steps.find((step) => step.id === id && step.parent === null);
}

/** The plan's phases, each with its sub-steps in the order they came. */
function nest(steps: readonly Flat[]): PlanPhase[] {
  return steps.filter((step) => step.parent === null).map((phase) => ({
    ...plain(phase),
    steps: childrenOf(steps, phase.id).map(plain),
  }));
}

/** Sets a step's status and the times it implies: when it first ran, when it closed. */
function stamp(step: Flat, status: StepStatus, at: Time): Flat {
  if (status === step.status) {
    return step;
  }
  switch (status) {
    case "running": {
      return { ...step, status, startedAt: step.startedAt ?? at, endedAt: null };
    }
    case "blocked": {
      return { ...step, status, endedAt: null };
    }
    case "pending": {
      return { ...step, status, startedAt: null, endedAt: null };
    }
    case "done":
    case "skipped": {
      return { ...step, status, endedAt: at };
    }
  }
}

/** Where a sub-step may sit: under a phase the plan holds, never under a sub-step or itself. */
function parentRefusal(steps: readonly Flat[], id: string, parent: string): Refusal | null {
  const phase = steps.find((step) => step.id === parent);
  if (parent === id || phase === undefined) {
    return {
      refusal: `Step ${id} names parent ${parent}, which is not a phase of the plan; declare the `
        + "phase first, or in the same call before it.",
    };
  }
  if (phase.parent !== null) {
    return {
      refusal: `Step ${id} names parent ${parent}, which is itself a sub-step of ${phase.parent}; `
        + "sub-steps sit under a phase, one level deep.",
    };
  }
  return childrenOf(steps, id).length > 0
    ? { refusal: `Step ${id} has sub-steps of its own, so it stays a phase.` }
    : null;
}

/** The step `patch` makes of the one the plan holds by its id, if any, or why it's refused. */
function patched(steps: readonly Flat[], patch: Patch, at: Time): Flat | Refusal {
  const held = steps.find((step) => step.id === patch.id);
  const parent = patch.parent === undefined ? (held?.parent ?? null) : patch.parent;
  const moved = parent !== null && parent !== held?.parent;
  const misplaced = moved ? parentRefusal(steps, patch.id, parent) : null;
  if (misplaced !== null) {
    return misplaced;
  }
  const title = patch.title ?? held?.title ?? null;
  if (title === null) {
    return { refusal: `Step ${patch.id} is new and needs a title.` };
  }
  const named = patch.title === null
    ? null
    : titleRefusal(patch.id, held?.title, title, parent === null);
  if (named !== null) {
    return { refusal: named };
  }
  const detail = patch.detail === undefined ? (held?.detail ?? null) : patch.detail;
  if (held === undefined && parent === null && detail === null) {
    return {
      refusal: `Phase ${patch.id} "${title}" is new and needs a \`detail\`: one sentence on what `
        + "it delivers and how you will know it is done.",
    };
  }
  const base: Flat = held ?? {
    id: patch.id, title, detail, parent, status: "pending", startedAt: null, endedAt: null,
  };
  return stamp({ ...base, title, detail, parent }, patch.status ?? base.status, at);
}

/**
 * A phase with sub-steps closes once they all have: `done` on one with open sub-steps would
 * claim work no sub-step did, and `skipped` drops them with it.
 */
function closed(given: readonly Flat[], patches: readonly Patch[], at: Time): Flat[] | Refusal {
  let steps = [...given];
  for (const patch of patches) {
    const open = phaseOf(steps, patch.id) === undefined
      ? []
      : childrenOf(steps, patch.id).filter((step) => !isClosed(step));
    if (open.length > 0 && patch.status === "done") {
      const ids = open.map((step) => step.id).join(", ");
      return {
        refusal: `Phase ${patch.id} has open sub-steps ${ids}, and a phase with sub-steps is done `
          + "once they all are: mark each done or skipped, and the phase closes itself.",
      };
    }
    if (open.length > 0 && patch.status === "skipped") {
      steps = steps.map((step) => (open.includes(step) ? stamp(step, "skipped", at) : step));
    }
  }
  return steps;
}

/** A phase's status as its sub-steps make it, or its own while it has none or none started. */
function derive(phase: Flat, children: readonly Flat[]): StepStatus {
  const has = (status: StepStatus): boolean => children.some((child) => child.status === status);
  if (children.length === 0) {
    return phase.status;
  }
  if (children.every((child) => child.status === "skipped")) {
    return "skipped";
  }
  if (children.every(isClosed)) {
    return "done";
  }
  if (has("running")) {
    return "running";
  }
  if (has("blocked")) {
    return "blocked";
  }
  if (has("done")) {
    return "running";
  }
  return isOpen(phase) ? phase.status : "pending";
}

/** What the sub-steps make of their phases. */
function settle(steps: readonly Flat[], at: Time): Flat[] {
  return steps.map((step) => {
    return step.parent === null ? stamp(step, derive(step, childrenOf(steps, step.id)), at) : step;
  });
}

/** When the plan's first step started, if one has. */
function firstStart(steps: readonly Flat[]): Time | null {
  const starts = steps.flatMap((step) => (step.startedAt === null ? [] : [step.startedAt]));
  return starts.toSorted().at(0) ?? null;
}

/** What a replace dropped: the old plan's phases not yet closed that the new plan doesn't name. */
function droppedNote(old: Plan, steps: readonly Flat[]): string[] {
  const kept = new Set(steps.map((step) => step.title));
  const dropped = old.phases.filter((phase) => !isClosed(phase) && !kept.has(phase.title));
  if (dropped.length === 0) {
    return [];
  }
  const named = dropped.map((phase) => `${described(phase)} (${phase.status})`).join(", ");
  return [
    `The plan this replaced still had open phases: ${named}. If this is the same task, they `
    + "belong in the new plan: add them back with their status.",
  ];
}

function grownNote(phase: Flat): string {
  return `Added phase ${described(phase)} to a plan already under way. Follow-up work on an `
    + "existing phase (a QA cycle, a fix, a review, a re-run) belongs under that phase as a "
    + "sub-step (`parent`), so the phases stay the task's end-to-end milestones.";
}

/** What a patch's status for a phase became, when its sub-steps decided otherwise. */
function derivedNotes(steps: readonly Flat[], patches: readonly Patch[]): string[] {
  return patches.flatMap((patch) => {
    const phase = phaseOf(steps, patch.id);
    const overruled = phase !== undefined && patch.status !== null
      && patch.status !== phase.status && childrenOf(steps, phase.id).length > 0;
    return overruled
      ? [`Phase ${phase.id} is ${phase.status}, not ${patch.status}: a phase with sub-steps `
        + "takes its status from them."]
      : [];
  });
}

/**
 * Upserts the call's steps by id, over an empty plan when it replaces, at `at`; a refusal leaves
 * the plan as it was. A replaced plan's clock is the new plan's own.
 */
export function apply(plan: Plan | null, update: Update, at: Time): Applied | Refusal {
  const old = plan ?? { phases: [], now: null, startedAt: null, updatedAt: at };
  const steps = update.replace ? [] : flatten(old);
  const hadStarted = steps.some((step) => step.startedAt !== null || isClosed(step));
  const added: Flat[] = [];
  for (const patch of update.steps) {
    const step = patched(steps, patch, at);
    if ("refusal" in step) {
      return step;
    }
    const index = steps.findIndex((each) => each.id === patch.id);
    if (index === -1) {
      steps.push(step);
      added.push(step);
    } else {
      steps[index] = step;
    }
  }
  const closing = closed(steps, update.steps, at);
  if ("refusal" in closing) {
    return closing;
  }
  const settled = settle(closing, at);
  const grown = hadStarted ? added.filter((step) => step.parent === null) : [];
  const notes = [
    ...(update.replace ? droppedNote(old, settled) : []),
    ...grown.map(grownNote),
    ...derivedNotes(settled, update.steps),
  ];
  const startedAt = (update.replace ? null : old.startedAt) ?? firstStart(settled);
  const now = update.now === undefined ? old.now : update.now;
  return { plan: { phases: nest(settled), now, startedAt, updatedAt: at }, notes };
}
