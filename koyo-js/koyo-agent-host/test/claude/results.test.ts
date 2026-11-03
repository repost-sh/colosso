import { describe, expect, test } from "bun:test";

import type { Usage } from "../../src/protocol/model";
import {
  classify,
  compactPoint,
  type Signs,
  summed,
} from "../../src/providers/claude/results";
import { failed, MODEL, success, WINDOW } from "../fixtures/claude/frames";

const QUIET: Signs = { authFailed: false, overloaded: false, limited: null, interrupted: false };

describe("a result", () => {
  test("that completed completes the turn", () => {
    expect(classify(success(["p1"]), QUIET)).toEqual({ outcome: "completed", failure: null });
  });

  test("of subtype success with is_error fails it, whatever its subtype says", () => {
    const text = "API Error: 400 bad request";
    const result = success(["p1"], { isError: true, text });
    expect(classify(result, QUIET)).toEqual({
      outcome: "failed",
      failure: { kind: "api", message: "API Error: 400 bad request", retryAt: null },
    });
  });

  test("with an API status of 429 is a usage limit, with the time a rate-limit event gave", () => {
    const result = success(["p1"], { apiStatus: 429, text: "Rate limited" });
    const retryAt = "2026-11-05T08:00:00.000Z";
    expect(classify(result, { ...QUIET, limited: { retryAt } })).toEqual({
      outcome: "failed",
      failure: { kind: "usage_limit", message: "Claude's usage limit is reached.", retryAt },
    });
  });

  test("with an API status of 529 is Claude overloaded", () => {
    expect(classify(success(["p1"], { apiStatus: 529 }), QUIET).failure?.kind).toBe("overloaded");
  });

  test("of an error terminal reason fails, even when nothing else says so", () => {
    expect(classify(success(["p1"], { terminal: "prompt_too_long", text: "" }), QUIET)).toEqual({
      outcome: "failed",
      failure: { kind: "api", message: "Claude stopped: prompt too long.", retryAt: null },
    });
    expect(classify(success(["p1"], { terminal: "blocking_limit" }), QUIET).failure?.kind)
      .toBe("usage_limit");
  });

  test("after a login failure reads Sign in to Claude again", () => {
    const result = success(["p1"], { isError: true, text: "Invalid API key" });
    expect(classify(result, { ...QUIET, authFailed: true }).failure).toEqual({
      kind: "auth",
      message: "Sign in to Claude again.",
      retryAt: null,
    });
  });

  test("aborted while streaming or running tools interrupts the turn", () => {
    // As recorded: a stop while the model streamed.
    const streaming = failed(["p1"], ["[ede_diagnostic] result_type=user"], "aborted_streaming");
    expect(classify(streaming, QUIET)).toEqual({ outcome: "interrupted", failure: null });
    expect(classify(failed(["p1"], [], "aborted_tools"), QUIET).outcome).toBe("interrupted");
  });

  test("of an error during a stop the person asked for interrupts the turn", () => {
    expect(classify(failed(["p1"], ["Request was aborted."]), { ...QUIET, interrupted: true }))
      .toEqual({ outcome: "interrupted", failure: null });
  });

  test("of an error says what went wrong, without the CLI's diagnostic lines", () => {
    const result = failed(["p1"], ["[ede_diagnostic] result_type=user", "The tool crashed."]);
    expect(classify(result, QUIET).failure).toEqual({
      kind: "api",
      message: "The tool crashed.",
      retryAt: null,
    });
  });
});

describe("the context window", () => {
  test("is the model's, less the 33k Claude Code keeps to compact in", () => {
    expect(compactPoint(success(["p1"]).modelUsage, MODEL)).toBe(WINDOW - 33_000);
    expect(compactPoint(success(["p1"], { window: 200_000 }).modelUsage, "other")).toBe(167_000);
    expect(compactPoint({}, MODEL)).toBeNull();
  });
});

describe("usage", () => {
  test("of a turn a steer continued is both results'", () => {
    const one: Usage = {
      inputTokens: 2,
      outputTokens: 100,
      cacheReadTokens: 0,
      cacheCreationTokens: 28_000,
      costUsd: 0.01,
      contextTokens: 28_102,
      contextWindow: 967_000,
    };
    const two: Usage = { ...one, inputTokens: 3, costUsd: 0.02, contextTokens: 28_300 };
    expect(summed(one, two)).toEqual({
      inputTokens: 5,
      outputTokens: 200,
      cacheReadTokens: 0,
      cacheCreationTokens: 56_000,
      costUsd: 0.03,
      contextTokens: 28_300,
      contextWindow: 967_000,
    });
  });
});
