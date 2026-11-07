// Reaching a socket whatever folder it's in (docs/chat.md 4.1). A socket's address holds at most
// 103 bytes of path on macOS, and the host's folder can be deeper than that leaves room for: a
// checkout's own scratch folder, or a long `KOYO_DATA_DIR`. The way there from the working
// directory is often short when the absolute path isn't, and the system takes it the same: the
// host runs in its folder, so it binds its socket by name alone, and a test runs in the
// workspace, inside the checkout.

import { realpathSync } from "node:fs";
import { basename, dirname, join, relative } from "node:path";

/** The most bytes of path a socket address holds, its terminating zero left out. */
export const SOCKET_PATH_MAX = 103;

/**
 * How to reach the socket at `path`: `path` itself when an address holds it, else the way there
 * from the working directory when an address holds that, else `path`, which the system refuses.
 */
export function socketAddress(path: string): string {
  if (Buffer.byteLength(path) <= SOCKET_PATH_MAX) {
    return path;
  }
  const way = relative(process.cwd(), physical(path));
  return Buffer.byteLength(way) <= SOCKET_PATH_MAX ? way : path;
}

/**
 * `path` with its longest ancestor that exists resolved, links and all, as the working directory
 * always is; what's left of it, not made yet, is added as it is.
 */
function physical(path: string): string {
  const rest: string[] = [];
  let at = path;
  while (dirname(at) !== at) {
    const real = realOrNot(at);
    if (real !== undefined) {
      return join(real, ...rest.toReversed());
    }
    rest.push(basename(at));
    at = dirname(at);
  }
  return path;
}

function realOrNot(path: string): string | undefined {
  try {
    return realpathSync(path);
  } catch {
    return undefined;
  }
}
