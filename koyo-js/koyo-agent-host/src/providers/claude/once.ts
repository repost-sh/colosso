// A CLI run for one answer (docs/chat.md 4.2): in a process group of its own, told nothing unless
// it's given a prompt, and gone once it's answered or the time is up.

import type {
  Options,
  SDKUserMessage,
  SpawnedProcess,
  SpawnOptions,
} from "@anthropic-ai/claude-agent-sdk";

import { Queue } from "../../queue";
import type { Conversation, Start } from "./session";
import { Groups } from "./spawn";

/** What a run is for, and what it's started with. */
export interface OneRun {
  /** What it's called in the log. */
  readonly label: string;
  /** How long it may take. */
  readonly ms: number;
  /** What it's told, if anything. */
  readonly prompt?: string | AsyncIterable<SDKUserMessage>;
  readonly options: (spawn: (options: SpawnOptions) => SpawnedProcess) => Options;
}

/** What `use` makes of a CLI started as `run` says. */
export async function once<T>(
  start: Start,
  run: OneRun,
  use: (conversation: Conversation) => Promise<T>,
): Promise<T> {
  const nothing = new Queue<SDKUserMessage>();
  const groups = new Groups(run.label);
  const conversation = start({
    prompt: run.prompt ?? nothing,
    options: run.options(groups.spawn),
  });
  const timer = Promise.withResolvers<never>();
  const timeout = setTimeout(() => {
    timer.reject(new Error(`Claude didn't answer within ${String(run.ms / 1000)} s.`));
  }, run.ms);
  try {
    return await Promise.race([use(conversation), timer.promise]);
  } finally {
    clearTimeout(timeout);
    nothing.end();
    conversation.close();
    await groups.terminate();
  }
}
