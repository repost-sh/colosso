// Claude's tools as the timeline shows them (docs/chat.md 4.7): each tool's family, a label
// to show it by, what it was given and what came back, bounded to 16 KB (4.5.8), and what an
// approval of it shows a person.

import { LEGACY_TOOL_NAME_ALIASES } from "@anthropic-ai/claude-agent-sdk";
import type { BashOutput, FileEditOutput } from "@anthropic-ai/claude-agent-sdk/sdk-tools";
import { z } from "zod";

import { Hunk, type ToolFamily } from "../../protocol/model";
import type { ItemBody } from "../provider";

/** The most of a tool's input or output an item keeps. */
const BOUND = 16 * 1024;
/** The most of any one string of a tool's input an item keeps. */
const FIELD_BOUND = 4 * 1024;

export type ToolBody = Extract<ItemBody, { kind: "tool" }>;
/** A tool's input as an item keeps it (`bounded`). */
export type Json = ToolBody["payload"]["input"];

/** A tool's input, by the names of its fields. */
export type Fields = Readonly<Record<string, unknown>>;

const AnyRecord = z.record(z.string(), z.unknown());
const AnyJson = z.json();
// What the CLI's structured results are read for, typed by its own tools' types so they
// can't drift from them unseen.
const Patched: z.ZodType<Pick<FileEditOutput, "structuredPatch">> = z.object({
  structuredPatch: z.array(Hunk),
});
const Printed: z.ZodType<Pick<BashOutput, "stdout" | "stderr">> = z.object({
  stdout: z.string(),
  stderr: z.string(),
});

/** The name a tool goes by now, for one that was renamed (`Task` is `Agent`). */
export function canonical(name: string): string {
  return LEGACY_TOOL_NAME_ALIASES[name] ?? name;
}

export function family(name: string): ToolFamily {
  switch (canonical(name)) {
    case "Bash": {
      return "command";
    }
    case "Read": {
      return "read";
    }
    case "Edit":
    case "Write":
    case "MultiEdit":
    case "NotebookEdit": {
      return "edit";
    }
    case "Grep":
    case "Glob":
    case "LS": {
      return "search";
    }
    case "WebSearch":
    case "WebFetch": {
      return "web";
    }
    case "Agent": {
      return "agent";
    }
    case "TodoWrite": {
      return "todo";
    }
    case "Skill": {
      return "skill";
    }
    default: {
      return name.startsWith("mcp__") ? "mcp" : "other";
    }
  }
}

/** An MCP tool's server and its own name, from `mcp__<server>__<tool>`. */
export function mcpParts(name: string): { server: string; tool: string } | undefined {
  const match = /^mcp__(.+?)__(.+)$/u.exec(name);
  const [, server, tool] = match ?? [];
  return server === undefined || tool === undefined ? undefined : { server, tool };
}

/** A tool's input as fields; anything but an object has none. */
export function fieldsOf(input: unknown): Fields {
  const parsed = AnyRecord.safeParse(input);
  return parsed.success ? parsed.data : {};
}

/** The text in field `name`, when it holds text. */
export function text(fields: Fields, name: string): string | undefined {
  const value = fields[name];
  return typeof value === "string" ? value : undefined;
}

/** The file a call reads or changes, a notebook's included. */
function pathOf(fields: Fields): string | undefined {
  return text(fields, "file_path") ?? text(fields, "notebook_path");
}

/** What a tool call is shown by. */
export function label(name: string, fields: Fields): string {
  const path = pathOf(fields);
  switch (family(name)) {
    case "command": {
      return firstLine(text(fields, "command") ?? name);
    }
    case "read":
    case "edit": {
      return path ?? name;
    }
    case "search": {
      const pattern = text(fields, "pattern") ?? name;
      const where = text(fields, "path");
      return where === undefined ? pattern : `${pattern} in ${where}`;
    }
    case "web": {
      return text(fields, "query") ?? text(fields, "url") ?? name;
    }
    case "mcp": {
      const parts = mcpParts(name);
      return parts === undefined ? name : `${parts.server}: ${parts.tool}`;
    }
    case "agent": {
      return text(fields, "description") ?? name;
    }
    case "todo": {
      return "To-dos";
    }
    case "skill": {
      return text(fields, "skill") ?? name;
    }
    case "other": {
      return name;
    }
  }
}

/** The files a call reads or changes. */
function files(name: string, fields: Fields): string[] {
  const kind = family(name);
  const path = pathOf(fields);
  return (kind === "edit" || kind === "read") && path !== undefined ? [path] : [];
}

/**
 * A tool call as it starts: what it was given, nothing yet of what it gave back; `kept` is
 * the input bounded, when that's been worked out already.
 */
export function toolStarted(name: string, input: unknown, kept: Json = bounded(input)): ToolBody {
  const fields = fieldsOf(input);
  return {
    kind: "tool",
    payload: {
      name: canonical(name),
      family: family(name),
      label: label(name, fields),
      input: kept,
      output: null,
      exitCode: null,
      patch: null,
      files: files(name, fields),
      server: mcpParts(name)?.server ?? null,
    },
  };
}

/** What a tool's result said, as its `tool_result` block and the CLI's structured twin. */
export interface ToolResult {
  /** The result's text, as the model read it. */
  readonly text: string;
  readonly isError: boolean;
  /** The CLI's structured result, when the message carried only this one (pitfall 32). */
  readonly structured: unknown;
}

/** A tool call once its result came. */
export function toolSettled(started: ToolBody, result: ToolResult): ToolBody {
  const { family: kind } = started.payload;
  const printed = kind === "command" ? Printed.safeParse(result.structured) : undefined;
  const patch = kind === "edit" ? Patched.safeParse(result.structured) : undefined;
  const output = printed?.success === true
    ? [printed.data.stdout, printed.data.stderr].filter((part) => part !== "").join("\n")
    : result.text;
  return {
    kind: "tool",
    payload: {
      ...started.payload,
      output: tail(output),
      exitCode: kind === "command" ? exitCode(result) : null,
      patch: patch?.success === true ? boundedPatch(patch.data.structuredPatch) : null,
    },
  };
}

/** A patch as an item keeps it: whole lines up to the bound, then `…` where it was cut. */
export function boundedPatch(hunks: readonly Hunk[]): Hunk[] {
  const kept: Hunk[] = [];
  let room = BOUND;
  for (const hunk of hunks) {
    const lines: string[] = [];
    for (const line of hunk.lines) {
      room -= line.length + 1;
      if (room < 0) {
        return [...kept, { ...hunk, lines: [...lines, " …"] }];
      }
      lines.push(line);
    }
    kept.push(hunk);
  }
  return kept;
}

/** A command's exit code: 0 when it ran well, else the code its result names, if any. */
function exitCode(result: ToolResult): number | null {
  if (!result.isError) {
    return 0;
  }
  const code = /Exit code (\d+)/u.exec(result.text)?.[1];
  return code === undefined ? null : Number.parseInt(code, 10);
}

/**
 * What a request to run a tool shows as is, at most `BOUND` long: a command, a path, or else its
 * input as an item keeps it, `kept`.
 */
export function detail(name: string, fields: Fields, kept: Json): string {
  switch (canonical(name)) {
    case "Bash": {
      return head(text(fields, "command") ?? "");
    }
    case "Read":
    case "Edit":
    case "Write":
    case "MultiEdit":
    case "NotebookEdit": {
      return pathOf(fields) ?? "";
    }
    default: {
      return head(JSON.stringify(kept, null, 2));
    }
  }
}

/** The lines of the change a request to edit a file would make: bounded heads of what it says. */
export function change(name: string, fields: Fields): string[] | null {
  const marked = (field: string, mark: string): string[] =>
    head(text(fields, field) ?? "").split("\n").map((line) => `${mark}${line}`);
  switch (canonical(name)) {
    case "Edit": {
      return [...marked("old_string", "-"), ...marked("new_string", "+")];
    }
    case "Write": {
      return marked("content", "+");
    }
    default: {
      return null;
    }
  }
}

/** The end of `printed`, at most `BOUND` long: what a tool printed last matters most. */
function tail(printed: string): string {
  return printed.length > BOUND ? `…${printed.slice(-BOUND)}` : printed;
}

/** The start of `value`, at most `limit` long. */
export function head(value: string, limit: number = BOUND): string {
  return value.length > limit ? `${value.slice(0, limit)}…` : value;
}

/** `input` in the JSON an item keeps: long strings cut, and the whole cut if still too long. */
export function bounded(input: unknown): Json {
  const parsed = AnyJson.safeParse(input);
  if (!parsed.success) {
    return null;
  }
  const shortened = shorten(parsed.data);
  const written = JSON.stringify(shortened);
  return written.length > BOUND ? head(written) : shortened;
}

function shorten(value: Json): Json {
  if (typeof value === "string") {
    return head(value, FIELD_BOUND);
  }
  if (Array.isArray(value)) {
    return value.map(shorten);
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, each]) => [key, shorten(each)]));
  }
  return value;
}

function firstLine(value: string): string {
  return head(value.split("\n")[0] ?? "", 200);
}
