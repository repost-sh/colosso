// A connection's bytes cut into lines (docs/chat.md 4.3): what arrives comes in pieces of any
// size, a character's bytes may straddle two of them, and a line may not grow past a bound.

const NEWLINE = 0x0A;

/** What a piece of input completed: the lines, or that one grew past the bound. */
export type Lines = { readonly kind: "lines"; readonly lines: readonly string[] }
  | { readonly kind: "too_long" };

export class LineReader {
  readonly #max: number;
  readonly #decoder = new TextDecoder();
  /** The start of a line whose end hasn't come yet. */
  #pending: Uint8Array[] = [];
  #pendingBytes = 0;

  constructor(maxLineBytes: number) {
    this.#max = maxLineBytes;
  }

  /** The lines `chunk` ends, without their newlines. */
  push(chunk: Uint8Array): Lines {
    const lines: string[] = [];
    let start = 0;
    let end = chunk.indexOf(NEWLINE);
    while (end !== -1) {
      if (this.#pendingBytes + end - start > this.#max) {
        return { kind: "too_long" };
      }
      lines.push(this.#take(chunk.subarray(start, end)));
      start = end + 1;
      end = chunk.indexOf(NEWLINE, start);
    }
    const rest = chunk.subarray(start);
    if (this.#pendingBytes + rest.length > this.#max) {
      return { kind: "too_long" };
    }
    if (rest.length > 0) {
      // A copy: the runtime may reuse the chunk's memory for the next read.
      this.#pending.push(rest.slice());
      this.#pendingBytes += rest.length;
    }
    return { kind: "lines", lines };
  }

  #take(last: Uint8Array): string {
    if (this.#pending.length === 0) {
      return this.#decoder.decode(last);
    }
    const line = new Uint8Array(this.#pendingBytes + last.length);
    let offset = 0;
    for (const piece of this.#pending) {
      line.set(piece, offset);
      offset += piece.length;
    }
    line.set(last, offset);
    this.#pending = [];
    this.#pendingBytes = 0;
    return this.#decoder.decode(line);
  }
}
