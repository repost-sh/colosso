import { z } from "zod";

import { StepStatus } from "../protocol/model";

/** A step's id as the agent sends it: text, or a number, taken as its text. */
const Id = z.union([z.string(), z.number()]);

/**
 * The progress tool's input as `tool()` takes it, described as the agent reads it. It takes what
 * `parseUpdate` does: a number for an id, and null for a field left out.
 */
export const UPDATE_INPUT = {
  steps: z.array(z.object({
    id: Id.describe(
      "Short and stable across calls: \"2\" for a phase, \"2.1\", \"2.2\" for its sub-steps.",
    ),
    title: z.string().nullish().describe(
      "What the step delivers, specific enough to read cold: \"Histogram endpoint in the "
      + "explorer API\", \"QA the empty-state histogram on /explorer\". Never a kind of work "
      + "alone (\"Review\", \"QA cycle 2\", \"Fixes\"). Required when the step is new.",
    ),
    detail: z.string().nullish().describe(
      "One sentence on what the step covers and how you will know it is done. Required for a "
      + "new phase; worth giving a sub-step whose title cannot say it all.",
    ),
    parent: Id.nullish().describe(
      "The id of the phase this sub-step serves. Absent for a phase. One level only.",
    ),
    status: StepStatus.nullish().describe("Defaults to pending for a new step."),
  })).nullish().describe(
    "The steps to add or change, in plan order: a phase before its sub-steps.",
  ),
  replace: z.boolean().nullish().describe(
    "Discards the previous plan first: pass it only when the plan belongs to a new task. The new "
    + "plan's clock starts with its first step, and the answer names the open phases it dropped.",
  ),
  now: z.string().nullish().describe(
    "What you are doing right now, in a few words. An empty string clears it.",
  ),
};

/** What a call says of a step: null keeps what's held, as do `detail` and `parent` undefined. */
export interface Patch {
  readonly id: string;
  readonly title: string | null;
  /** null clears it. */
  readonly detail: string | null | undefined;
  /** null makes the step a phase. */
  readonly parent: string | null | undefined;
  readonly status: StepStatus | null;
}

/** A call of the tool, flat as the agent sends it. */
export interface Update {
  readonly steps: readonly Patch[];
  readonly replace: boolean;
  /** The line to keep: null clears it, undefined leaves the one held. */
  readonly now: string | null | undefined;
}

/** Why a call changed nothing, as the agent is told. */
export interface Refusal {
  readonly refusal: string;
}

const Fields = z.record(z.string(), z.unknown());
const Steps = z.array(z.unknown());
const Status = StepStatus.nullish();

function textOf(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

function idOf(value: unknown): string | null {
  return typeof value === "string" || typeof value === "number" ? textOf(String(value)) : null;
}

/** What a field says when it says anything: a null is a field left out, as JSON writes one. */
function given(fields: Readonly<Record<string, unknown>>, name: string): unknown {
  const value = fields[name];
  return value === null ? undefined : value;
}

function parseStep(input: unknown): Patch | Refusal {
  const fields = Fields.safeParse(input);
  const id = fields.success ? idOf(fields.data["id"]) : null;
  if (!fields.success || id === null) {
    return { refusal: "Every step needs an id." };
  }
  const status = Status.safeParse(fields.data["status"]);
  if (!status.success) {
    const named = JSON.stringify(fields.data["status"]);
    return {
      refusal: `Step ${id} has status ${named}; use pending, running, done, blocked or skipped.`,
    };
  }
  const detail = given(fields.data, "detail");
  const parent = given(fields.data, "parent");
  return {
    id,
    title: textOf(fields.data["title"]),
    detail: detail === undefined ? undefined : textOf(detail),
    parent: parent === undefined ? undefined : idOf(parent),
    status: status.data ?? null,
  };
}

/** Reads a call of the tool as the agent made it; a refusal names what to fix. */
export function parseUpdate(input: unknown): Update | Refusal {
  const fields = Fields.safeParse(input);
  const call = fields.success ? fields.data : {};
  const listed = Steps.safeParse(call["steps"]);
  const steps: Patch[] = [];
  for (const one of listed.success ? listed.data : []) {
    const step = parseStep(one);
    if ("refusal" in step) {
      return step;
    }
    steps.push(step);
  }
  const now = call["now"];
  return {
    steps,
    replace: call["replace"] === true,
    now: typeof now === "string" ? textOf(now) : undefined,
  };
}
