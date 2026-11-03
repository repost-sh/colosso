import { describe, expect, test } from "bun:test";

import { LineReader } from "../src/server/lines";

const encoder = new TextEncoder();
const bytes = (text: string): Uint8Array => encoder.encode(text);

describe("LineReader", () => {
  test("gives the lines a piece ends and keeps the rest for the next", () => {
    const reader = new LineReader(64);
    expect(reader.push(bytes("one\ntw"))).toStrictEqual({ kind: "lines", lines: ["one"] });
    expect(reader.push(bytes("o\n\nthree"))).toStrictEqual({ kind: "lines", lines: ["two", ""] });
    expect(reader.push(bytes("\n"))).toStrictEqual({ kind: "lines", lines: ["three"] });
  });

  test("decodes a character whose bytes two pieces share", () => {
    const reader = new LineReader(64);
    const line = bytes("ensō\n");
    expect(reader.push(line.subarray(0, 4))).toStrictEqual({ kind: "lines", lines: [] });
    expect(reader.push(line.subarray(4))).toStrictEqual({ kind: "lines", lines: ["ensō"] });
  });

  test("holds a line to its bound, in one piece or several", () => {
    expect(new LineReader(4).push(bytes("abcd\n"))).toStrictEqual({
      kind: "lines",
      lines: ["abcd"],
    });
    expect(new LineReader(4).push(bytes("abcde\n"))).toStrictEqual({ kind: "too_long" });
    const reader = new LineReader(4);
    expect(reader.push(bytes("abc"))).toStrictEqual({ kind: "lines", lines: [] });
    expect(reader.push(bytes("de"))).toStrictEqual({ kind: "too_long" });
  });

  test("doesn't keep a piece the runtime may reuse", () => {
    const reader = new LineReader(64);
    const piece = bytes("abc");
    reader.push(piece);
    piece.fill(0x7A);
    expect(reader.push(bytes("\n"))).toStrictEqual({ kind: "lines", lines: ["abc"] });
  });
});
