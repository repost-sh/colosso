/** The MCP server a session keeps the chat's plan through, in the host, and its one tool. */
export const PROGRESS_SERVER = "koyo_progress";
export const PROGRESS_TOOL_NAME = "update";
/** The tool as the agent calls it. */
export const PROGRESS_TOOL = `mcp__${PROGRESS_SERVER}__${PROGRESS_TOOL_NAME}`;

/** What the agent is told of the progress tool when it's offered. */
export const PROGRESS_DESCRIPTION = [
  "Declares and updates the plan the person follows in the progress section of the chat's "
  + "details card: a few phases (the task's end-to-end milestones), each with sub-steps "
  + "(`parent`) for the work inside it.",
  "`steps` are upserted by `id`: the first call declares every phase with its `title` and "
  + "`detail`, and later calls send only the steps that are new or whose `status`, `title` or "
  + "`detail` changed.",
  "QA cycles, fixes, reviews and re-runs are sub-steps of the phase they serve, never new phases.",
  "`replace: true` starts the plan of a different task.",
  "`now` is one short line on what you are doing; an empty string clears it.",
  "Answers with the whole plan, phase by phase, and notes on how the call used it.",
  "Only the chat's main agent keeps the plan; a subagent never calls this tool and says what "
  + "changed in its report instead.",
].join(" ");

/** What Claude Code's system prompt is told of the plan while the progress tool is offered. */
export const PROGRESS_PROMPT = [
  "# Progress",
  "The person follows the work in the progress section of the chat's details card: a bar of how "
  + "far the task has come, then its phases, each with its sub-steps and details, all fed by the "
  + `\`${PROGRESS_TOOL}\` tool. It is their only view of the whole task end to end, so it must `
  + "read as steady forward motion through one plan.",
  "",
  "Shape: phases and sub-steps.",
  "- Before work of three or more steps, or any work you delegate to subagents, declare the whole "
  + "task in one call: every phase from start to finish (usually 3 to 7), each with a `title`, a "
  + "`detail` and a `status`, and the sub-steps you already know under each. Pass `replace: true` "
  + "only when the plan belongs to a new task.",
  "- A phase is an end-to-end milestone the person cares about (\"Histogram endpoint in the "
  + "explorer API\", \"Histogram panel in the CLI explorer\", \"Real-surface QA and release\"). A "
  + "sub-step (`parent` set to the phase's id, ids like 2.1, 2.2) is a unit of work inside it.",
  "- Work that appears along the way (a QA cycle, a fix for what it found, a review, a gate "
  + "re-run, a respawned worker) is a new sub-step of the phase it serves. Never a new phase, and "
  + "never a replace: a new phase is only for a milestone the plan truly lacked.",
  "",
  "Names: the person reads the card cold, mid-task.",
  "- Titles say what is built, fixed or checked and where: \"Fix histogram bucket overflow found "
  + "in QA cycle 1\", not \"Fixes\"; \"QA cycle 2: explorer histogram on staging\", not \"QA cycle "
  + "2\". A title of generic words alone is refused.",
  "- `detail` is one sentence: what the step covers and how you will know it is done.",
  "- Titles and details stay fixed once declared: what happened since goes into statuses, new "
  + "sub-steps and `now`, never into a rewritten title. A title rewritten to name other work is "
  + "refused: other work is a step with an id of its own.",
  "",
  "Updates: keep the whole plan in view.",
  "- The bar and each phase's share are computed from the plan: every phase counts the same, and "
  + "an open one by its done sub-steps over its sub-steps. Report state through statuses only; "
  + "there is no way to set progress directly.",
  "- Update a step the moment its state changes, sending only the steps that changed: `running` "
  + "when you start or dispatch it, `done` only once you have verified it, `blocked` while it "
  + "waits on the person, `skipped` when you drop it. A phase with sub-steps takes its status "
  + "from them: it runs once one starts and is done once all are done or skipped, so make its "
  + "final verification a sub-step of its own. Marking such a phase `skipped` skips its open "
  + "sub-steps; marking it `done` while one is open is refused.",
  "- Every update is to the one plan you declared, never a narrower plan for the slice in front "
  + "of you. The tool answers with the whole plan; read it before the next update.",
  "- Use `now` for one short line on what you are doing between steps, such as \"gating the "
  + "worker report for 2.3\".",
  "Only the chat's main agent keeps the plan: a subagent never calls the tool. Single-step and "
  + "conversational work needs no plan.",
].join("\n");
