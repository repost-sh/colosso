// What a session tells its CLI (docs/chat.md 4.7): one queue of prompts for the life of the
// process (a `Queue`), which the SDK reads as its input; ended, it has the CLI finish what it
// has and exit. Each prompt carries the uuid its frames are credited by. Pictures go with a
// prompt as images; any other file is named by its path, so the agent can read it.

import type { SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";

import type { TurnInput } from "../provider";

/** A block of a prompt's content. */
type Block = Exclude<SDKUserMessage["message"]["content"], string>[number];
type ImageType = "image/png" | "image/jpeg" | "image/gif" | "image/webp";

/**
 * `turn` as the CLI takes it: its text after its pictures, the other files named below it,
 * with its id as the message's uuid and, for a message joining a running turn, the priority
 * that lands it at the turn's next step (4.5.2).
 */
export async function userMessage(turn: TurnInput, priority?: "next"): Promise<SDKUserMessage> {
  const pictures = turn.attachments.flatMap((attachment) => {
    const type = imageType(attachment.mime);
    return type === undefined ? [] : [{ path: attachment.path, type }];
  });
  const images = await Promise.all(pictures.map(async ({ path, type }): Promise<Block> => {
    const data = Buffer.from(await Bun.file(path).arrayBuffer()).toString("base64");
    return { type: "image", source: { type: "base64", media_type: type, data } };
  }));
  const named = turn.attachments.filter((attachment) => imageType(attachment.mime) === undefined);
  const files = named.map((attachment) => `\n[Attached file: ${attachment.path}]`).join("");
  const content: Block[] = [...images, { type: "text", text: `${turn.text}${files}` }];
  return {
    type: "user",
    message: { role: "user", content },
    parent_tool_use_id: null,
    uuid: turn.promptId,
    ...(priority === undefined ? {} : { priority }),
  };
}

/** The picture types Claude takes as images. */
function imageType(mime: string): ImageType | undefined {
  switch (mime) {
    case "image/png":
    case "image/jpeg":
    case "image/gif":
    case "image/webp": {
      return mime;
    }
    default: {
      return undefined;
    }
  }
}
