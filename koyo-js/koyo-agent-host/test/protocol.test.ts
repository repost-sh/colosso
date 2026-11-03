// The protocol's golden fixtures (docs/chat.md C8): a request and its result for every method,
// a line for every event, and an error. Each parses, and encodes again to exactly what it
// was, so neither side drops or reshapes a field; the app's serde types are held to the same
// files.

import { readdirSync } from "node:fs";

import { describe, expect, test } from "bun:test";
import { z } from "zod";

import { events } from "../src/protocol/events";
import { methods } from "../src/protocol/methods";
import { isMethod, readLine, WireError } from "../src/protocol/wire";

const FIXTURES = new URL("fixtures/protocol/", import.meta.url);

const MethodFixture = z.object({
  request: z.object({ id: z.int(), method: z.string(), params: z.json() }),
  response: z.object({ id: z.int(), result: z.json() }),
});
const EventFixture = z.object({ event: z.string(), params: z.json() });

async function fixture(path: string): Promise<unknown> {
  const json: unknown = JSON.parse(await Bun.file(new URL(path, FIXTURES)).text());
  return json;
}

/** `value` as `schema` reads it, encoded again. */
function reencoded(schema: z.ZodType, value: unknown): unknown {
  const json: unknown = JSON.parse(JSON.stringify(schema.parse(value)));
  return json;
}

/** The names the fixtures in `folder` are for. */
function named(folder: string): string[] {
  return sorted(readdirSync(new URL(folder, FIXTURES)).map((file) => file.replace(/\.json$/u, "")));
}

function sorted(names: string[]): string[] {
  return names.toSorted((a, b) => a.localeCompare(b));
}

describe("methods", () => {
  test("every method has a fixture, and every fixture a method", () => {
    expect(named("methods/")).toStrictEqual(sorted(Object.keys(methods)));
  });

  for (const method of Object.keys(methods).filter(isMethod)) {
    const spec = methods[method];
    test(method, async () => {
      const { request, response } = MethodFixture.parse(await fixture(`methods/${method}.json`));
      expect(request.method).toBe(method);
      expect(response.id).toBe(request.id);
      expect(readLine(JSON.stringify(request))).toStrictEqual({
        kind: "request",
        id: request.id,
        method,
        params: request.params,
      });
      expect(reencoded(spec.params, request.params)).toStrictEqual(request.params);
      expect(reencoded(spec.result, response.result)).toStrictEqual(response.result);
    });
  }
});

describe("events", () => {
  test("every event has a fixture, and every fixture an event", () => {
    expect(named("events/")).toStrictEqual(sorted(Object.keys(events)));
  });

  for (const [event, schema] of Object.entries(events)) {
    test(event, async () => {
      const line = EventFixture.parse(await fixture(`events/${event}.json`));
      expect(line.event).toBe(event);
      expect(reencoded(schema, line.params)).toStrictEqual(line.params);
    });
  }
});

test("an error", async () => {
  const line = z.object({ id: z.int(), error: z.json() }).parse(await fixture("error.json"));
  expect(reencoded(WireError, line.error)).toStrictEqual(line.error);
});
