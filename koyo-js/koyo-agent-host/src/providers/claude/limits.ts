// What a `rate_limit_event` says of the account's subscription limits (docs/claude-accounts.md
// 4), for the app to keep the active account's usage current between its readings of the usage
// endpoint. Claude Code 2.1.322 reads them from each response's `anthropic-ratelimit-unified-*`
// headers: the window the response was measured against, and, in `unifiedWindows`, every window
// the headers carried. That field is marked internal and isn't in the SDK's types, so it's read
// as it comes and dropped when it's not what it was.

import type { SDKRateLimitInfo } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

import { type AccountLimits, LimitStatus, type LimitWindow } from "../../protocol/model";

const Window = z.object({
  utilization: z.number().nonnegative(),
  resetsAt: z.number().positive(),
  status: LimitStatus.optional().catch(undefined),
});

const Windows = z.object({
  five_hour: Window.optional().catch(undefined),
  seven_day: Window.optional().catch(undefined),
  seven_day_overage_included: Window.optional().catch(undefined),
});

/** Epoch seconds as the protocol's time. */
function iso(seconds: number): string {
  return new Date(seconds * 1000).toISOString();
}

function window(raw: z.infer<typeof Window> | undefined): LimitWindow | null {
  return raw === undefined
    ? null
    : { utilization: raw.utilization, resetsAt: iso(raw.resetsAt), status: raw.status ?? null };
}

/** `info` as the protocol's limits. */
export function accountLimits(info: SDKRateLimitInfo): AccountLimits {
  const unified = "unifiedWindows" in info ? info.unifiedWindows : undefined;
  const parsed = Windows.safeParse(unified ?? {});
  const windows = parsed.success ? parsed.data : {};
  const utilization = info.utilization;
  return {
    status: info.status,
    window: info.rateLimitType ?? null,
    utilization: utilization !== undefined && utilization >= 0 ? utilization : null,
    resetsAt: info.resetsAt === undefined ? null : iso(info.resetsAt),
    fiveHour: window(windows.five_hour),
    sevenDay: window(windows.seven_day),
    sevenDayOverageIncluded: window(windows.seven_day_overage_included),
  };
}
