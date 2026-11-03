// Claude as the host's first provider (docs/chat.md C6, C7, 4.6): sessions on the person's own
// `claude`, through the official Agent SDK, in their login environment; its status, catalog
// and chat titles. Of Koyo's settings (`setConfig`), it follows where `claude` is.

import {
  getSessionMessages,
  type PermissionMode,
  query,
  resolveSettings,
  type SDKControlInitializeResponse,
} from "@anthropic-ai/claude-agent-sdk";

import { errorMessage, log } from "../../log";
import type { Catalog, Command, ProviderStatus } from "../../protocol/methods";
import type {
  Capabilities,
  Config,
  Provider,
  ProviderSession,
  SessionSink,
  SessionSpec,
} from "../provider";
import { commands, MISSING, models, probe, readyStatus } from "./catalog";
import { type Env, findClaude, loginEnvironment, Versions } from "./environment";
import { modeOf, titleOptions } from "./options";
import { ClaudeSession, type Launch, type Start } from "./session";
import { Groups } from "./spawn";

/** What Claude needs from the machine, which tests give it their own of. */
export interface ClaudeDeps {
  /** The environment the CLI runs in, read once. */
  readonly environment: () => Promise<Env>;
  readonly start: Start;
  /** Where `claude` is, Koyo's setting first. */
  readonly find: (setting: string | null, env: Env) => string | undefined;
  readonly version: (executable: string, env: Env) => Promise<string | null>;
  /** The default mode Claude's own settings name for `cwd`. */
  readonly settingsMode: (cwd: string) => Promise<PermissionMode | undefined>;
  /** Whether a conversation has turns on disk to resume. */
  readonly resumable: (sessionId: string, cwd: string) => Promise<boolean>;
  /** How long a stopped turn has to end before its process is closed. */
  readonly stopMs: number;
}

/** How long a title may take. */
const TITLE_MS = 60_000;
const MISSING_CLI = "Claude Code isn't installed. Install it, then send the message again.";

const versions = new Versions();

const DEFAULTS: ClaudeDeps = {
  environment: async () => await loginEnvironment(),
  start: (params) => query(params),
  find: findClaude,
  version: async (executable, env) => await versions.of(executable, env),
  settingsMode,
  resumable: hasTurns,
  stopMs: 10_000,
};

interface Found {
  readonly executable: string;
  readonly version: string | null;
  readonly env: Env;
}

export class ClaudeProvider implements Provider {
  readonly id = "claude";
  readonly capabilities: Capabilities = {
    steer: true,
    resume: true,
    images: true,
    compact: true,
    plan: true,
  };

  readonly #deps: ClaudeDeps;
  readonly #env: Promise<Env>;
  /** Where Koyo's setting says `claude` is, if it says. */
  #claudePath: string | null = null;
  /** The CLI's initialize answers, by executable and version. */
  readonly #inits = new Map<string, Promise<SDKControlInitializeResponse>>();
  /** The probes running, by the folder each runs in, which another probe there shares. */
  readonly #probes = new Map<string, Promise<SDKControlInitializeResponse>>();
  /** The slash commands of each folder Claude ran in. */
  readonly #commands = new Map<string, Command[]>();
  /** The conversations found to have turns, which they keep: transcripts only grow. */
  readonly #resumable = new Set<string>();

  constructor(deps: Partial<ClaudeDeps> = {}) {
    this.#deps = { ...DEFAULTS, ...deps };
    // Read once, at start (4.2).
    this.#env = this.#deps.environment();
  }

  /** Takes Koyo's settings, of which Claude follows where `claude` is. */
  setConfig(config: Config): void {
    this.#claudePath = config.claudePath;
  }

  async status(refresh: boolean): Promise<ProviderStatus> {
    const found = await this.#found();
    if (found === undefined) {
      return MISSING;
    }
    try {
      return readyStatus(found, await this.#init(found, process.cwd(), refresh));
    } catch (error) {
      log.warn("Claude's status couldn't be read", error);
      return {
        provider: this.id,
        state: "error",
        executable: found.executable,
        version: found.version,
        account: null,
        message: errorMessage(error),
      };
    }
  }

  /** What Claude offers in `cwd`, and the default mode Claude's own settings name there. */
  async catalog(cwd: string): Promise<Catalog> {
    const [settings, found] = await Promise.all([this.#deps.settingsMode(cwd), this.#found()]);
    const defaultMode = modeOrNull(settings);
    if (found === undefined) {
      return { models: [], commands: [], defaultMode };
    }
    const init = this.#commands.has(cwd)
      ? await this.#init(found, cwd, false)
      : await this.#probe(found, cwd);
    return { models: models(init.models), commands: this.#commands.get(cwd) ?? [], defaultMode };
  }

  open(spec: SessionSpec, sink: SessionSink): ProviderSession {
    return new ClaudeSession(spec, sink, {
      launch: async (opening) => await this.#launch(opening),
      start: this.#deps.start,
      commands: (cwd, list) => {
        this.#commands.set(cwd, commands(list));
      },
      stopMs: this.#deps.stopMs,
    });
  }

  /**
   * The reply of the smallest model asked for a title, in one turn with no tools and nothing
   * kept (4.5.7).
   */
  async title(prompt: string, cwd: string): Promise<string | undefined> {
    const found = await this.#found();
    if (found === undefined) {
      return undefined;
    }
    const groups = new Groups("Claude's title run");
    const conversation = this.#deps.start({
      prompt: titlePrompt(prompt),
      options: titleOptions({
        executable: found.executable,
        env: found.env,
        cwd,
        spawn: groups.spawn,
      }),
    });
    const timer = setTimeout(() => {
      conversation.close();
    }, TITLE_MS);
    let reply: string | undefined;
    try {
      for await (const frame of conversation) {
        if (frame.type === "result") {
          reply = frame.subtype === "success" && !frame.is_error ? frame.result : undefined;
          break;
        }
      }
    } finally {
      clearTimeout(timer);
      conversation.close();
      await groups.terminate();
    }
    return reply;
  }

  async #found(): Promise<Found | undefined> {
    const env = await this.#env;
    const executable = this.#deps.find(this.#claudePath, env);
    return executable === undefined
      ? undefined
      : { executable, version: await this.#deps.version(executable, env), env };
  }

  async #launch(spec: SessionSpec): Promise<Launch> {
    const [found, resumable] = await Promise.all([this.#found(), this.#hasTurns(spec)]);
    if (found === undefined) {
      throw new Error(MISSING_CLI);
    }
    return { executable: found.executable, env: found.env, resumable };
  }

  /** Whether the conversation `spec` resumes has turns on disk, read until it's found to. */
  async #hasTurns(spec: SessionSpec): Promise<boolean> {
    if (!spec.resume || this.#resumable.has(spec.sessionId)) {
      return spec.resume;
    }
    const has = await this.#deps.resumable(spec.sessionId, spec.cwd);
    if (has) {
      this.#resumable.add(spec.sessionId);
    }
    return has;
  }

  /** The CLI's initialize answer for this executable and version, probing for it once. */
  async #init(found: Found, cwd: string, refresh: boolean): Promise<SDKControlInitializeResponse> {
    const known = refresh ? undefined : this.#inits.get(key(found));
    return await (known ?? this.#probe(found, cwd));
  }

  /** Probes the CLI in `cwd`, which also tells that folder's slash commands. */
  async #probe(found: Found, cwd: string): Promise<SDKControlInitializeResponse> {
    const running = this.#probes.get(cwd);
    if (running !== undefined) {
      return await running;
    }
    const probing = probe(this.#deps.start, { executable: found.executable, env: found.env, cwd });
    this.#probes.set(cwd, probing);
    this.#inits.set(key(found), probing);
    try {
      const init = await probing;
      this.#commands.set(cwd, commands(init.commands));
      return init;
    } catch (error) {
      this.#inits.delete(key(found));
      throw error;
    } finally {
      this.#probes.delete(cwd);
    }
  }
}

function key(found: Found): string {
  return `${found.executable}@${found.version ?? "unknown"}`;
}

function modeOrNull(mode: PermissionMode | undefined): ReturnType<typeof modeOf> {
  return mode === undefined ? null : modeOf(mode);
}

function titlePrompt(prompt: string): string {
  return [
    "Write a title for a chat that starts with the message below: at most six words, plain",
    "text, no quotes and no closing period. Reply with the title alone.",
    "",
    "<message>",
    prompt.slice(0, 4000),
    "</message>",
  ].join("\n");
}

/** The default mode Claude's settings name for `cwd`, read as the CLI reads them. */
async function settingsMode(cwd: string): Promise<PermissionMode | undefined> {
  try {
    return (await resolveSettings({ cwd })).effective.permissions?.defaultMode;
  } catch (error) {
    log.debug("Claude's settings couldn't be read", error);
    return undefined;
  }
}

/** Whether the conversation's transcript has a turn in it, which resuming needs. */
async function hasTurns(sessionId: string, cwd: string): Promise<boolean> {
  try {
    return (await getSessionMessages(sessionId, { dir: cwd, limit: 1 })).length > 0;
  } catch (error) {
    log.debug(`Claude's conversation ${sessionId} couldn't be read`, error);
    return false;
  }
}
