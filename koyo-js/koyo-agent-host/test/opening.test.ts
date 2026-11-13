import { describe, expect, test } from "bun:test";

import { opening } from "../src/threads/records";

describe("what a chat's row says was said last", () => {
  test("is the first line with words, without markdown's marks, spaces closed up", () => {
    expect(opening("Find the flaky test.")).toBe("Find the flaky test.");
    expect(opening("\n\n## Done\n\nThe rest.")).toBe("Done");
    expect(opening("> **Note:** the `cache`   is   cold")).toBe("Note: the cache is cold");
    expect(opening("- first item\n- second")).toBe("first item");
    expect(opening("1. Run it")).toBe("Run it");
    expect(opening("keeps snake_case and a * b")).toBe("keeps snake_case and a * b");
  });

  test("is cut to 200 characters, and none without words", () => {
    expect(opening("word ".repeat(100))).toHaveLength(200);
    expect(opening("  \n \t\n")).toBeNull();
    expect(opening("")).toBeNull();
  });
});
