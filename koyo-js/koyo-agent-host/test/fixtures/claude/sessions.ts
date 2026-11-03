// The recorded sessions' turns, frame for frame as the CLI sent them (hook, thinking-token
// and UI frames left out: the driver drops them), for the prompt whose uuid they echo.

import type { SDKMessage } from "@anthropic-ai/claude-agent-sdk";

import {
  assistant,
  blockStart,
  blockStop,
  init,
  message,
  messageDelta,
  messageStart,
  messageStop,
  rateLimit,
  status,
  stream,
  success,
  textBlock,
  textDelta,
  thinkingBlock,
  toolResults,
  toolUse,
} from "./frames";

export const NOTES = "/work/rec-cwd/notes.txt";
const EDITED = `The file ${NOTES} has been updated successfully.`;
export const SKY = "Sunlight holds every color, but air scatters blue the most.";

/** What every recorded turn starts with: the session's settings and the API request. */
function opening(prompt: string, mode: "auto" | "default" = "auto"): SDKMessage[] {
  return [
    init(mode, { echo: [prompt] }),
    status("requesting"),
    rateLimit({ status: "allowed", resetsAt: 1_793_865_600, overageStatus: "rejected" }),
  ];
}

/** "Answer in two short sentences: why is the sky blue?" */
export function textTurn(prompt: string): SDKMessage[] {
  const id = "msg_011CggdQ7qq4WaL7TjL1qvXv";
  const echo = { echo: [prompt] };
  return [
    ...opening(prompt),
    stream(messageStart(id, { input: 2, output: 1, cacheCreation: 28_562 }), echo),
    stream(blockStart(0, textBlock(""))),
    stream(textDelta(0, "Sunlight holds every color, ")),
    stream(textDelta(0, "but air scatters blue the most.")),
    assistant(message(id, [textBlock(SKY)]), echo),
    stream(blockStop(0)),
    stream(messageDelta(104)),
    stream(messageStop()),
    success([prompt], { text: SKY }),
  ];
}

/** "Write notes.txt with 'one', edit it to 'two', `cat` it, then reply 'done'." */
export function toolsTurn(prompt: string): SDKMessage[] {
  const echo = { echo: [prompt] };
  return [
    ...opening(prompt, "default"),
    stream(messageStart("msg_1"), echo),
    stream(blockStart(0, toolUse("toolu_write", "Write", {}))),
    assistant(message("msg_1", [
      toolUse("toolu_write", "Write", { file_path: NOTES, content: "one\n" }),
    ]), echo),
    stream(blockStop(0)),
    stream(messageDelta(60, "tool_use")),
    stream(messageStop()),
    toolResults([{ id: "toolu_write", content: `File created successfully at: ${NOTES}` }], {
      type: "create",
      filePath: NOTES,
      content: "one\n",
      structuredPatch: [],
      originalFile: null,
      userModified: false,
    }),
    status("requesting"),
    stream(messageStart("msg_2")),
    stream(blockStart(0, toolUse("toolu_edit", "Edit", {}))),
    assistant(message("msg_2", [
      toolUse("toolu_edit", "Edit", {
        replace_all: false,
        file_path: NOTES,
        old_string: "one",
        new_string: "two",
      }),
    ])),
    stream(blockStop(0)),
    stream(messageDelta(80, "tool_use")),
    stream(messageStop()),
    toolResults([{ id: "toolu_edit", content: EDITED }], {
      filePath: NOTES,
      oldString: "one",
      newString: "two",
      originalFile: "one\n",
      structuredPatch: [
        { oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-one", "+two"] },
      ],
      userModified: false,
      replaceAll: false,
    }),
    status("requesting"),
    stream(messageStart("msg_3")),
    stream(blockStart(0, toolUse("toolu_bash", "Bash", {}))),
    assistant(message("msg_3", [
      toolUse("toolu_bash", "Bash", {
        command: "cat notes.txt",
        description: "Print contents of notes.txt",
      }),
    ])),
    stream(blockStop(0)),
    stream(messageDelta(70, "tool_use")),
    stream(messageStop()),
    toolResults([{ id: "toolu_bash", content: "two" }], {
      stdout: "two",
      stderr: "",
      interrupted: false,
      isImage: false,
      noOutputExpected: false,
    }),
    status("requesting"),
    stream(messageStart("msg_4")),
    stream(blockStart(0, textBlock(""))),
    assistant(message("msg_4", [textBlock("done")])),
    stream(blockStop(0)),
    stream(messageDelta(2)),
    stream(messageStop()),
    success([prompt], { text: "done" }),
  ];
}

/**
 * "Use AskUserQuestion to ask whether I prefer tabs or spaces": a thinking block whose text
 * the CLI withholds, then the question, which goes through `canUseTool` once its block ends.
 */
export function askTurn(prompt: string): SDKMessage[] {
  const echo = { echo: [prompt] };
  return [
    ...opening(prompt, "default"),
    stream(messageStart("msg_ask"), echo),
    stream(blockStart(0, thinkingBlock())),
    assistant(message("msg_ask", [thinkingBlock()]), echo),
    stream(blockStop(0)),
    stream(blockStart(1, toolUse("toolu_ask", "AskUserQuestion", {}))),
    assistant(message("msg_ask", [toolUse("toolu_ask", "AskUserQuestion", QUESTIONS)])),
    stream(blockStop(1)),
    stream(messageDelta(90, "tool_use")),
    stream(messageStop()),
  ];
}

export const QUESTIONS = {
  questions: [{
    question: "Do you prefer tabs or spaces?",
    header: "Indentation",
    multiSelect: false,
    options: [
      { label: "Tabs", description: "Indent with tab characters." },
      { label: "Spaces", description: "Indent with space characters." },
    ],
  }],
};
