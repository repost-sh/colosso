// The host's executable (docs/chat.md 4.1): `koyo-agent-host --dir <folder>`, the folder
// `settings::data_dir()/agents`, which the app starts and waits for the `ready` line of.
// `--foreground` logs to stderr rather than the folder's log, and `KOYO_LOG` says how much.
// `KOYO_AGENT_PROVIDER=fake` puts the fake provider, its sessions answering by themselves, in
// Claude's place, for the host run end to end (docs/chat.md 6).

import { parseArgs } from "node:util";

import { run } from "./daemon/daemon";
import { errorMessage, levelNamed, log, setLevel } from "./log";
import { ClaudeProvider } from "./providers/claude/provider";
import { answer, FakeProvider } from "./providers/fake";

const USAGE = "usage: koyo-agent-host --dir <folder> [--foreground]";

interface Args {
  readonly dir: string;
  readonly foreground: boolean;
}

/** The command line, or `undefined` when it isn't one the host takes. */
function args(): Args | undefined {
  try {
    const { values } = parseArgs({
      options: { dir: { type: "string" }, foreground: { type: "boolean" } },
    });
    return values.dir === undefined
      ? undefined
      : { dir: values.dir, foreground: values.foreground === true };
  } catch (error) {
    log.error(errorMessage(error));
    return undefined;
  }
}

const level = levelNamed(process.env["KOYO_LOG"] ?? "");
if (level !== undefined) {
  setLevel(level);
}
const given = args();
if (given === undefined) {
  log.error(USAGE);
  process.exit(2);
}
const fake = process.env["KOYO_AGENT_PROVIDER"] === "fake";
await run({
  root: given.dir,
  foreground: given.foreground,
  providers: [fake ? new FakeProvider({ onStart: answer }) : new ClaudeProvider()],
  ready: () => {
    process.stdout.write("ready\n");
  },
});
// Its sessions and timers gone, the host exits rather than wait on what a library left open.
process.exit(0);
