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
  /** What it reported, but usage and limits. */
  readonly events: DriverEvent[];
  readonly usages: DriverEvent[];
  readonly limits: DriverEvent[];
  readonly send: (...all: SDKMessage[]) => void;
}

/** A turn's mapping alone; `compact` when it compacts the conversation. */
export function mapped(compact = false): Mapped {
  const events: DriverEvent[] = [];
  const usages: DriverEvent[] = [];
  const limits: DriverEvent[] = [];
  const frames = new Frames((event) => {
    if (event.type === "usage") {
      usages.push(event);
    } else if (event.type === "limits") {
      limits.push(event);
    } else {
      events.push(event);
    }
  }, 1000);
  frames.begin("turn-1", compact);
  return {
    frames,
    events,
    usages,
    limits,
    send: (...all) => {
      for (const frame of all) {
        frames.frame(frame);
      }
    },
  };
}
