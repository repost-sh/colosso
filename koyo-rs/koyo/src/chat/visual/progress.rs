//! `--visual-check-chat-progress`: the plan a chat's agent keeps, as the details card shows it
//! (`docs/chat.md` 5.3, 5.5), saved as PNGs under `.superpowers/qa/chat-progress/`. The fake
//! provider keeps a plan for a message that starts "Show your plan", moving it along until it's
//! all done, and stops partway for one that starts "Show your plan, then stop". Drawn: the card
//! down with the share beside its toggle; the card up beside the chat as the plan runs, its
//! progress open, light and dark, and widened; the chat at full width left of the card, its
//! progress open and closed; the plan all done; a chat whose turn ended with a step running; the
//! card over that chat in a pane too narrow for both, split with another; and the Chat settings
//! page with its switch.
//!
//! ```sh
//! mkdir -p .superpowers/c/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$PWD/.superpowers/c TMPDIR=$PWD/.superpowers/c/tmp \
//!   KOYO_CONFIG_DIR=$PWD/.superpowers/qa/chat-progress/config \
//!   KOYO_LOG_DIR=$PWD/.superpowers/c/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-chat-progress
//! ```

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{Entity, Keystroke, PlatformInput, ScrollDelta, ScrollEvent, point, px};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::progress::{Plan, StepStatus};

use crate::chat::thread::ChatThread;
use crate::model::{BranchUnit, Edge, Tab};
use crate::navigation::Route;
use crate::settings::{self, ChatWidth};
use crate::settings_view::SettingsPage;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

const OUT: &str = ".superpowers/qa/chat-progress";
/// How long the fake's plan gets to reach a step, past its own pace.
const PATIENCE: Duration = Duration::from_secs(20);
/// What's told to the chats: the fake provider's cues to keep a plan, all the way or partway.
const SAID: &str = "Show your plan: the ledger test fails one run in ten.";
/// How wide the card's dragged for the picture of a wider one.
const WIDENED: f32 = 420.;
const SAID_TO_STOP: &str = "Show your plan, then stop: the ledger test fails one run in ten.";

/// The chat's plan and whether its agent works, as the app has them.
fn plan_of(drawn: &Drawn, thread: &Entity<ChatThread>) -> (Option<Plan>, bool) {
    drawn.cx.read(|cx| {
        let replica = thread.read(cx).replica();
        let plan = replica.and_then(|replica| replica.thread.progress.clone());
        (plan, replica.is_some_and(koyo_agent::replica::Replica::working))
    })
}

/// Whether step `id` of the plan has `status`.
fn has(plan: Option<&Plan>, id: &str, status: StepStatus) -> bool {
    (plan.into_iter().flat_map(Plan::steps)).any(|step| step.id == id && step.status == status)
}

/// Waits for chat `id` to hold its thread, and for `ready` of its plan and whether it works.
fn waiting(
    drawn: &mut Drawn,
    id: &ThreadId,
    ready: impl Fn(Option<&Plan>, bool) -> bool,
) -> Result<Entity<ChatThread>> {
    let shell = drawn.shell.clone();
    let thread = drawn.cx.update(|cx| {
        let chats = shell.read(cx).chats().clone();
        chats.update(cx, |chats, cx| chats.thread(id, cx))
    });
    drawn.until(PATIENCE, |drawn| {
        let (plan, working) = plan_of(drawn, &thread);
        Ok(ready(plan.as_ref(), working))
    })?;
    Ok(thread)
}

/// Changes the settings by `change`, and draws what that changed.
fn set(drawn: &mut Drawn, change: impl FnOnce(&mut settings::Settings)) -> Result<()> {
    drawn.cx.update(|cx| settings::update(cx, change));
    drawn.draw()
}

/// Changes the branch's tabs and panes by `change`, and draws what that changed.
fn panes(drawn: &mut Drawn, change: impl FnOnce(&mut BranchUnit)) -> Result<()> {
    let shell = drawn.shell.clone();
    (drawn.cx)
        .update_window(drawn.window, |_, _, cx| {
            shell.update(cx, |shell, cx| {
                shell.workspace.branch_mut().map(change);
                cx.notify();
            });
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()?;
    drawn.draw()
}

/// Puts the first tab's pane on the right of the shown one, which halves the chat's pane: too
/// narrow for the chat beside the card. Says the pane it moved.
fn beside_another(drawn: &mut Drawn) -> Result<u64> {
    let mut moved = None;
    panes(drawn, |unit| {
        let pane = |tab: Option<&Tab>| tab.map(|tab| tab.first().id);
        let (shown, other) = (pane(unit.tabs.get(unit.active_tab)), pane(unit.tabs.first()));
        if let (Some(shown), Some(other)) = (shown, other)
            && shown != other
        {
            unit.move_pane(other, shown, Edge::Right);
            moved = Some(other);
        }
    })?;
    moved.ok_or_else(|| "there's no other tab to put beside the chat".into())
}

/// Takes pane `moved` back to a tab of its own.
fn alone_again(drawn: &mut Drawn, moved: u64) -> Result<()> {
    panes(drawn, |unit| {
        let shown = unit.tabs.get(unit.active_tab).map(|tab| tab.first().id);
        unit.pane_to_tab(moved, 0);
        if let Some(shown) = shown {
            unit.active_tab = unit.tab_with(shown).unwrap_or(unit.active_tab);
        }
    })
}

/// Presses ⌘⇧B, which brings the details card up, or takes it down.
fn toggle_card(drawn: &mut Drawn) -> Result<()> {
    let keystroke = Keystroke::parse("cmd-shift-b").map_err(|error| error.to_string())?;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()
}

/// Saves what shows as `name`, drawn twice first: what floats is laid out in one frame and drawn
/// in the next.
fn shot(drawn: &mut Drawn, name: &str) -> Result<PathBuf> {
    drawn.draw()?;
    drawn.draw()?;
    drawn.save(name)
}

/// Scrolls what's under the middle of the window down by `by`, as a wheel would.
fn scroll(drawn: &mut Drawn, by: f32) -> Result<()> {
    let event = ScrollEvent {
        position: point(px(1000.), px(600.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-by))),
        ..ScrollEvent::default()
    };
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            window.dispatch_event(PlatformInput::Scroll(event), cx);
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()
}

pub(super) fn draw() -> Result<Vec<String>> {
    crate::visual::draw_glass_as_fill();
    #[cfg(debug_assertions)]
    crate::sight::count_unseen(true);
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let shell = drawn.shell.clone();
    let root = checkout()?;
    let mut saved: Vec<PathBuf> = Vec::new();

    let id = drawn.chat_saying(&root, SAID)?;
    let thread = waiting(&mut drawn, &id, |plan, _| has(plan, "2.1", StepStatus::Running))?;
    set(&mut drawn, |settings| settings.chat_details.open = false)?;
    saved.push(shot(&mut drawn, "progress-card-down.png")?);
    set(&mut drawn, |settings| {
        settings.chat_details.open = true;
        settings.chat_details.progress_open = true;
    })?;
    saved.push(shot(&mut drawn, "progress-running.png")?);
    drawn.cx.update(|cx| cx.dispatch_action(&UseDarkTheme));
    saved.push(shot(&mut drawn, "progress-running-dark.png")?);
    drawn.cx.update(|cx| cx.dispatch_action(&UseLightTheme));
    set(&mut drawn, |settings| settings.chat_details.width = Some(WIDENED))?;
    saved.push(shot(&mut drawn, "progress-widened.png")?);
    set(&mut drawn, |settings| {
        settings.chat_details.width = None;
        settings.chat_width = ChatWidth::Full;
    })?;
    toggle_card(&mut drawn)?;
    saved.push(shot(&mut drawn, "progress-full-card-down.png")?);
    toggle_card(&mut drawn)?;
    saved.push(shot(&mut drawn, "progress-pushed-full.png")?);
    set(&mut drawn, |settings| settings.chat_details.progress_open = false)?;
    saved.push(shot(&mut drawn, "progress-pushed-full-closed.png")?);
    set(&mut drawn, |settings| {
        settings.chat_width = ChatWidth::Narrow;
        settings.chat_details.progress_open = true;
    })?;
    drawn.until(PATIENCE, |drawn| {
        let (plan, working) = plan_of(drawn, &thread);
        Ok(!working && plan.is_some_and(|plan| plan.is_closed()))
    })?;
    saved.push(shot(&mut drawn, "progress-done.png")?);

    let stopped = drawn.chat_saying(&root, SAID_TO_STOP)?;
    waiting(&mut drawn, &stopped, |plan, working| {
        !working && has(plan, "2.1", StepStatus::Running)
    })?;
    saved.push(shot(&mut drawn, "progress-paused.png")?);
    let moved = beside_another(&mut drawn)?;
    toggle_card(&mut drawn)?;
    saved.push(shot(&mut drawn, "progress-over-narrow.png")?);
    alone_again(&mut drawn, moved)?;

    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::Chat), cx);
        });
    });
    drawn.draw()?;
    scroll(&mut drawn, 900.)?;
    saved.push(shot(&mut drawn, "settings-chat-progress.png")?);
    let data = settings::data_dir().ok_or("there's no data folder")?;
    let host = koyo_agent::client::host_dir(&data);
    koyo_agent::client::stop_host(&host).map_err(|error| error.to_string())?;
    Ok((saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect())
}
