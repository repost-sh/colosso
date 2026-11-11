//! What a chat's response said of the account's subscription limits (`docs/claude-accounts.md`
//! 4), as `AccountLimits` in `koyo-js/koyo-agent-host/src/protocol/model.ts` has it: the
//! `account.limits` event, which the app takes for the Mac's login, whichever chat it came from.

use serde::{Deserialize, Serialize};

use super::ids::Time;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
}

/// The window a response was measured against, as Claude Code names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWindowName {
    FiveHour,
    SevenDay,
    SevenDayOpus,
    SevenDaySonnet,
    /// The per-model weekly bucket (`7d_oi`), Fable's on the accounts that have one.
    SevenDayOverageIncluded,
    Overage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// The fraction of the window used: 0–1, past 1 when usage legitimately ran past its cap.
    pub utilization: f64,
    pub resets_at: Time,
    pub status: Option<LimitStatus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLimits {
    pub status: LimitStatus,
    pub window: Option<LimitWindowName>,
    pub utilization: Option<f64>,
    pub resets_at: Option<Time>,
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    pub seven_day_overage_included: Option<LimitWindow>,
}
