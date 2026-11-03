// What a turn uses as it runs (docs/chat.md 4.7): each model message's tokens from its stream's
// `message_start`, updated by its `message_delta`, which counts the message so far, and the
// context the conversation holds after it: all the message read, cached or not, and what it
// wrote. The model's window comes from a result's `modelUsage`, less the reserve Claude Code
// keeps before it compacts (T3 adapter pitfall 38).

import type { SDKPartialAssistantMessage, SDKResultMessage } from "@anthropic-ai/claude-agent-sdk";

import type { Usage } from "../../protocol/model";
import { compactPoint } from "./results";

type StreamEvent = SDKPartialAssistantMessage["event"];
type StartUsage = Extract<StreamEvent, { type: "message_start" }>["message"]["usage"];
type DeltaUsage = Extract<StreamEvent, { type: "message_delta" }>["usage"];

/** What one model message used. */
interface Measured {
  input: number;
  cacheCreation: number;
  cacheRead: number;
  output: number;
}

export class Meter {
  /** The session's model, whose window the results tell. */
  model: string | null = null;
  #window: number | null = null;
  readonly #messages = new Map<string, Measured>();
  #message: string | null = null;
  #context: number | null = null;

  /** Forgets the turn before; the context and the window carry over. */
  begin(): void {
    this.#messages.clear();
    this.#message = null;
  }

  /** A message started, with what its request read. */
  start(id: string, usage: StartUsage): void {
    this.#message = id;
    this.#messages.set(id, {
      input: usage.input_tokens,
      cacheCreation: usage.cache_creation_input_tokens ?? 0,
      cacheRead: usage.cache_read_input_tokens ?? 0,
      output: usage.output_tokens,
    });
    this.#measured();
  }

  /** The message's counts so far; a count that's `null` keeps what its start said. */
  delta(usage: DeltaUsage): void {
    const measured = this.#current();
    if (measured === undefined) {
      return;
    }
    measured.input = usage.input_tokens ?? measured.input;
    measured.cacheCreation = usage.cache_creation_input_tokens ?? measured.cacheCreation;
    measured.cacheRead = usage.cache_read_input_tokens ?? measured.cacheRead;
    measured.output = usage.output_tokens;
    this.#measured();
  }

  /** The conversation was compacted down to `tokens`. */
  compacted(tokens: number): void {
    this.#context = tokens;
  }

  /** Learns the model's window from a result, which is where it's told. */
  learn(result: SDKResultMessage): void {
    this.#window = compactPoint(result.modelUsage, this.model) ?? this.#window;
  }

  /** The context as it stands, and the point at which Claude compacts it. */
  context(): { tokens: number | null; window: number | null } {
    return { tokens: this.#context, window: this.#window };
  }

  /** The turn's usage so far. */
  used(): Usage {
    const total: Measured = { input: 0, cacheCreation: 0, cacheRead: 0, output: 0 };
    for (const each of this.#messages.values()) {
      total.input += each.input;
      total.cacheCreation += each.cacheCreation;
      total.cacheRead += each.cacheRead;
      total.output += each.output;
    }
    return {
      inputTokens: total.input,
      outputTokens: total.output,
      cacheReadTokens: total.cacheRead,
      cacheCreationTokens: total.cacheCreation,
      costUsd: null,
      contextTokens: this.#context,
      contextWindow: this.#window,
    };
  }

  #current(): Measured | undefined {
    return this.#message === null ? undefined : this.#messages.get(this.#message);
  }

  #measured(): void {
    const measured = this.#current();
    if (measured !== undefined) {
      const { input, cacheCreation, cacheRead, output } = measured;
      this.#context = input + cacheCreation + cacheRead + output;
    }
  }
}
