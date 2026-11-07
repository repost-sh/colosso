import { afterEach, describe, expect, test } from "bun:test";

import { Background } from "../src/providers/claude/background";
import { bench, closeBenches, Sink } from "./claude/support";
import { init } from "./fixtures/claude/frames";
import { textTurn } from "./fixtures/claude/sessions";
import { backgroundTasks as listed } from "./fixtures/claude/tasks";
import { closeHarnesses, harness, running } from "./support/actor";
import { thread } from "./support/records";
import { settle } from "./support/wait";

afterEach(closeHarnesses);
afterEach(closeBenches);

describe("what Claude runs in the background", () => {
  test("is counted from the CLI's list, leaving out what isn't activity, as it changes", () => {
    const sink = new Sink();
    const background = new Background(sink.event);
    expect(background.hears(init("default"))).toBe(false);
    expect(background.hears(listed({ type: "local_bash" }, { type: "monitor" }))).toBe(true);
    // A watcher the CLI started on its own isn't the agent's work.
    background.hears(listed({ type: "local_bash" }, { type: "monitor" }, {
      type: "monitor",
      ambient: true,
    }));
    background.hears(listed({ type: "monitor" }));
    background.reset();
    background.reset();
    expect(sink.of("background")).toStrictEqual([
      { type: "background", tasks: 2 },
      { type: "background", tasks: 1 },
      { type: "background", tasks: 0 },
    ]);
  });
});

describe("Claude's session", () => {
  test("says what runs in the background, during its turn or after it", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(listed({ type: "local_bash" }));
    query.send(...textTurn(promptId));
    // The turn's over; the CLI lists what's left as a Monitor starts.
    query.send(listed({ type: "local_bash" }, { type: "monitor" }));
    await settle();
    expect(made.sink.of("background")).toStrictEqual([
      { type: "background", tasks: 1 },
      { type: "background", tasks: 2 },
    ]);
  });
});

describe("a chat's background", () => {
  test("is what its session last said, past its turn, and none once it's gone", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.report({ type: "background", tasks: 2 });
    session.finished();
    await settle();
    expect(chats.actor.summary().status).toBe("idle");
    expect(chats.actor.thread.backgroundTasks).toBe(2);
    expect(chats.store.thread(chats.id)?.backgroundTasks).toBe(2);

    session.report({ type: "session.ended", error: null });
    await settle();
    expect(chats.actor.thread.backgroundTasks).toBe(0);
  });

  test("is none for a chat a host that stopped left with some", () => {
    const chats = harness({}, { thread: { ...thread(), backgroundTasks: 3 } });
    expect(chats.actor.thread.backgroundTasks).toBe(0);
  });
});
