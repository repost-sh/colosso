// The files messages attach (docs/chat.md 4.3.4): the app writes them into the host's
// attachments folder and names them by file, so a name is checked to be one of that folder's
// own files before an agent is given its path, and goes with its chat. One neither a message nor
// the app's drafts ended up naming, the app having stopped before it could remove it, is swept up
// as the host starts.

import { existsSync, lstatSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { basename, join } from "node:path";

import { z } from "zod";

import type { Attachment } from "../protocol/model";
import { HostError } from "../protocol/wire";

/** How long a file nothing names stays, for a message still being written. */
const UNNAMED_FOR_MS = 7 * 24 * 60 * 60 * 1000;

/** What the app writes of its drafts that matters here: each one's chat, then the draft. */
const Drafts = z.array(
  z.tuple([z.unknown(), z.object({ attachments: z.array(z.object({ file: z.string() })) })]),
);

/**
 * Refuses a message with neither words nor files, and an attachment that isn't a plain file of
 * `folder`'s own (no `..`, no links out).
 */
export function checkMessage(
  folder: string,
  text: string,
  attachments: readonly Attachment[],
): void {
  if (text.trim() === "" && attachments.length === 0) {
    throw new HostError("bad_request", "A message needs words or a file.");
  }
  for (const { file } of attachments) {
    if (!isPlainName(file) || !isFile(join(folder, file))) {
      throw new HostError("bad_request", `\`${file}\` isn't a file in the attachments folder.`);
    }
  }
}

/** Removes the files a deleted chat attached. */
export function removeAttachments(folder: string, files: readonly string[]): void {
  for (const file of files.filter(isPlainName)) {
    rmSync(join(folder, file), { force: true });
  }
}

/**
 * The files the app's drafts at `path` name, which stay for as long as they do. A file there that
 * can't be read throws, as nothing can be said to be unnamed then.
 */
export function draftAttachments(path: string): ReadonlySet<string> {
  if (!existsSync(path)) {
    return new Set();
  }
  const drafts = Drafts.parse(JSON.parse(readFileSync(path, "utf8")));
  return new Set(drafts.flatMap(([, draft]) => draft.attachments.map(({ file }) => file)));
}

/**
 * Removes the files of `folder` nothing names, once they're old enough at `now` not to be on
 * their way into a message. Their age is from when they were made here, as a copy keeps its
 * modified time from the file it was made of.
 */
export function sweepAttachments(folder: string, named: ReadonlySet<string>, now: number): void {
  if (!existsSync(folder)) {
    return;
  }
  const unnamed = readdirSync(folder).filter((file) => !named.has(file));
  for (const path of unnamed.map((file) => join(folder, file))) {
    const stat = lstatSync(path);
    if (stat.isFile() && now - stat.ctimeMs > UNNAMED_FOR_MS) {
      rmSync(path, { force: true });
    }
  }
}

function isPlainName(file: string): boolean {
  return file !== "." && file !== ".." && basename(file) === file;
}

function isFile(path: string): boolean {
  try {
    return lstatSync(path).isFile();
  } catch {
    return false;
  }
}
