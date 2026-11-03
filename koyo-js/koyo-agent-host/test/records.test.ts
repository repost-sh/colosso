import { describe, expect, test } from "bun:test";

import { titleFrom } from "../src/threads/records";

describe("a title from a model's reply", () => {
  test("is the reply's first line, unquoted, at most six words", () => {
    expect(titleFrom("\n  **Fix the flaky login test in CI today please**\n"))
      .toBe("Fix the flaky login test in");
    expect(titleFrom("'Rename the store.'")).toBe("Rename the store");
  });

  test("is none when nothing is left", () => {
    expect(titleFrom("  \n")).toBeUndefined();
    expect(titleFrom("\"**\"")).toBeUndefined();
  });
});
