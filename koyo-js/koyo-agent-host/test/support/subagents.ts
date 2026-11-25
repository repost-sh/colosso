import type { Outcome, SubagentEvent, SubagentStart } from "../../src/providers/provider";

export const CALL = "toolu_agent";

const START: SubagentStart = {
  description: "Count files",
  agentType: "general-purpose",
  prompt: "Count the files.",
  model: "haiku",
  effort: "low",
};

export function started(key = CALL, parentKey: string | null = null): SubagentEvent {
  return { type: "subagent.started", key, parentKey, subagent: START };
}

export function ended(outcome: Outcome = "completed", key = CALL): SubagentEvent {
  return { type: "subagent.ended", key, outcome, summary: "One file.", usage: null };
}
