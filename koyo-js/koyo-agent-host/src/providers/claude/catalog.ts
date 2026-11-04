// What Claude offers and whether it can run (docs/chat.md 4.5.9, C7). Models, efforts and the
// account come from the CLI's answer to its initialize request and depend only on the CLI and
// the account, so they're kept per executable and version; slash commands depend on the
// folder, refreshed by each session's CLI for its folder, and read by an initialization-only
// probe for a folder no session has run in: a query whose prompt never comes, closed once its
// CLI has answered, with no API call.

import type {
  AccountInfo,
  ModelInfo,
  SDKControlInitializeResponse,
  SlashCommand,
} from "@anthropic-ai/claude-agent-sdk";

import type { Command, Model, ProviderStatus } from "../../protocol/methods";
import type { Env } from "./environment";
import { once } from "./once";
import { type Place, probeOptions } from "./options";
import type { Start } from "./session";

/** How long a probe may take to be answered. */
const PROBE_MS = 25_000;

/** The CLI's initialize answer, for a place it runs in. */
export async function probe(start: Start, place: Omit<Place, "spawn">): Promise<
  SDKControlInitializeResponse
> {
  return await once(start, {
    label: "Claude's probe",
    ms: PROBE_MS,
    options: (spawn) => probeOptions({ ...place, spawn }),
  }, async (conversation) => await conversation.initializationResult());
}

/**
 * What a row goes by once it's chosen. "Default" says nothing of what runs, so that row goes by
 * the name of the row that runs the same model, or else the id of what it runs.
 */
function label(model: ModelInfo, list: readonly ModelInfo[]): string {
  if (model.value !== "default" || model.resolvedModel === undefined) {
    return model.displayName;
  }
  const same = list.find((other) => other !== model && other.resolvedModel === model.resolvedModel);
  return same?.displayName ?? model.resolvedModel;
}

export function models(list: readonly ModelInfo[]): Model[] {
  return list.map((model) => ({
    id: model.value,
    name: model.displayName,
    label: label(model, list),
    runs: model.resolvedModel ?? null,
    efforts: model.supportedEffortLevels ?? [],
    fast: model.supportsFastMode ?? false,
    auto: model.supportsAutoMode ?? false,
  }));
}

export function commands(list: readonly SlashCommand[]): Command[] {
  return list.map((command) => ({
    name: command.name,
    description: command.description,
    argumentHint: command.argumentHint === "" ? null : command.argumentHint,
  }));
}

/**
 * Whether the account is signed in. Only a token source of "none", with no API key and no
 * email, says it isn't; a key or a gateway in the environment signs in without an account.
 */
function signedIn(account: AccountInfo, env: Env): boolean {
  if (env["ANTHROPIC_API_KEY"] !== undefined || env["ANTHROPIC_BASE_URL"] !== undefined) {
    return true;
  }
  const noKey = account.apiKeySource === undefined || account.apiKeySource === "none";
  return !(account.tokenSource === "none" && noKey && account.email === undefined);
}

/** The status of a CLI that answered its probe. */
export function readyStatus(
  found: { readonly executable: string; readonly version: string | null; readonly env: Env },
  init: SDKControlInitializeResponse,
): ProviderStatus {
  const { account } = init;
  const signed = signedIn(account, found.env);
  return {
    provider: "claude",
    state: signed ? "ready" : "signed_out",
    executable: found.executable,
    version: found.version ?? init.claude_code_version ?? null,
    account: signed
      ? { email: account.email ?? null, plan: account.subscriptionType ?? null }
      : null,
    message: signed ? null : "Sign in to Claude.",
  };
}

/** The status when no CLI was found. */
export const MISSING: ProviderStatus = {
  provider: "claude",
  state: "missing",
  executable: null,
  version: null,
  account: null,
  message: "Install Claude Code.",
};
