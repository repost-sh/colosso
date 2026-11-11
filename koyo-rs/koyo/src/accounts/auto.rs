//! Auto-switch as the app runs it (`docs/claude-accounts.md` 5). While it's on, the accounts
//! crate's engine ticks on the switcher's job line, so its readings and switches never interleave
//! with the person's, each tick after the wait the last one asked for (claude-swap's cadence: a
//! minute, longer while blocked). What it does that matters is said in the panel, and a switch
//! also as a notice. Turned off, the engine goes, with the ticks still waiting; its going
//! unpins the switcher's poll planning, after any tick still running.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::{Context, Task};
use koyo_accounts::autoswitch::{AutoSwitchEngine, AutoSwitchEvent, TickReport};

use super::model::AutoSwitch;
use super::{Accounts, Job, words};
use crate::native::notifications;
use crate::settings;

/// The name auto-switch's notices are posted under: not a chat's, so a click brings Koyo to the
/// front, where the panel says what happened.
const NOTICE: &str = "claude-accounts-auto-switch";

pub struct Auto {
    engine: Arc<Mutex<AutoSwitchEngine>>,
    /// The wait for the next tick.
    ticking: Option<Task<()>>,
}

impl Accounts {
    /// Starts or stops auto-switch as Settings say; it starts once the switcher is on hand
    /// (`made` follows again then). Each tick runs at Settings' threshold of the moment.
    pub(super) fn follow_auto(&mut self, cx: &mut Context<Self>) {
        let wanted = &settings::get(cx).accounts;
        let (on, threshold) = (wanted.auto_switch, wanted.threshold());
        let Some(running) = &mut self.running else {
            return;
        };
        let service = &mut running.service;
        match (&service.auto, on) {
            (Some(_), true) | (None, false) => {}
            (None, true) => {
                let Some(engine) = service.engine(threshold) else {
                    return;
                };
                service.auto = Some(Auto {
                    engine: Arc::new(Mutex::new(engine)),
                    ticking: None,
                });
                self.tick(cx);
            }
            (Some(_), false) => {
                service.auto = None;
                service.drop_ticks();
            }
        }
        let said = (self.view.auto_switch.as_ref()).and_then(|auto| auto.said.clone());
        let shown = on.then_some(AutoSwitch { threshold, said });
        if self.view.auto_switch != shown {
            self.view.auto_switch = shown;
            cx.notify();
        }
    }

    /// Asks for a tick, while auto-switch is on.
    fn tick(&mut self, cx: &mut Context<Self>) {
        let Some(auto) = (self.running.as_ref()).and_then(|running| running.service.auto.as_ref())
        else {
            return;
        };
        let job = Job::Tick {
            engine: auto.engine.clone(),
            threshold: settings::get(cx).accounts.threshold(),
        };
        self.ask(job, cx);
    }

    /// What a tick did: what matters of it said in the panel, a switch also as a notice; the
    /// next tick after the wait it asked for.
    pub(super) fn ticked(&mut self, report: &TickReport, cx: &mut Context<Self>) {
        let now = super::now();
        if let Some(shown) = &mut self.view.auto_switch {
            for event in &report.events {
                let Some(said) = words::auto_switched(event, now) else {
                    continue;
                };
                if matches!(event, AutoSwitchEvent::Switch(_)) {
                    notifications::post(NOTICE, "Claude account switched", &said);
                }
                shown.said = Some(said);
            }
        }
        let Some(auto) = (self.running.as_mut()).and_then(|running| running.service.auto.as_mut())
        else {
            return;
        };
        let wait = Duration::from_secs_f64(report.delay.max(1.));
        auto.ticking = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, Accounts::tick).ok();
        }));
    }
}
