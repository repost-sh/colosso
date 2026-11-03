import { randomUUID } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

import type { SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";
import { afterEach, describe, expect, test } from "bun:test";

import { userMessage } from "../../src/providers/claude/prompts";
import { Queue } from "../../src/queue";
import { removeScratch, scratch } from "../support/scratch";
import { turnOf } from "./support";

afterEach(removeScratch);

describe("a prompt", () => {
  test("carries its pictures as images before its text, other files named after it", async () => {
    const dir = scratch();
    const picture = join(dir, "shot.png");
    const second = join(dir, "second.jpg");
    writeFileSync(picture, Buffer.from([0x89, 0x50, 0x4E, 0x47]));
    writeFileSync(second, Buffer.from([0xFF, 0xD8]));
    const promptId = randomUUID();
    const sent = await userMessage({
      ...turnOf("What's wrong here?", promptId),
      attachments: [
        { path: picture, mime: "image/png" },
        { path: join(dir, "log.txt"), mime: "text/plain" },
        { path: second, mime: "image/jpeg" },
      ],
    });
    expect(sent).toEqual({
      type: "user",
      message: {
        role: "user",
        content: [
          { type: "image", source: { type: "base64", media_type: "image/png", data: "iVBORw==" } },
          { type: "image", source: { type: "base64", media_type: "image/jpeg", data: "/9g=" } },
          { type: "text", text: `What's wrong here?\n[Attached file: ${join(dir, "log.txt")}]` },
        ],
      },
      parent_tool_use_id: null,
      uuid: promptId,
    });
  });

  test("joining a running turn lands at its next step", async () => {
    const sent = await userMessage(turnOf("Also this."), "next");
    expect(sent.priority).toBe("next");
  });
});

describe("the prompt queue", () => {
  test("gives the CLI its prompts in order, and ends when told", async () => {
    const queue = new Queue<SDKUserMessage>();
    const read: string[] = [];
    const reading = (async (): Promise<void> => {
      for await (const prompt of queue) {
        read.push(prompt.uuid ?? "");
      }
    })();
    const one = await userMessage(turnOf("One."));
    const two = await userMessage(turnOf("Two."));
    queue.push(one);
    queue.push(two);
    queue.end();
    queue.push(one);
    await reading;
    expect(read).toEqual([one, two].map((prompt: SDKUserMessage) => prompt.uuid ?? ""));
  });
});
