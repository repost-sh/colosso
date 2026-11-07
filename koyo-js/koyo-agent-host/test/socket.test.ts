// A socket in a folder deeper than a socket's address holds, as the checkout's scratch folder of
// a checkout deep enough is: the host listens there, and is reached there, all the same.

import { existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { afterEach, expect, test } from "bun:test";

import { SOCKET_PATH_MAX, socketAddress } from "../src/socket";
import { removeScratch, scratch } from "./support/scratch";

afterEach(removeScratch);

test("a socket too deep for its address is bound and reached where it is", async () => {
  // Deep enough whatever the checkout's path, and still a short way from the workspace.
  const dir = join(scratch(), "d".repeat(60));
  mkdirSync(dir);
  const path = join(dir, "host.sock");
  expect(Buffer.byteLength(path)).toBeGreaterThan(SOCKET_PATH_MAX);
  expect(Buffer.byteLength(socketAddress(path))).toBeLessThanOrEqual(SOCKET_PATH_MAX);
  const server = Bun.listen({
    unix: socketAddress(path),
    socket: {
      data(socket, chunk) {
        socket.write(chunk);
      },
    },
  });
  expect(existsSync(path) && statSync(path).isSocket()).toBe(true);
  const heard = Promise.withResolvers<string>();
  const client = await Bun.connect({
    unix: socketAddress(path),
    socket: {
      data(_socket, chunk) {
        heard.resolve(chunk.toString());
      },
    },
  });
  client.write("ping");
  expect(await heard.promise).toBe("ping");
  client.end();
  server.stop(true);
});

test("a path an address holds is its own address", () => {
  expect(socketAddress("/data/agents/host.sock")).toBe("/data/agents/host.sock");
});
