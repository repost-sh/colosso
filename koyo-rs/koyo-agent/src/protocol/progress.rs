//! The plan an agent keeps of a chat's task with Koyo's progress tool (`docs/chat.md` 5.5), as
//! `Plan` in the host's `protocol/model.ts` defines it: phases, each with its sub-steps, one
//! level deep.

use serde::{Deserialize, Serialize};

use super::ids::Time;

/// Where a step stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Blocked,
    Skipped,
}

impl StepStatus {
    /// Whether the step is under way: running, or waiting on the person.
    pub fn is_open(self) -> bool {
        matches!(self, StepStatus::Running | StepStatus::Blocked)
    }

    /// Whether the step is over: done, or dropped.
    pub fn is_closed(self) -> bool {
        matches!(self, StepStatus::Done | StepStatus::Skipped)
    }
}

/// A sub-step of a phase: a unit of the work the phase delivers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    /// One sentence on what it covers and how the agent will know it's done.
    pub detail: Option<String>,
    pub status: StepStatus,
    pub started_at: Option<Time>,
    pub ended_at: Option<Time>,
}

/// An end-to-end milestone of the task and the sub-steps it takes: once one of them has
/// started, the phase's status is theirs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPhase {
    #[serde(flatten)]
    pub step: PlanStep,
    pub steps: Vec<PlanStep>,
}

impl PlanPhase {
    /// How much of the phase is done, from 0 to 1: all of it once it's done, else its done
    /// sub-steps over those not skipped.
    pub fn share(&self) -> f32 {
        if self.step.status == StepStatus::Done {
            return 1.;
        }
        let (done, counted) = self.steps_done();
        if counted == 0 {
            0.
        } else {
            done as f32 / counted as f32
        }
    }

    /// Its sub-steps done, of those not skipped.
    pub fn steps_done(&self) -> (usize, usize) {
        done_of(self.steps.iter().map(|step| step.status))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub phases: Vec<PlanPhase>,
    /// The agent's one line on what it's doing now.
    pub now: Option<String>,
    /// When the plan's first step started; a plan that replaces another starts its own.
    pub started_at: Option<Time>,
    pub updated_at: Time,
}

impl Plan {
    /// How much of the task is done, from 0 to 1: each phase not skipped counts the same, an open
    /// one by its share. None while no phase counts.
    pub fn share(&self) -> Option<f32> {
        let (total, counted) = (self.phases.iter())
            .filter(|phase| phase.step.status != StepStatus::Skipped)
            .fold((0., 0_usize), |(total, counted), phase| {
                (total + phase.share(), counted + 1)
            });
        (counted > 0).then(|| total / counted as f32)
    }

    /// Its phases done, of those not skipped.
    pub fn phases_done(&self) -> (usize, usize) {
        done_of(self.phases.iter().map(|phase| phase.step.status))
    }

    /// Whether every phase is over, and with it the task.
    pub fn is_closed(&self) -> bool {
        (self.phases.iter()).all(|phase| phase.step.status.is_closed())
    }

    /// Each phase's step, then its sub-steps.
    pub fn steps(&self) -> impl Iterator<Item = &PlanStep> {
        (self.phases.iter()).flat_map(|phase| std::iter::once(&phase.step).chain(&phase.steps))
    }
}

/// How many of `statuses` are done, and how many aren't skipped.
fn done_of(statuses: impl Iterator<Item = StepStatus>) -> (usize, usize) {
    statuses.fold((0, 0), |(done, counted), status| match status {
        StepStatus::Done => (done + 1, counted + 1),
        StepStatus::Skipped => (done, counted),
        StepStatus::Pending | StepStatus::Running | StepStatus::Blocked => (done, counted + 1),
    })
}
