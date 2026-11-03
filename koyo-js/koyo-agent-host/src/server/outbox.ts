// What's to go out on a socket that can't take it all at once: queued in order, written as the
// socket takes more, so a large snapshot or a burst of appends is never cut short. It counts
// what waits, so a client that stops reading can be told apart from one that's slow.

/** Where an outbox writes: as much of `bytes` as it takes, and how much that was. */
export interface Writable {
  readonly write: (bytes: Uint8Array) => number;
}

export class Outbox {
  /** What the socket hasn't taken yet, from `#head` on, oldest first. */
  #queued: Uint8Array[] = [];
  #head = 0;
  #waiting = 0;

  /** How many bytes wait. */
  get waiting(): number {
    return this.#waiting;
  }

  /** Writes `bytes` after what waits, as much as `socket` takes now. */
  push(bytes: Uint8Array, socket: Writable): void {
    this.#queued.push(bytes);
    this.#waiting += bytes.length;
    if (this.#head === this.#queued.length - 1) {
      this.drain(socket);
    }
  }

  /** Writes what waits, as much as `socket` takes; it says when it takes more. */
  drain(socket: Writable): void {
    let bytes = this.#queued.at(this.#head);
    while (bytes !== undefined) {
      // A closed socket takes nothing, and its close follows.
      const written = Math.max(socket.write(bytes), 0);
      this.#waiting -= Math.min(written, bytes.length);
      if (written < bytes.length) {
        this.#queued[this.#head] = bytes.subarray(written);
        return;
      }
      this.#head += 1;
      bytes = this.#queued.at(this.#head);
    }
    this.clear();
  }

  clear(): void {
    this.#queued = [];
    this.#head = 0;
    this.#waiting = 0;
  }
}
