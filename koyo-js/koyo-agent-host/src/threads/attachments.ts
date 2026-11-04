// The files messages attach (docs/chat.md 4.3.4): the app writes them into the host's
// attachments folder and names them by file, so a name is checked to be one of that folder's
// own files before an agent is given its path, and goes with its chat. One no message ended up
// naming, the app having stopped before it could remove it, is swept up as the host starts.

import { existsSync, lstatSync, readdirSync, rmSync } from "node:fs";
import { basename, join } from "node:path";

import type { Attachment } from "../protocol/model";
import { HostError } from "../protocol/wire";

/** How long a file no message names stays, for a message still being written. */
const UNNAMED_FOR_MS = 7 * 24 * 60 * 60 * 1000;

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
 * Removes the files of `folder` no message names, once they're old enough at `now` not to be on
 * their way into one. Their age is from when they were made here, as a copy keeps its modified
 * time from the file it was made of.
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
