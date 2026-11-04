// What the model writes, as it writes it (docs/chat.md C10): text and thinking stream per
// content block, each block an item of its own that starts with its first text, and each is
// reconciled with the `assistant` snapshot of its block. The CLI snapshots every block once it
// ends, in a frame of its own, so a block is found by its message's id and its place among
// that message's blocks of its kind (T3 adapter pitfall 30), not by its index in the frame.

import type {
  SDKAssistantMessage,
  SDKPartialAssistantMessage,
} from "@anthropic-ai/claude-agent-sdk";

import type { ItemBody, ItemEvent } from "../provider";

type StreamEvent = SDKPartialAssistantMessage["event"];
type Kind = "text" | "thinking";

/** A block of text or thinking, as it streamed and as its snapshot had it. */
interface Block {
  readonly kind: Kind;
  text: string;
  started: boolean;
  closed: boolean;
}

export class Texts {
  readonly #emit: (event: ItemEvent) => void;
  readonly #blocks = new Map<string, Block>();
  /** The streaming message's blocks by their index in it, as keys. */
  readonly #streaming = new Map<number, string>();
  #message = "message";
  readonly #streamed = { text: 0, thinking: 0 };
  /** How many blocks of each kind each message's snapshots had so far. */
  readonly #snapshotted = new Map<string, { text: number; thinking: number }>();

  constructor(emit: (event: ItemEvent) => void) {
    this.#emit = emit;
  }

  /** Forgets the turn's blocks, as a turn begins and once it has ended. */
  clear(): void {
    this.#blocks.clear();
    this.#streaming.clear();
    this.#snapshotted.clear();
    this.#message = "message";
  }

  /** A stream event of the main conversation. */
  stream(event: StreamEvent): void {
    switch (event.type) {
      case "message_start": {
        this.#message = event.message.id;
        this.#streaming.clear();
        this.#streamed.text = 0;
        this.#streamed.thinking = 0;
        break;
      }
      case "content_block_start": {
        const block = event.content_block;
        if (block.type === "text") {
          this.#grow(this.#open(event.index, "text"), "text", block.text);
        } else if (block.type === "thinking") {
          this.#grow(this.#open(event.index, "thinking"), "thinking", block.thinking);
        }
        break;
      }
      case "content_block_delta": {
        const key = this.#streaming.get(event.index);
        const { delta } = event;
        if (key !== undefined && delta.type === "text_delta") {
          this.#grow(key, "text", delta.text);
        } else if (key !== undefined && delta.type === "thinking_delta") {
          this.#grow(key, "thinking", delta.thinking);
        }
        break;
      }
      case "content_block_stop": {
        const key = this.#streaming.get(event.index);
        if (key !== undefined) {
          this.#close(key);
        }
        break;
      }
      case "message_delta":
      case "message_stop": {
        break;
      }
    }
  }

  /** The text and thinking blocks of a snapshot. */
  snapshot(message: SDKAssistantMessage["message"]): void {
    const seen = this.#snapshotted.get(message.id) ?? { text: 0, thinking: 0 };
    this.#snapshotted.set(message.id, seen);
    for (const block of message.content) {
      if (block.type === "text") {
        this.#settle(`${message.id}:text:${String(seen.text)}`, "text", block.text);
        seen.text += 1;
      } else if (block.type === "thinking") {
        this.#settle(`${message.id}:thinking:${String(seen.thinking)}`, "thinking", block.thinking);
        seen.thinking += 1;
      }
    }
  }

  /** Names the streamed block at `index`: its message and its place among its kind. */
  #open(index: number, kind: Kind): string {
    const key = `${this.#message}:${kind}:${String(this.#streamed[kind])}`;
    this.#streamed[kind] += 1;
    this.#streaming.set(index, key);
    return key;
  }

  /** Adds streamed text to its item, which starts with its first text. */
  #grow(key: string, kind: Kind, text: string): void {
    if (text === "") {
      return;
    }
    const block = this.#block(key, kind);
    if (!block.started) {
      block.started = true;
      this.#emit({ type: "item.started", key, item: body(kind, "") });
    }
    block.text += text;
    this.#emit({ type: "item.appended", key, text });
  }

  #close(key: string): void {
    const block = this.#blocks.get(key);
    if (block === undefined || !block.started || block.closed) {
      return;
    }
    block.closed = true;
    const item = body(block.kind, block.text);
    this.#emit({ type: "item.updated", key, item, status: "completed" });
  }

  /** A block as its snapshot has it, whole: it corrects what streamed, or shows unstreamed. */
  #settle(key: string, kind: Kind, final: string): void {
    const block = this.#block(key, kind);
    if (!block.started) {
      if (final === "") {
        return;
      }
      block.started = true;
    }
    // A thinking block whose text the model withheld keeps what streamed of it.
    const text = final === "" ? block.text : final;
    if (block.closed && text === block.text) {
      return;
    }
    block.text = text;
    block.closed = true;
    this.#emit({ type: "item.updated", key, item: body(kind, text), status: "completed" });
  }

  #block(key: string, kind: Kind): Block {
    const known = this.#blocks.get(key);
    if (known !== undefined) {
      return known;
    }
    const block: Block = { kind, text: "", started: false, closed: false };
    this.#blocks.set(key, block);
    return block;
  }
}

/** Text is the assistant's reply; thinking is shown as reasoning, as the CLI shows it. */
function body(kind: Kind, text: string): ItemBody {
  return kind === "text"
    ? { kind: "assistant_text", payload: { text } }
    : { kind: "reasoning", payload: { text } };
}
