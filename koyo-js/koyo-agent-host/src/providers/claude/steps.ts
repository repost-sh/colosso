// The tool calls of a turn or a subagent (docs/chat.md 4.7). A call starts on its `tool_use`
// block or its permission request, whichever comes first (T3 adapter pitfall 33), and settles
// on its `tool_result`; what's still open when the turn ends, the actor closes (4.5). An Agent
// call is a subagent's start, for those who keep them; a TodoWrite call also shows the to-do
// list, one item a turn that each call updates. AskUserQuestion and ExitPlanMode have no item
// of their own: the request they make stands in the timeline for them.

import type { SDKUserMessage } from "@anthropic-ai/claude-agent-sdk";
import type { TodoWriteInput } from "@anthropic-ai/claude-agent-sdk/sdk-tools";
import { z } from "zod";

import { type Item, Todo } from "../../protocol/model";
import type { ItemBody, ItemEvent } from "../provider";
import {
  canonical,
  type Json,
  type ToolBody,
  type ToolResult,
  toolSettled,
  toolStarted,
} from "./tools";

type Content = Exclude<SDKUserMessage["message"]["content"], string>;
type ResultContent = Extract<Content[number], { type: "tool_result" }>["content"];
type Listed = Pick<TodoWriteInput["todos"][number], "content" | "status">;

/** A call as it's first seen: its tool's name, its input, and the input bounded if known. */
interface Called {
  readonly name: string;
  readonly input: unknown;
  readonly kept?: Json;
}

/** Who an Agent call goes to. */
export interface Spawner {
  /** An Agent call was seen; `parentKey` names the subagent that made it, if one did. */
  readonly start: (key: string, input: unknown, parentKey: string | null) => void;
  /** Whether the result of call `key` was a subagent's, which it took. */
  readonly settle: (key: string, result: ToolResult) => boolean;
}

/** A TodoWrite call's input, typed by the CLI's own so it can't drift from it unseen. */
const Todos: z.ZodType<{ todos: Listed[] }> = z.object({
  todos: z.array(z.object({ content: z.string(), status: Todo.shape.status })),
});

export class Steps {
  readonly #emit: (event: ItemEvent) => void;
  readonly #spawner: Spawner;
  /** The subagent these are the calls of, if they are one's. */
  readonly #owner: string | null;
  /** The calls started and not yet settled, by their tool use's id. */
  readonly #open = new Map<string, ToolBody>();
  readonly #settled = new Set<string>();
  readonly #declined = new Set<string>();
  #todos = "todos";
  #listed = false;

  constructor(emit: (event: ItemEvent) => void, spawner: Spawner, owner: string | null) {
    this.#emit = emit;
    this.#spawner = spawner;
    this.#owner = owner;
  }

  /** Readies for a turn, naming its to-do list `${turn}:todos`. */
  begin(turn: string): void {
    this.clear();
    this.#todos = `${turn}:todos`;
  }

  /** Forgets the turn's calls. */
  clear(): void {
    this.#open.clear();
    this.#settled.clear();
    this.#declined.clear();
    this.#listed = false;
  }

  /** A call seen, from its block or its permission request; the second sight adds nothing. */
  start(id: string, call: Called): void {
    const { name, input } = call;
    const tool = canonical(name);
    if (this.#open.has(id) || this.#settled.has(id)) {
      return;
    }
    if (tool === "AskUserQuestion" || tool === "ExitPlanMode") {
      return;
    }
    if (tool === "Agent") {
      this.#spawner.start(id, input, this.#owner);
      return;
    }
    const body = toolStarted(name, input, call.kept);
    this.#open.set(id, body);
    this.#emit({ type: "item.started", key: id, item: body });
    if (tool === "TodoWrite") {
      this.#list(input);
    }
  }

  /** The person or Claude's checks declined the call, whose result then reads as declined. */
  decline(id: string): void {
    this.#declined.add(id);
  }

  /** The call's result came. */
  settle(id: string, result: ToolResult): void {
    if (this.#spawner.settle(id, result)) {
      return;
    }
    const body = this.#open.get(id);
    if (body === undefined) {
      return;
    }
    this.#open.delete(id);
    this.#settled.add(id);
    let status: Item["status"] = result.isError ? "failed" : "completed";
    if (this.#declined.has(id)) {
      status = "declined";
    }
    this.#emit({ type: "item.updated", key: id, item: toolSettled(body, result), status });
  }

  #list(input: unknown): void {
    const parsed = Todos.safeParse(input);
    if (!parsed.success) {
      return;
    }
    const todos = parsed.data.todos.map((todo) => ({ text: todo.content, status: todo.status }));
    const item: ItemBody = { kind: "todos", payload: { todos } };
    if (!this.#listed) {
      this.#listed = true;
      this.#emit({ type: "item.started", key: this.#todos, item });
    }
    this.#emit({ type: "item.updated", key: this.#todos, item, status: "completed" });
  }
}

/** Settles the calls whose results a user frame's `content` has. */
export function settleResults(steps: Steps, content: Content, structured: unknown): void {
  const results = content.flatMap((block) => block.type === "tool_result" ? [block] : []);
  for (const block of results) {
    steps.settle(block.tool_use_id, {
      text: resultText(block.content),
      isError: block.is_error === true,
      // The CLI's structured twin is the message's, so it's only one result's (pitfall 32).
      structured: results.length === 1 ? structured : undefined,
    });
  }
}

/** What a message or a tool's result says, as the model read it. */
export function resultText(content: ResultContent | Content): string {
  if (content === undefined || typeof content === "string") {
    return content ?? "";
  }
  return content
    .flatMap((block) => {
      if (block.type === "text") {
        return [block.text];
      }
      return block.type === "image" ? ["[image]"] : [];
    })
    .join("\n");
}
