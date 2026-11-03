// The tool calls of a turn (docs/chat.md 4.7). A call starts on its `tool_use` block or its
// permission request, whichever comes first (T3 adapter pitfall 33), and settles on its
// `tool_result`; what's still open when the turn ends, the actor closes (4.5). An Agent call
// is a subagent item, under which its own calls go; a TodoWrite call also shows the to-do
// list, one item a turn that each call updates. AskUserQuestion and ExitPlanMode have no item
// of their own: the request they make stands in the timeline for them.

import type { TodoWriteInput } from "@anthropic-ai/claude-agent-sdk/sdk-tools";
import { z } from "zod";

import { type Item, Todo } from "../../protocol/model";
import type { DriverEvent, ItemBody } from "../provider";
import {
  canonical,
  fieldsOf,
  head,
  type Json,
  text,
  type ToolBody,
  type ToolResult,
  toolSettled,
  toolStarted,
} from "./tools";

type SubagentBody = Extract<ItemBody, { kind: "subagent" }>;
type Listed = Pick<TodoWriteInput["todos"][number], "content" | "status">;

/** A call as it's first seen: its tool's name, its input, and the input bounded if known. */
interface Called {
  readonly name: string;
  readonly input: unknown;
  readonly kept?: Json;
}

/** The most of a subagent's report its item keeps. */
const SUMMARY_BOUND = 4 * 1024;

/** A TodoWrite call's input, typed by the CLI's own so it can't drift from it unseen. */
const Todos: z.ZodType<{ todos: Listed[] }> = z.object({
  todos: z.array(z.object({ content: z.string(), status: Todo.shape.status })),
});

export class Steps {
  readonly #emit: (event: DriverEvent) => void;
  /** The calls started and not yet settled, by their tool use's id. */
  readonly #open = new Map<string, ToolBody | SubagentBody>();
  readonly #settled = new Set<string>();
  readonly #declined = new Set<string>();
  #todos = "todos";
  #listed = false;

  constructor(emit: (event: DriverEvent) => void) {
    this.#emit = emit;
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
  start(id: string, call: Called, parentKey: string | null): void {
    const { name, input } = call;
    const tool = canonical(name);
    if (this.#open.has(id) || this.#settled.has(id)) {
      return;
    }
    if (tool === "AskUserQuestion" || tool === "ExitPlanMode") {
      return;
    }
    const body = tool === "Agent" ? subagent(input) : toolStarted(name, input, call.kept);
    this.#open.set(id, body);
    this.#emit({ type: "item.started", key: id, parentKey, item: body });
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
    const item = body.kind === "tool" ? toolSettled(body, result) : reported(body, result.text);
    this.#emit({ type: "item.updated", key: id, item, status });
  }

  /** A subagent that ran in the background finished, with its summary. */
  finished(id: string, summary: string, ok: boolean): void {
    const body = this.#open.get(id);
    if (body?.kind !== "subagent") {
      return;
    }
    this.#open.delete(id);
    this.#settled.add(id);
    const status = ok ? "completed" : "failed";
    this.#emit({ type: "item.updated", key: id, item: reported(body, summary), status });
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
      this.#emit({ type: "item.started", key: this.#todos, parentKey: null, item });
    }
    this.#emit({ type: "item.updated", key: this.#todos, item, status: "completed" });
  }
}

function subagent(input: unknown): SubagentBody {
  const fields = fieldsOf(input);
  return {
    kind: "subagent",
    payload: {
      description: text(fields, "description") ?? "Subagent",
      agentType: text(fields, "subagent_type") ?? null,
      summary: null,
    },
  };
}

function reported(body: SubagentBody, summary: string): SubagentBody {
  const said = summary.trim();
  return {
    kind: "subagent",
    payload: { ...body.payload, summary: said === "" ? null : head(said, SUMMARY_BOUND) },
  };
}
