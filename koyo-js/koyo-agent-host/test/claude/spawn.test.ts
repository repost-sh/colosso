import type { SpawnOptions } from "@anthropic-ai/claude-agent-sdk";
import { describe, expect, test } from "bun:test";

import { Group } from "../../src/providers/claude/spawn";
import { until } from "../support/wait";

/** How long a group has to be gone once it's asked to end. */
const GONE_MS = 1000;

/** Starts `script` in a group of its own; gives the group and the pid it prints first. */
function started(script: string, signal = new AbortController().signal): {
  group: Group;
  printed: () => Promise<number>;
} {
  const options: SpawnOptions = {
    command: "/bin/sh",
    args: ["-c", script],
    cwd: process.cwd(),
    env: { PATH: "/usr/bin:/bin" },
    signal,
  };
  const said: string[] = [];
  const group = Group.start(options, (line) => {
    said.push(line);
  });
  const { stdout } = group.process;
  stdout.setEncoding("utf8");
  const pid = Promise.withResolvers<number>();
  let text = "";
  stdout.on("data", (chunk: string) => {
    text += chunk;
    if (text.includes("\n")) {
      pid.resolve(Number.parseInt(text, 10));
    }
  });
  return { group, printed: async () => await pid.promise };
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function gone(pid: number): Promise<boolean> {
  return await until(() => !alive(pid), GONE_MS);
}

describe("the CLI's process group", () => {
  test("ends whole on terminate, what the CLI started with it", async () => {
    const { group, printed } = started("sleep 30 & echo $!; wait");
    const child = await printed();
    expect(alive(child)).toBe(true);
    await group.terminate();
    expect(await gone(child)).toBe(true);
  });

  test("ends what the CLI left running when the CLI exits", async () => {
    const { printed } = started("sleep 30 & echo $!");
    const child = await printed();
    expect(await gone(child)).toBe(true);
  });

  test("is signalled whole by the SDK's kill", async () => {
    const { group, printed } = started("sleep 30 & echo $!; wait");
    const child = await printed();
    expect(group.process.kill("SIGTERM")).toBe(true);
    expect(await gone(child)).toBe(true);
  });

  test("ends when the SDK gives up on it", async () => {
    const stop = new AbortController();
    const { printed } = started("sleep 30 & echo $!; wait", stop.signal);
    const child = await printed();
    stop.abort();
    expect(await gone(child)).toBe(true);
  });
});
