// What the CLI says of a session's subagents and other tasks, as recorded (0.3.317).

import { randomUUID } from "node:crypto";

import type {
  SDKBackgroundTasksChangedMessage,
  SDKTaskNotificationMessage,
  SDKTaskProgressMessage,
  SDKTaskStartedMessage,
  SDKTaskUpdatedMessage,
  SDKUserMessage,
} from "@anthropic-ai/claude-agent-sdk";

import { SESSION } from "./frames";

/** The CLI's list of what runs in the background, by task type, `ambient` for one that isn't
 * the agent's work. */
export function backgroundTasks(
  ...tasks: readonly { readonly type: string; readonly ambient?: boolean }[]
): SDKBackgroundTasksChangedMessage {
  return {
    type: "system",
    subtype: "background_tasks_changed",
    tasks: tasks.map(({ type, ambient }, at) => ({
      task_id: `task-${String(at)}`,
      task_type: type,
      description: `${type} ${String(at)}`,
      ...(ambient === undefined ? {} : { ambient }),
    })),
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function taskNotification(
  taskId: string,
  toolUseId: string,
  summary: string,
  state: SDKTaskNotificationMessage["status"] = "completed",
): SDKTaskNotificationMessage {
  return {
    type: "system",
    subtype: "task_notification",
    task_id: taskId,
    tool_use_id: toolUseId,
    status: state,
    output_file: "",
    summary,
    usage: { total_tokens: 23_219, tool_uses: 1, duration_ms: 4987 },
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** A subagent's task starting, as recorded. */
export function taskStarted(
  taskId: string,
  toolUseId: string,
  shape: { readonly background?: boolean; readonly prompt?: string } = {},
): SDKTaskStartedMessage {
  return {
    type: "system",
    subtype: "task_started",
    task_id: taskId,
    tool_use_id: toolUseId,
    description: "Count files",
    subagent_type: "general-purpose",
    is_backgrounded: shape.background ?? false,
    spawn_depth: 1,
    task_type: "local_agent",
    prompt: shape.prompt ?? "Count the files.",
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** A shell a subagent ran, which is a task of its own, as recorded. */
export function shellStarted(
  taskId: string,
  toolUseId: string,
  owner?: string,
): SDKTaskStartedMessage {
  return {
    ...(owner === undefined ? {} : { parent_task_id: owner }),
    type: "system",
    subtype: "task_started",
    task_id: taskId,
    tool_use_id: toolUseId,
    description: "Wait for 8 seconds",
    is_backgrounded: false,
    task_type: "local_bash",
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** What a subagent's doing, as recorded. */
export function taskProgress(taskId: string, toolUseId: string): SDKTaskProgressMessage {
  return {
    type: "system",
    subtype: "task_progress",
    task_id: taskId,
    tool_use_id: toolUseId,
    description: "Running List files in the working directory",
    subagent_type: "general-purpose",
    usage: { total_tokens: 19_613, tool_uses: 1, duration_ms: 2760 },
    last_tool_name: "Bash",
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** A task moved to the background, or not. */
export function taskUpdated(taskId: string, background: boolean): SDKTaskUpdatedMessage {
  return {
    type: "system",
    subtype: "task_updated",
    task_id: taskId,
    patch: { is_backgrounded: background },
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** Words said to a subagent, as its first frame says its prompt. */
export function saidToSubagent(parent: string, said: string): SDKUserMessage {
  return {
    type: "user",
    message: { role: "user", content: [{ type: "text", text: said }] },
    parent_tool_use_id: parent,
    uuid: randomUUID(),
    session_id: SESSION,
  };
}
