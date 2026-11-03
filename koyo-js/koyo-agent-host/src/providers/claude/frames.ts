// A turn's frames as timeline items (docs/chat.md 4.7): each frame the turn is credited with
// goes to what it's about, the text the model writes (`Texts`), the tools it calls (`Steps`)
// or what it used (`Meter`); compaction becomes one item, from its status, boundary and result
// frames; what the CLI says for the person becomes a notice; the turn's result says how it
// ended and what it used. A frame outside the SDK's union, or a system frame of a subtype
// this driver doesn't know, is logged and dropped.

import type {
  SDKAssistantMessage,
  SDKCompactBoundaryMessage,
  SDKMessage,
  SDKPartialAssistantMessage,
  SDKRateLimitEvent,
  SDKResultMessage,
  SDKStatusMessage,
  SDKSystemMessage,
  SDKUserMessage,
  SDKUserMessageReplay,
} from "@anthropic-ai/claude-agent-sdk";

import { log } from "../../log";
import type { Item, NoticeLevel, Usage } from "../../protocol/model";
import type { DriverEvent, ItemBody } from "../provider";
import { classify, type Ending, LIMITED, resultUsage, type Signs } from "./results";
import { Steps } from "./steps";
import { Texts } from "./text";
import type { Fields, Json } from "./tools";
import { Meter } from "./usage";

type System = Extract<SDKMessage, { type: "system" }>;
type CompactionBody = Extract<ItemBody, { kind: "compaction" }>;

interface Compaction {
  readonly key: string;
  body: CompactionBody;
  open: boolean;
}

type ResultContent = Extract<
  Exclude<SDKUserMessage["message"]["content"], string>[number],
  { type: "tool_result" }
>["content"];

const RETRYING = "Claude's API answered with an error; Claude Code is trying again.";
const UNSAVED = "Claude couldn't save this conversation: the disk is full.";

export class Frames {
  readonly #emit: (event: DriverEvent) => void;
  readonly #steps: Steps;
  readonly #meter = new Meter();
  readonly #texts: Texts;
  #turn = "turn";
  #compact = false;
  #compaction: Compaction | undefined;
  #compactions = 0;
  #authFailed = false;
  #overloaded = false;
  #limited: { retryAt: string | null } | null = null;
  #retried = false;

  constructor(emit: (event: DriverEvent) => void) {
    this.#emit = emit;
    this.#steps = new Steps(emit);
    this.#texts = new Texts(emit);
  }

  /** Readies for a new turn, named `turn`; `compact` when it compacts the conversation. */
  begin(turn: string, compact: boolean): void {
    this.#turn = turn;
    this.#compact = compact;
    this.#compaction = undefined;
    this.#compactions = 0;
    this.#authFailed = false;
    this.#overloaded = false;
    this.#limited = null;
    this.#retried = false;
    this.#texts.clear();
    this.#steps.begin(turn);
    this.#meter.begin();
  }

  /**
   * Forgets the turn's text, calls and compaction once it has ended, so none of it is kept
   * while the session idles; what was left open, the actor closes (4.5).
   */
  end(): void {
    this.#texts.clear();
    this.#steps.clear();
    this.#compaction = undefined;
  }

  /** The session's `init`, which names the model whose window the results tell. */
  init(frame: SDKSystemMessage): void {
    this.#meter.model = frame.model;
  }

  /** A call seen first in its permission request, its input bounded as `kept`. */
  toolStarted(toolUseId: string, name: string, input: Fields, kept: Json): void {
    this.#steps.start(toolUseId, { name, input, kept }, null);
  }

  /** The person declined a call, whose result then reads as declined. */
  toolDeclined(toolUseId: string): void {
    this.#steps.decline(toolUseId);
  }

  /**
   * How the turn's `result` ended it, `interrupted` when the person asked it to stop, and
   * what it used; a `/compact` that didn't compact says why in its result, shown as a notice
   * (T3 adapter pitfall 39).
   */
  result(result: SDKResultMessage, interrupted: boolean): { ending: Ending; usage: Usage } {
    this.#meter.learn(result);
    const usage = resultUsage(result, this.#meter.context());
    const ending = classify(result, this.signs(interrupted));
    if (this.#compact && !this.compacted && result.subtype === "success" && !result.is_error) {
      this.#notice("info", result.result.trim());
    }
    return { ending, usage };
  }

  /** Maps one frame credited to the running turn. */
  frame(frame: SDKMessage): void {
    switch (frame.type) {
      case "stream_event": {
        this.#stream(frame);
        return;
      }
      case "assistant": {
        this.#assistant(frame);
        return;
      }
      case "user": {
        this.#user(frame);
        return;
      }
      case "system": {
        this.#system(frame);
        return;
      }
      case "rate_limit_event": {
        this.#rateLimit(frame);
        return;
      }
      case "auth_status": {
        this.#notice("error", frame.error ?? "");
        return;
      }
      // Results never come here (`Turns` takes them first, for `result`); the rest has
      // nothing for the timeline.
      case "result":
      case "tool_progress":
      case "tool_use_summary":
      case "prompt_suggestion":
      case "conversation_reset": {
        return;
      }
    }
    unknown(frame);
  }

  /** What the turn's frames said that bears on how it ended. */
  signs(interrupted: boolean): Signs {
    return {
      authFailed: this.#authFailed,
      overloaded: this.#overloaded,
      limited: this.#limited,
      interrupted,
    };
  }

  /** Whether the turn compacted the conversation. */
  get compacted(): boolean {
    return this.#compactions > 0;
  }

  #stream(frame: SDKPartialAssistantMessage): void {
    // A subagent's own text isn't shown (its transcript is skipped); its tools are.
    if (frame.parent_tool_use_id !== null) {
      return;
    }
    const { event } = frame;
    this.#texts.stream(event);
    if (event.type === "message_start") {
      this.#meter.start(event.message.id, event.message.usage);
      this.#emit({ type: "usage", usage: this.#meter.used() });
    } else if (event.type === "message_delta") {
      this.#meter.delta(event.usage);
      this.#emit({ type: "usage", usage: this.#meter.used() });
    }
  }

  #assistant(frame: SDKAssistantMessage): void {
    this.#sign(frame.error);
    const { message } = frame;
    const parentKey = frame.parent_tool_use_id;
    if (parentKey === null && message.model === "<synthetic>") {
      // The CLI's own words, such as an API error, come as a message of no model.
      const said = message.content.flatMap((block) => block.type === "text" ? [block.text] : []);
      this.#notice(frame.error === undefined ? "info" : "error", said.join("\n"));
      return;
    }
    if (parentKey === null) {
      this.#texts.snapshot(message);
    }
    for (const block of message.content) {
      if (block.type === "tool_use") {
        this.#steps.start(block.id, { name: block.name, input: block.input }, parentKey);
      }
    }
  }

  #user(frame: SDKUserMessage | SDKUserMessageReplay): void {
    // A replay is the CLI's record of what was said, as a compaction's summary.
    if ("isReplay" in frame) {
      return;
    }
    const { content } = frame.message;
    if (typeof content === "string") {
      return;
    }
    const results = content.flatMap((block) => block.type === "tool_result" ? [block] : []);
    for (const block of results) {
      this.#steps.settle(block.tool_use_id, {
        text: resultText(block.content),
        isError: block.is_error === true,
        // The CLI's structured twin is the message's, so it's only one result's (pitfall 32).
        structured: results.length === 1 ? frame.tool_use_result : undefined,
      });
    }
  }

  #system(frame: System): void {
    switch (frame.subtype) {
      case "status": {
        this.#status(frame);
        return;
      }
      case "compact_boundary": {
        this.#boundary(frame);
        return;
      }
      case "api_retry": {
        if (!this.#retried) {
          this.#retried = true;
          this.#notice("warning", RETRYING);
        }
        return;
      }
      case "model_refusal_fallback":
      case "model_refusal_no_fallback": {
        this.#notice("warning", frame.content);
        return;
      }
      case "local_command_output": {
        this.#notice("info", frame.content);
        return;
      }
      case "informational": {
        // Info and suggestion lines are the CLI's chatter (T3 adapter pitfall 42).
        if (frame.level === "warning" || frame.level === "notice") {
          this.#notice(frame.level === "warning" ? "warning" : "info", frame.content);
        }
        return;
      }
      case "task_notification": {
        if (frame.tool_use_id !== undefined) {
          this.#steps.finished(frame.tool_use_id, frame.summary, frame.status !== "failed");
        }
        return;
      }
      case "permission_denied": {
        this.#steps.decline(frame.tool_use_id);
        return;
      }
      case "session_writes_lost": {
        this.#notice("error", UNSAVED);
        return;
      }
      case "init":
      case "control_request_progress":
      case "hook_started":
      case "hook_progress":
      case "hook_response":
      case "plugin_install":
      case "task_started":
      case "task_updated":
      case "task_progress":
      case "background_tasks_changed":
      case "thinking_tokens":
      case "session_state_changed":
      case "worker_shutting_down":
      case "commands_changed":
      case "notification":
      case "files_persisted":
      case "memory_recall":
      case "elicitation_complete":
      case "mirror_error": {
        return;
      }
    }
    unknownSystem(frame);
  }

  // Compaction (T3 adapter pitfalls 39, 40).

  #status(frame: SDKStatusMessage): void {
    if (frame.status === "compacting") {
      this.#compacting();
      return;
    }
    if (frame.compact_result === undefined) {
      return;
    }
    this.#compacted(frame.compact_result === "success" ? "completed" : "failed");
    if (frame.compact_result === "failed" && frame.compact_error !== undefined) {
      this.#notice("error", frame.compact_error);
    }
  }

  #compacting(): Compaction {
    if (this.#compaction?.open === true) {
      return this.#compaction;
    }
    this.#compactions += 1;
    const compaction: Compaction = {
      key: `${this.#turn}:compaction:${String(this.#compactions)}`,
      body: {
        kind: "compaction",
        payload: { trigger: this.#compact ? "manual" : "auto", before: null, after: null },
      },
      open: true,
    };
    this.#compaction = compaction;
    const { key, body } = compaction;
    this.#emit({ type: "item.started", key, parentKey: null, item: body });
    return compaction;
  }

  /** Ends the open compaction with `status`. */
  #compacted(status: Item["status"]): void {
    const compaction = this.#compaction;
    if (compaction?.open !== true) {
      return;
    }
    compaction.open = false;
    this.#emit({ type: "item.updated", key: compaction.key, item: compaction.body, status });
  }

  /** The boundary a compaction leaves, with what the context held before and after. */
  #boundary(frame: SDKCompactBoundaryMessage): void {
    const meta = frame.compact_metadata;
    const compaction = this.#compaction ?? this.#compacting();
    compaction.open = false;
    compaction.body = {
      kind: "compaction",
      payload: { trigger: meta.trigger, before: meta.pre_tokens, after: meta.post_tokens ?? null },
    };
    const { key, body } = compaction;
    this.#emit({ type: "item.updated", key, item: body, status: "completed" });
    if (meta.post_tokens !== undefined) {
      this.#meter.compacted(meta.post_tokens);
      this.#emit({ type: "usage", usage: this.#meter.used() });
    }
  }

  // Limits and errors (T3 adapter pitfall 41).

  #rateLimit(frame: SDKRateLimitEvent): void {
    const info = frame.rate_limit_info;
    const overage = info.overageStatus === "allowed" || info.overageStatus === "allowed_warning";
    if (info.status !== "rejected" || overage) {
      return;
    }
    const resets = info.resetsAt;
    const retryAt = resets === undefined ? null : new Date(resets * 1000).toISOString();
    if (this.#limited === null) {
      this.#notice("warning", LIMITED);
    }
    this.#limited = { retryAt };
  }

  #sign(error: SDKAssistantMessage["error"]): void {
    if (error === "authentication_failed") {
      this.#authFailed = true;
    } else if (error === "overloaded") {
      this.#overloaded = true;
    } else if (error === "rate_limit") {
      this.#limited ??= { retryAt: null };
    }
  }

  #notice(level: NoticeLevel, text: string): void {
    if (text.trim() !== "") {
      this.#emit({ type: "notice", level, text });
    }
  }
}

/** A tool result's text, as the model read it. */
function resultText(content: ResultContent): string {
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

/** A frame of a type outside the SDK's union, as arrive at runtime: logged and dropped. */
function unknown(frame: { readonly type: string }): void {
  log.debug(`Dropped a frame of type ${frame.type}.`);
}

function unknownSystem(frame: { readonly subtype: string }): void {
  log.debug(`Dropped a system frame of subtype ${frame.subtype}.`);
}
