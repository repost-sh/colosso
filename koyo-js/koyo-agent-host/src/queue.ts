// Values one side gives and another reads, in the order given, as an async iterable: the
// prompts a session feeds its CLI (docs/chat.md 4.7) are one. A reader that comes before a
// value waits for it; once the queue ends, what's queued is still read, then the end, or
// the failure it ended with.

export class Queue<T extends object> implements AsyncIterable<T> {
  readonly #queued: T[] = [];
  readonly #waiting: PromiseWithResolvers<IteratorResult<T, undefined>>[] = [];
  #ended = false;
  #failure: Error | undefined;

  /** Gives `value`, or nothing once the queue ended. */
  push(value: T): void {
    if (this.#ended) {
      return;
    }
    const reader = this.#waiting.shift();
    if (reader === undefined) {
      this.#queued.push(value);
    } else {
      reader.resolve({ value, done: false });
    }
  }

  /** Ends the queue, as having failed with `failure` when one is given; the first end holds. */
  end(failure?: Error): void {
    if (this.#ended) {
      return;
    }
    this.#ended = true;
    this.#failure = failure;
    for (const reader of this.#waiting.splice(0)) {
      if (failure === undefined) {
        reader.resolve({ value: undefined, done: true });
      } else {
        reader.reject(failure);
      }
    }
  }

  /** The next value, once it's given; the end once the queue ended. */
  async next(): Promise<IteratorResult<T, undefined>> {
    const value = this.#queued.shift();
    if (value !== undefined) {
      return { value, done: false };
    }
    if (this.#failure !== undefined) {
      throw this.#failure;
    }
    if (this.#ended) {
      return { value: undefined, done: true };
    }
    const reader = Promise.withResolvers<IteratorResult<T, undefined>>();
    this.#waiting.push(reader);
    return await reader.promise;
  }

  [Symbol.asyncIterator](): AsyncIterator<T, undefined> {
    return { next: async () => await this.next() };
  }
}
