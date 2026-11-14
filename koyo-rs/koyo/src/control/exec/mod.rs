//! The executors: each operation carried out on the window's shell, answering its output as
//! JSON. Reads and quick changes answer at once; what waits on git, the terminal daemon or a
//! file being written answers from a task, git's work off the UI thread. A destructive call
//! first has its question for the person made, naming exactly what it would do.

mod accounts;
mod activity;
mod app;
mod beside;
mod browser;
mod chat;
mod desktop;
mod device;
mod file;
mod info;
mod kept;
mod open;
mod pane;
mod project;
mod select;
mod settings;
mod tab;
mod worktree;

use gpui_kit::base::Root;
use gpui_kit::{AnyWindowHandle, App, Context, Entity, Task, Window};
use koyo_app_control::{Command, ControlError};
use serde::Serialize;
use serde_json::Value;

use super::Caller;
use super::confirm::Question;
use crate::shell::Shell;

pub(crate) use app::status_value;
pub(crate) use project::list as projects_value;
pub(crate) use settings::get as settings_value;
pub(crate) use tab::list_all as tabs_value;

/// An operation's answer: now, or once its task is done.
pub(crate) enum Exec {
    Ready(Result<Value, ControlError>),
    Later(Task<Result<Value, ControlError>>),
}

impl From<Result<Exec, ControlError>> for Exec {
    /// An operation that waits on a task, or why it couldn't start.
    fn from(started: Result<Exec, ControlError>) -> Exec {
        started.unwrap_or_else(|error| Exec::Ready(Err(error)))
    }
}

/// Carries out `command` for `caller`.
pub(crate) fn run(caller: &Caller, command: Command, cx: &mut App) -> Exec {
    use Command as C;
    match command {
        C::AppStatus(_) => Exec::Ready(status_value(cx)),
        C::AppFocus(_) => Exec::Ready(app::focus(cx)),
        C::LayoutSet(input) => Exec::Ready(app::layout_set(&input, cx)),
        C::SettingsOpen(input) => Exec::Ready(app::settings_open(input.page, cx)),
        C::SettingsClose(_) => Exec::Ready(app::settings_close(cx)),
        C::ProjectList(_) => Exec::Ready(project::list(cx)),
        C::ProjectAdd(input) => project::add(caller, &input, cx).into(),
        C::ProjectSelect(input) => Exec::Ready(project::select(&input.project, cx)),
        C::ProjectRemove(input) => Exec::Ready(project::remove(&input.project, cx)),
        C::ProjectMove(input) => Exec::Ready(project::move_to(&input, cx)),
        C::WorktreeBranches(input) => {
            worktree::branches(caller, input.project.as_deref(), cx).into()
        }
        C::WorktreeCreate(input) => worktree::create(caller, input, cx).into(),
        C::WorktreeSelect(input) => Exec::Ready(worktree::select(caller, &input, cx)),
        C::WorktreeDelete(input) => worktree::delete(caller, &input, cx).into(),
        C::TabList(input) => Exec::Ready(tab::list(caller, &input, cx)),
        C::TabOpen(input) => tab::open(caller, &input, cx).into(),
        C::TabSelect(input) => Exec::Ready(tab::select(caller, &input, cx)),
        C::TabMove(input) => Exec::Ready(tab::move_to(&input, cx)),
        C::TabClose(input) => Exec::Ready(tab::close(&input, true, cx)),
        C::TabReopen(_) => Exec::Ready(tab::reopen(cx)),
        C::PaneSplit(input) => pane::split(caller, &input, cx).into(),
        C::PaneFocus(input) => Exec::Ready(pane::focus(input.pane, cx)),
        C::PaneResize(input) => Exec::Ready(pane::resize(&input, cx)),
        C::PaneDetach(input) => Exec::Ready(pane::detach(input.pane, cx)),
        C::PaneClose(input) => Exec::Ready(tab::close(&input, false, cx)),
        C::FileOpen(input) => file::open(caller, &input, cx),
        C::FileSave(input) => file::save(&input, cx).into(),
        C::ChangesOpen(input) => Exec::Ready(file::changes_open(caller, &input, cx)),
        C::ChatList(input) => chat::list(caller, &input, cx).into(),
        C::ChatChoices(input) => chat::choices(caller, &input, cx).into(),
        C::ChatRead(input) => chat::read(&input, cx).into(),
        C::ChatOpen(input) => Exec::Ready(chat::open(&input, cx)),
        C::ChatCreate(input) => chat::create(caller, input, cx).into(),
        C::ChatSend(input) => chat::send(input, cx).into(),
        C::ChatSet(input) => chat::set(caller, input, cx).into(),
        C::ChatStop(input) => Exec::Ready(chat::stop(&input, cx)),
        C::SettingsGet(_) => Exec::Ready(settings::get(cx)),
        C::SettingsSet(input) => Exec::Ready(settings::set(input, cx)),
        C::ActivityList(input) => Exec::Ready(activity::list(caller, &input, cx)),
        C::ClaudeAccountsList(_) => Exec::Ready(accounts::list(cx)),
        C::BrowserList(_) => Exec::Ready(browser::list(cx)),
        C::BrowserClaim(input) => Exec::Ready(browser::claim(caller, &input, cx)),
        C::BrowserRelease(input) => Exec::Ready(browser::release(caller, input.pane, cx)),
        C::BrowserNavigate(input) => browser::navigate(caller, &input, cx).into(),
        C::BrowserGo(input) => browser::go(caller, &input, cx).into(),
        C::BrowserWait(input) => browser::wait(caller, &input, cx).into(),
        C::BrowserRead(input) => browser::read(caller, &input, cx).into(),
        C::BrowserSnapshot(input) => browser::snapshot(caller, &input, cx).into(),
        C::BrowserScreenshot(input) => browser::screenshot(caller, &input, cx).into(),
        C::BrowserEval(input) => browser::eval(caller, input, cx).into(),
        C::BrowserClick(input) => browser::click(caller, input, cx).into(),
        C::BrowserType(input) => browser::type_text(caller, input, cx).into(),
        C::BrowserPressKey(input) => browser::press_key(caller, &input, cx).into(),
        C::BrowserScroll(input) => browser::scroll(caller, input, cx).into(),
        C::BrowserViewport(input) => browser::viewport(caller, &input, cx).into(),
        C::BrowserDialog(input) => browser::dialog(caller, &input, cx).into(),
        C::DesktopOpen(input) => desktop::open(caller, input, cx),
        C::DesktopWindows(_) => desktop::windows(cx),
        C::DesktopScreenshot(input) => desktop::screenshot(&input, cx).into(),
        C::DesktopSnapshot(input) => desktop::snapshot(&input, cx),
        C::DesktopClick(input) => desktop::click(caller, &input, cx).into(),
        C::DesktopMove(input) => desktop::move_to(caller, &input, cx).into(),
        C::DesktopDrag(input) => desktop::drag(caller, &input, cx).into(),
        C::DesktopScroll(input) => desktop::scroll(caller, &input, cx).into(),
        C::DesktopType(input) => desktop::type_text(caller, &input, cx).into(),
        C::DesktopKey(input) => desktop::key(caller, input, cx),
        C::DesktopFocus(input) => desktop::focus(caller, &input, cx),
        C::DesktopTakeFocus(input) => desktop::take_focus(caller, input, cx),
        C::DesktopReturnFocus(_) => desktop::return_focus(cx),
        C::DesktopWait(input) => desktop::wait(&input, cx),
        C::DesktopClose(input) => desktop::close(caller, &input, cx).into(),
        C::DeviceList(_) => device::list(cx).into(),
        C::DeviceOpen(input) => device::open(caller, input, cx).into(),
        C::DeviceClose(input) => device::close(caller, &input, cx).into(),
        C::DeviceScreenshot(input) => device::screenshot(caller, &input, cx).into(),
        C::DeviceSnapshot(input) => device::snapshot(caller, &input, cx).into(),
        C::DeviceWait(input) => device::wait(caller, &input, cx).into(),
        C::DeviceTap(input) => device::tap(caller, &input, cx).into(),
        C::DeviceSwipe(input) => device::swipe(caller, &input, cx).into(),
        C::DeviceScroll(input) => device::scroll(caller, &input, cx).into(),
        C::DevicePinch(input) => device::pinch(caller, &input, cx).into(),
        C::DeviceType(input) => device::type_text(caller, &input, cx).into(),
        C::DeviceKey(input) => device::key(caller, &input, cx).into(),
        C::DeviceButton(input) => device::button(caller, &input, cx).into(),
        C::DeviceRotate(input) => device::rotate(caller, &input, cx).into(),
        C::DeviceApps(input) => device::apps(caller, &input, cx).into(),
        C::DeviceInstall(input) => device::install(caller, &input, cx).into(),
        C::DeviceLaunch(input) => device::launch(caller, &input, cx).into(),
        C::DeviceTerminate(input) => device::terminate(caller, &input, cx).into(),
        C::DeviceOpenUrl(input) => device::open_url(caller, &input, cx).into(),
    }
}

/// The question the person is asked before `command` is carried out, naming exactly what it
/// would do; or why it can't be.
pub(crate) fn question(
    caller: &Caller,
    command: &Command,
    cx: &mut App,
) -> Task<Result<Question, ControlError>> {
    let who = caller.label();
    match command {
        Command::WorktreeDelete(input) => worktree::question(&who, caller, input, cx),
        Command::TabClose(input) => Task::ready(tab::close_question(&who, input, true, cx)),
        Command::PaneClose(input) => Task::ready(tab::close_question(&who, input, false, cx)),
        other => Task::ready(Err(ControlError::internal(format!(
            "{} has no question to confirm it with.",
            other.name()
        )))),
    }
}

/// The window showing the shell, and the shell, while the window is open.
pub(crate) fn main_shell(cx: &App) -> Option<(AnyWindowHandle, Entity<Shell>)> {
    cx.windows().into_iter().find_map(|window| {
        let shell = window
            .read::<Root, _, _>(cx, |root, cx| root.read(cx).view().clone())
            .ok()
            .and_then(|view| view.downcast::<Shell>().ok())
            // Tests show the shell as the window's root itself.
            .or_else(|| window.read::<Shell, _, _>(cx, |shell, _| shell).ok())?;
        Some((window, shell))
    })
}

/// Why an operation on the window can't be carried out while it's closed.
pub(crate) fn window_closed() -> ControlError {
    ControlError::not_found("Koyo's window is closed. Call app_focus to open it, then try again.")
}

/// Runs `change` on the shell in its window.
pub(crate) fn update_shell<R>(
    cx: &mut App,
    change: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) -> R,
) -> Result<R, ControlError> {
    let (window, shell) = main_shell(cx).ok_or_else(window_closed)?;
    window
        .update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| change(shell, window, cx))
        })
        .map_err(|error| {
            ControlError::internal(format!("Koyo's window couldn't be reached: {error}"))
        })
}

/// Runs `change` on the shell in its window, which answers the operation's output or why it
/// failed.
pub(crate) fn try_update_shell<R>(
    cx: &mut App,
    change: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) -> Result<R, ControlError>,
) -> Result<R, ControlError> {
    update_shell(cx, change)?
}

/// `value` as JSON.
pub(crate) fn output<T: Serialize>(value: &T) -> Result<Value, ControlError> {
    serde_json::to_value(value)
        .map_err(|error| ControlError::internal(format!("Koyo couldn't write its answer: {error}")))
}
