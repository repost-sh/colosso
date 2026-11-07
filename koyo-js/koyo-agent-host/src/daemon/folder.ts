// The host's folder (docs/chat.md 4.1), `settings::data_dir()/agents/`, which the app names
// with `--dir`: who may use the host is who may enter it (0700), as Bun can't ask a socket's
// peer who it is; the socket is trusted only while the lock beside it is held.

import { chmodSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";

import { SOCKET_PATH_MAX, socketAddress } from "../socket";
import { type Lock, takeLock } from "./lock";

export interface Folder {
  readonly root: string;
  readonly socket: string;
  readonly lock: string;
  readonly log: string;
  readonly database: string;
  readonly attachments: string;
  /** The app's drafts, which name attachments no message does yet. */
  readonly drafts: string;
}

export function folder(root: string): Folder {
  return {
    root,
    socket: join(root, "host.sock"),
    lock: join(root, "host.lock"),
    log: join(root, "host.log"),
    database: join(root, "chats.sqlite"),
    attachments: join(root, "attachments"),
    drafts: join(root, "chat-drafts.json"),
  };
}

/**
 * Makes the folder this user's alone and takes its lock, then clears the way for the socket: a
 * socket left there was a host's that died, since the lock is ours. Answers who holds the lock
 * when another host does.
 */
export function claim(dir: Folder): Lock {
  if (Buffer.byteLength(socketAddress(dir.socket)) > SOCKET_PATH_MAX) {
    const where = `${dir.socket}, even from ${process.cwd()},`;
    throw new Error(`The socket path ${where} is too long; use a shorter KOYO_DATA_DIR.`);
  }
  mkdirSync(dir.root, { recursive: true, mode: 0o700 });
  chmodSync(dir.root, 0o700);
  const lock = takeLock(dir.lock);
  if (lock.kind === "held") {
    rmSync(dir.socket, { force: true });
  }
  return lock;
}
