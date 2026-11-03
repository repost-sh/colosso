// The files messages attach (docs/chat.md 4.3.4): the app writes them into the host's
// attachments folder and names them by file, so a name is checked to be one of that folder's
// own files before an agent is given its path, and goes with its chat.

import { lstatSync, rmSync } from "node:fs";
import { basename, join } from "node:path";

import type { Attachment } from "../protocol/model";
import { HostError } from "../protocol/wire";

/** Refuses an attachment that isn't a plain file of `folder`'s own (no `..`, no links out). */
export function checkAttachments(folder: string, attachments: readonly Attachment[]): void {
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
