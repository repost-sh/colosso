// Which frames are a turn's (docs/chat.md 4.7; T3 adapter pitfalls 6, 7, 8). The CLI starts
// turns of its own, when a background task ends or a scheduled wake fires, so the
// conversation's frames are credited to the person's turn only once the CLI says it took the
// turn's prompt: it echoes the prompt's uuid on the turn's first reply frames and its result
// (`user_message_uuid(s)`), and reports it started in a `command_lifecycle` frame. Until then
// the conversation's frames are held. A frame of the conversation while no turn runs is a turn
// the CLI started: it's adopted, and shows as any other, with no message of the person's to
// start it. A message steered into a turn that the CLI hadn't taken when the turn's result came
// runs as a turn of its own right after; it's still this turn's, which ends with the last of
// them. A subagent's frames are its own, whatever runs.

import type { SDKMessage, SDKResultMessage } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

import { log } from "../../log";
import type { Failure, Usage } from "../../protocol/model";
import { type DriverEvent, processFailure } from "../provider";
import type { Frames } from "./frames";
import { type Ending, summed } from "./results";

const NEVER_STARTED = "Claude acknowledged the message but never started a turn for it.";

/** A frame outside the SDK's union that says what became of a prompt. */
export const Lifecycle = z.object({
  type: z.literal("command_lifecycle"),
  command_uuid: z.string(),
  state: z.string(),
});
export type Lifecycle = z.infer<typeof Lifecycle>;

interface Turn {
  readonly promptId: string;
  /** Its prompt's id and its steers'. */
  readonly prompts: Set<string>;
  readonly steers: Set<string>;
  /** The prompts the CLI took into a turn, or cancelled. */
  readonly taken: Set<string>;
  readonly dropped: Set<string>;
  /** Whether its prompt went to the CLI. */
  sent: boolean;
  /** Whether the CLI said it took the prompt; frames are credited from then on. */
  echoed: boolean;
  /** Whether the CLI started it, with no prompt of ours. */
  readonly adopted: boolean;
  readonly held: SDKMessage[];
  interrupted: boolean;
  /** What its results used so far, when one came and a steer's turn is still to come. */
  usage: Usage | null;
  ending: Ending | null;
}

export class Turns {
  readonly #emit: (event: DriverEvent) => void;
  readonly #frames: Frames;
  readonly #finished: () => void;
  #turn: Turn | undefined;

  constructor(emit: (event: DriverEvent) => void, frames: Frames, finished: () => void) {
    this.#emit = emit;
    this.#frames = frames;
    this.#finished = finished;
  }

  /** Whether a turn runs. */
  get running(): boolean {
    return this.#turn !== undefined;
  }

  /** Whether `promptId`'s turn is the one running. */
  runs(promptId: string): boolean {
    return this.#turn?.promptId === promptId;
  }

  /** The ids of the running turn's prompts. */
  prompts(): ReadonlySet<string> {
    return this.#turn?.prompts ?? new Set();
  }

  /** Starts a turn of `promptId`; `compact` when it compacts the conversation. */
  start(promptId: string, compact: boolean): void {
    this.#begin(promptId, compact, false);
  }

  /** A turn the CLI started: its prompt, if it had one, is the CLI's, and it has gone. */
  #adopt(): Turn {
    const turn = this.#begin(crypto.randomUUID(), false, true);
    this.#emit({ type: "turn.adopted" });
    return turn;
  }

  #begin(promptId: string, compact: boolean, adopted: boolean): Turn {
    const turn: Turn = {
      promptId,
      prompts: new Set([promptId]),
      steers: new Set(),
      taken: new Set(),
      dropped: new Set(),
      sent: adopted,
      echoed: adopted,
      adopted,
      held: [],
      interrupted: false,
      usage: null,
      ending: null,
    };
    this.#turn = turn;
    this.#frames.begin(promptId, compact);
    return turn;
  }

  /** The turn's prompt went to the CLI. */
  sent(promptId: string): void {
    if (this.#turn?.promptId === promptId) {
      this.#turn.sent = true;
    }
  }

  /** Adds a message to the running turn; false when none runs or it's being stopped. */
  steer(promptId: string): boolean {
    const turn = this.#turn;
    if (turn === undefined || turn.interrupted) {
      return false;
    }
    turn.prompts.add(promptId);
    turn.steers.add(promptId);
    return true;
  }

  /** A steer that never reached the CLI. */
  unsteer(promptId: string): void {
    this.#turn?.steers.delete(promptId);
    this.#settleIfDone();
  }

  /**
   * The person stopped the turn: one whose prompt hasn't gone to the CLI ends now; one whose
   * prompt has waits for its result. Whether the CLI must be asked to stop it.
   */
  interrupt(): boolean {
    const turn = this.#turn;
    if (turn === undefined) {
      return false;
    }
    turn.interrupted = true;
    if (turn.sent) {
      return true;
    }
    this.#finish({ outcome: "interrupted", failure: null }, null);
    return false;
  }

  /** Ends the turn as failed before its prompt reached the CLI. */
  fail(promptId: string, failure: Failure): void {
    if (this.#turn?.promptId === promptId) {
      this.#finish({ outcome: "failed", failure }, null);
    }
  }

  /** The CLI asked for a permission, which only a turn it runs can: the turn is credited. */
  permission(): boolean {
    const turn = this.#turn;
    if (turn === undefined) {
      return false;
    }
    this.#echo();
    return true;
  }

  /** The process ended; the actor settles what ran (4.5.4). */
  abandon(): void {
    this.#turn = undefined;
  }

  /** What became of a prompt, as a `command_lifecycle` frame says. */
  lifecycle(lifecycle: Lifecycle): void {
    const turn = this.#turn;
    const id = lifecycle.command_uuid;
    if (turn?.prompts.has(id) !== true) {
      return;
    }
    switch (lifecycle.state) {
      case "started": {
        turn.taken.add(id);
        if (id === turn.promptId) {
          this.#echo();
        }
        break;
      }
      case "cancelled": {
        turn.dropped.add(id);
        if (id === turn.promptId && !turn.echoed) {
          this.#finish({ outcome: "interrupted", failure: null }, null);
        } else {
          this.#settleIfDone();
        }
        break;
      }
      case "completed": {
        // A prompt the CLI had already recorded completes without a turn (pitfall 7).
        if (id === turn.promptId && !turn.echoed) {
          this.#finish({ outcome: "failed", failure: processFailure(NEVER_STARTED) }, null);
        } else if (!turn.taken.has(id)) {
          turn.dropped.add(id);
          this.#settleIfDone();
        }
        break;
      }
      default: {
        break;
      }
    }
  }

  /** A frame of the conversation, credited to the running turn once the CLI took it. */
  frame(frame: SDKMessage): void {
    if (this.#frames.subagents.hears(frame) || this.#frames.background.hears(frame)) {
      return;
    }
    if (frame.type === "result") {
      this.#result(frame);
      return;
    }
    const turn = this.#turn ?? (gated(frame) ? this.#adopt() : undefined);
    if (turn === undefined) {
      return;
    }
    if (!turn.echoed && this.#echoes(turn, frame)) {
      this.#echo();
    } else if (!turn.echoed && gated(frame)) {
      turn.held.push(frame);
      return;
    }
    this.#frames.frame(frame);
  }

  #result(result: SDKResultMessage): void {
    const turn = this.#turn;
    if (turn === undefined) {
      log.debug("A turn no frame was seen of ended.");
      return;
    }
    const ours = this.#echoes(turn, result) || turn.adopted;
    const stamped = echoed(result).length > 0;
    const human = result.origin === undefined || result.origin.kind === "human";
    if (!ours && (stamped || !human)) {
      // Another turn's, one the CLI ran of its own just before: what it said still shows.
      for (const frame of turn.held.splice(0)) {
        this.#frames.frame(frame);
      }
      return;
    }
    // A CLI that echoes only on the result credits the turn with all it held.
    this.#echo();
    const { ending, usage: used } = this.#frames.result(result, turn.interrupted);
    const usage = summed(turn.usage, used);
    if (ending.outcome === "completed" && !turn.interrupted && waiting(turn)) {
      turn.usage = usage;
      turn.ending = ending;
      return;
    }
    this.#finish(ending, usage);
  }

  /** A turn whose last result came finishes once no steer of it is left to run. */
  #settleIfDone(): void {
    const turn = this.#turn;
    if (turn === undefined || turn.ending === null || waiting(turn)) {
      return;
    }
    this.#finish(turn.ending, turn.usage);
  }

  #finish(ending: Ending, usage: Usage | null): void {
    this.#frames.end(ending.outcome);
    this.#turn = undefined;
    this.#emit({ type: "turn.finished", outcome: ending.outcome, failure: ending.failure, usage });
    this.#finished();
  }

  /** The CLI took the turn's prompt: the turn starts, with what was held for it. */
  #echo(): void {
    const turn = this.#turn;
    if (turn === undefined || turn.echoed) {
      return;
    }
    turn.echoed = true;
    this.#emit({ type: "turn.started", promptId: turn.promptId });
    for (const frame of turn.held.splice(0)) {
      this.#frames.frame(frame);
    }
  }

  /** Whether `frame` says the CLI took one of the turn's prompts; notes the ones it took. */
  #echoes(turn: Turn, frame: SDKMessage): boolean {
    const ids = echoed(frame).filter((id) => turn.prompts.has(id));
    for (const id of ids) {
      turn.taken.add(id);
    }
    return ids.length > 0;
  }
}

/** The prompt uuids a frame says its turn answers, as the SDK's frames carry them. */
function echoed(frame: SDKMessage): readonly string[] {
  const all = "user_message_uuids" in frame ? frame.user_message_uuids : undefined;
  const one = "user_message_uuid" in frame ? frame.user_message_uuid : undefined;
  return all ?? (one === undefined ? [] : [one]);
}

/** Whether a frame is the conversation's own, which only a turn the CLI took can have. */
function gated(frame: SDKMessage): boolean {
  const { type } = frame;
  const reply = type === "assistant" || type === "stream_event" || type === "user";
  return reply && frame.parent_tool_use_id === null;
}

/** Whether a steer of `turn` is still to run. */
function waiting(turn: Turn): boolean {
  return [...turn.steers].some((id) => !turn.taken.has(id) && !turn.dropped.has(id));
}
