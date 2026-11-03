import { describe, expect, test } from "bun:test";

import { Outbox, type Writable } from "../src/server/outbox";

/** A socket that takes `room` bytes, then nothing until it's given more room. */
class Narrow implements Writable {
  readonly taken: number[] = [];
  room: number;

  constructor(room: number) {
    this.room = room;
  }

  readonly write = (given: Uint8Array): number => {
    const took = Math.min(this.room, given.length);
    this.room -= took;
    this.taken.push(...given.subarray(0, took));
    return took;
  };
}

function bytes(...values: number[]): Uint8Array {
  return Uint8Array.from(values);
}

describe("an outbox", () => {
  test("writes at once what the socket takes, and the rest in order as it takes more", () => {
    const socket = new Narrow(3);
    const outbox = new Outbox();
    outbox.push(bytes(1, 2), socket);
    outbox.push(bytes(3, 4, 5), socket);
    outbox.push(bytes(6), socket);
    expect(socket.taken).toStrictEqual([1, 2, 3]);
    expect(outbox.waiting).toBe(3);
    socket.room = 2;
    outbox.drain(socket);
    expect(socket.taken).toStrictEqual([1, 2, 3, 4, 5]);
    expect(outbox.waiting).toBe(1);
    socket.room = 10;
    outbox.drain(socket);
    expect(socket.taken).toStrictEqual([1, 2, 3, 4, 5, 6]);
    expect(outbox.waiting).toBe(0);
  });

  test("waits for a drain once something waits, so what comes later can't pass it", () => {
    const socket = new Narrow(1);
    const outbox = new Outbox();
    outbox.push(bytes(1, 2), socket);
    socket.room = 10;
    outbox.push(bytes(3), socket);
    expect(socket.taken).toStrictEqual([1]);
    outbox.drain(socket);
    expect(socket.taken).toStrictEqual([1, 2, 3]);
  });
});
