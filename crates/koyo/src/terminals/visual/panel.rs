//! The activity panel (`docs/terminals.md` 7.4): ⌘⇧L in the terminal shows it beside the
//! terminal with what the person and the agent did, the commands the shell ran, the output
//! from a mark on and the terminal's record; ⌘⇧L again puts it away.

use gpui_kit::Entity;

use super::drive::{press, type_line, wait_until};
use crate::terminals::panel::{ActivityPanel, PanelTab};
use crate::visual::{Drawn, Result};

/// The shown terminal's panel, when it's open.
fn open_panel(drawn: &mut Drawn) -> Option<Entity<ActivityPanel>> {
    let shell = drawn.shell.clone();
    drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let id = shell.shown()?.0;
        shell.terminals.panels.get(&id).cloned()
    })
}

fn read<T>(drawn: &mut Drawn, panel: &Entity<ActivityPanel>, f: impl Fn(&ActivityPanel) -> T) -> T {
    drawn.cx.read(|cx| f(panel.read(cx)))
}

fn show(drawn: &mut Drawn, panel: &Entity<ActivityPanel>, tab: PanelTab) -> Result<()> {
    drawn
        .cx
        .update(|cx| panel.update(cx, |panel, cx| panel.show(tab, cx)));
    drawn.draw()
}

pub fn panel(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    press(drawn, "cmd-shift-l")?;
    let panel = open_panel(drawn).ok_or("⌘⇧L in the terminal showed no panel")?;
    wait_until(drawn, "the panel to read the logs", |drawn| {
        let (actions, commands) = read(drawn, &panel, |p| (p.actions().len(), p.commands().len()));
        Ok(actions > 3 && commands >= 5)
    })?;
    let actions = read(drawn, &panel, ActivityPanel::actions);
    for wanted in ["took control", "gave back"] {
        if !actions.iter().any(|action| action.starts_with(wanted)) {
            return Err(format!("the activity has no {wanted:?}: {actions:?}"));
        }
    }
    said.push(format!("{}", drawn.save("panel-activity.png")?.display()));

    show(drawn, &panel, PanelTab::Commands)?;
    let commands = read(drawn, &panel, ActivityPanel::commands);
    let interrupted = commands.iter().find(|(command, _)| command == "sleep 30");
    if interrupted.is_none_or(|(_, exit)| *exit == Some(0)) {
        return Err(format!(
            "the interrupted sleep isn't a failed command: {commands:?}"
        ));
    }
    said.push(format!("{}", drawn.save("panel-commands.png")?.display()));
    drawn
        .cx
        .update(|cx| panel.update(cx, ActivityPanel::press_failed_only));
    drawn.draw()?;
    said.push(format!("{}", drawn.save("panel-failed.png")?.display()));

    show(drawn, &panel, PanelTab::Output)?;
    drawn
        .cx
        .update(|cx| panel.update(cx, ActivityPanel::press_mark));
    wait_until(drawn, "the mark", |drawn| {
        Ok(read(drawn, &panel, |p| p.output().is_some()))
    })?;
    type_line(drawn, "echo after-the-mark")?;
    wait_until(drawn, "the output since the mark", |drawn| {
        let text = read(drawn, &panel, |p| p.output().map(str::to_owned)).unwrap_or_default();
        Ok(text.contains("\nafter-the-mark\n"))
    })?;
    said.push(format!("{}", drawn.save("panel-output.png")?.display()));

    show(drawn, &panel, PanelTab::Info)?;
    wait_until(drawn, "the terminal's record", |drawn| {
        Ok(read(drawn, &panel, ActivityPanel::has_info))
    })?;
    said.push(format!("{}", drawn.save("panel-info.png")?.display()));

    drop(panel);
    press(drawn, "cmd-shift-l")?;
    if open_panel(drawn).is_some() {
        return Err("⌘⇧L again left the panel open".into());
    }
    said.push(format!(
        "the activity panel read {} actions and {} commands, the output from a mark, and the record",
        actions.len(),
        commands.len()
    ));
    Ok(())
}
