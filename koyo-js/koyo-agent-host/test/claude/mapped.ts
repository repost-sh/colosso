// A turn's mapping on its own, without a session: the frames a test sends and the events they
// come to.

import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";

import { Frames } from "../../src/providers/claude/frames";
import type { DriverEvent } from "../../src/providers/provider";

/** The `events` of `type`, in order. */
export function ofType<T extends DriverEvent["type"]>(
  events: readonly DriverEvent[],
  type: T,
): Extract<DriverEvent, { type: T }>[] {
  return events.flatMap((event) => isType(event, type) ? [event] : []);
}

function isType<T extends DriverEvent["type"]>(
  event: DriverEvent,
  type: T,
): event is Extract<DriverEvent, { type: T }> {
  return event.type === type;
}

interface Mapped {
  readonly frames: Frames;
  /** What it reported, but usage. */
  readonly events: DriverEvent[];
  readonly usages: DriverEvent[];
  readonly send: (...all: SDKMessage[]) => void;
}

/** A turn's mapping alone; `compact` when it compacts the conversation. */
export function mapped(compact = false): Mapped {
  const events: DriverEvent[] = [];
  const usages: DriverEvent[] = [];
  const frames = new Frames((event) => {
    (event.type === "usage" ? usages : events).push(event);
  });
  frames.begin("turn-1", compact);
  return {
    frames,
    events,
    usages,
    send: (...all) => {
      for (const frame of all) {
        frames.frame(frame);
      }
    },
  };
}
