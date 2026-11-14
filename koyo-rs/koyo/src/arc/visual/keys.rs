//! ⌥← and ⌥→, ⌥⇧← and ⌥⇧→, and ⌘⌥→ as a person presses them (`docs/arc-sidebar.md` 7): ⌥→
//! shows the next project, the light passing to its tile, a frame of it caught mid-way, and
//! from the space's last tile the next space's, sliding the space over as a swipe does; past the last project, and ⌥⇧→ at the last space, the space gives and
//! settles back; from a new chat's composer, where ⌥← and ⌥→ go a word back and on, ⌘⌥→ shows
//! the next project all the same, and a press on the chat's background takes the keyboard out
//! of the prompt, where it stays (`shell/keys.rs`). Then Settings → Keyboard shortcuts, which
//! lists them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{KeyUpEvent, Keystroke, Modifiers, PlatformInput, point, px};

use super::acts::{Part, frames, save_with, settle};
use crate::navigation::Route;
use crate::settings;
use crate::settings_view::SettingsPage;
use crate::shell::NewChat;
use crate::visual::{Drawn, Result};

/// A frame of the display's, at 60 Hz.
const FRAME: Duration = Duration::from_millis(16);

/// What the window made of a key: the action bound to it that ran, if one did, and the key
/// contexts it was pressed in.
type Heard = (Option<&'static str>, String);

/// Presses `keys`, each a keystroke let up after it's pressed, as a person does: an arrow
/// pressed again while down is taken for macOS repeating it (`key_repeat`). Draws after: what
/// the window made of the last.
fn press(drawn: &mut Drawn, keys: &'static str) -> Result<Heard> {
    let heard: Rc<RefCell<Option<Heard>>> = Rc::default();
    let hearing = heard.clone();
    let _observing = drawn.cx.update(|cx| {
        cx.observe_keystrokes(move |event, _, _| {
            let contexts = (event.context_stack.iter())
                .map(|context| format!("{context:?}"))
                .collect::<Vec<_>>()
                .join(" > ");
            let action = event.action.as_ref().map(|action| action.name());
            *hearing.borrow_mut() = Some((action, contexts));
        })
    });
    let window = drawn.window;
    for key in keys.split(' ') {
        let keystroke = Keystroke::parse(key).map_err(|error| error.to_string())?;
        (drawn.cx)
            .update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke.clone(), cx);
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            })
            .map_err(|error| error.to_string())?;
    }
    drawn.draw()?;
    let heard = heard.borrow_mut().take();
    heard.ok_or_else(|| format!("{keys} wasn't heard"))
}

/// Types `text` into what has the keyboard, and draws.
fn type_text(drawn: &mut Drawn, text: &str) -> Result<()> {
    drawn.cx.simulate_input(drawn.window, text);
    drawn.draw()
}

fn space(drawn: &Drawn) -> usize {
    drawn.cx.read(|cx| settings::get(cx).space)
}

/// The keys between projects and spaces, in a composer, and on the shortcuts page; `said` gets
/// the pictures and what each key did.
pub fn space_keys(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    between_spaces(drawn, said)?;
    in_the_composer(drawn, said)?;
    shortcuts_page(drawn, said)
}

/// The place among the tiles of the space shown of the project selected.
fn slot(drawn: &Drawn) -> Option<usize> {
    drawn.cx.read(|cx| {
        let shell = drawn.shell.read(cx);
        let selected = shell.workspace.active_project();
        (shell.space_projects(cx).iter()).position(|&index| Some(index) == selected)
    })
}

/// Shows the first space, its tile at `slot` selected (its last for none), as a click on it
/// does, and lets it settle with nothing focused, as when the window has just come forward:
/// the slot.
fn on_tile(drawn: &mut Drawn, slot: Option<usize>) -> Result<usize> {
    let shell = drawn.shell.clone();
    let slot = drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.show_space(0, cx);
            let tiles = shell.space_projects(cx);
            let slot = slot.unwrap_or(tiles.len().saturating_sub(1));
            if let Some(&index) = tiles.get(slot) {
                shell.select_project(index, cx);
            }
            slot
        })
    });
    settle(drawn)?;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| window.blur(cx))
        .map_err(|error| error.to_string())?;
    drawn.draw()?;
    Ok(slot)
}

/// With nothing focused: ⌥→ passes the light from the first tile to the next; from the space's
/// last tile it slides to the next project, the next space's, and past it, with only an empty
/// space after, gives; ⌥⇧→ shows that space and gives at it, the last; ⌥⇧← twice comes back to
/// the first.
fn between_spaces(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    on_tile(drawn, Some(0))?;
    let (action, _) = press(drawn, "alt-right")?;
    frames(drawn, 3, FRAME)?;
    let saved = save_with(drawn, "keys-light-passing", Part::Tiles)?;
    said.push(format!(
        "⌥→ ran {action:?}, the light caught passing: {saved}"
    ));
    settle(drawn)?;
    let saved = save_with(drawn, "keys-next-tile", Part::Tiles)?;
    said.push(format!(
        "then on tile {:?} of space {}: {saved}",
        slot(drawn),
        space(drawn)
    ));
    if (slot(drawn), space(drawn)) != (Some(1), 0) {
        return Err("⌥→ didn't light the next tile".into());
    }

    let last = on_tile(drawn, None)?;
    said.push(format!("on the first space's last tile, {last}"));
    let (action, _) = press(drawn, "alt-right")?;
    frames(drawn, 5, FRAME)?;
    let path = drawn.save("keys-sliding.png")?;
    said.push(format!("saved {} (⌥→ mid-slide)", path.display()));
    settle(drawn)?;
    let path = drawn.save("keys-next-project.png")?;
    said.push(format!("⌥→ ran {action:?}: space 0 → {}", space(drawn)));
    said.push(format!("saved {}", path.display()));
    if space(drawn) != 1 {
        return Err("⌥→ didn't show the second space's project".into());
    }
    // No project past it: the empty space after is passed over, so there's nothing to show.
    let gave = gives(drawn, "alt-right", "keys-last-project-gives.png", said)?;
    if space(drawn) != 1 || gave >= 0. {
        return Err("⌥→ past the last project didn't give and stay".into());
    }
    let (action, _) = press(drawn, "alt-shift-right")?;
    settle(drawn)?;
    said.push(format!("⌥⇧→ ran {action:?}: on to space {}", space(drawn)));
    if space(drawn) != 2 {
        return Err("⌥⇧→ didn't show the empty space".into());
    }
    let gave = gives(drawn, "alt-shift-right", "keys-last-gives.png", said)?;
    if space(drawn) != 2 || gave >= 0. {
        return Err("⌥⇧→ at the last space didn't give and stay".into());
    }
    press(drawn, "alt-shift-left alt-shift-left")?;
    settle(drawn)?;
    said.push(format!("⌥⇧← twice: back on space {}", space(drawn)));
    if space(drawn) != 0 {
        return Err("⌥⇧← didn't come back to the first space".into());
    }
    Ok(())
}

/// Presses `key` where it has nothing more to show, a frame of the space giving saved as `name`:
/// how far it gave, once it's settled back.
fn gives(drawn: &mut Drawn, key: &'static str, name: &str, said: &mut Vec<String>) -> Result<f32> {
    let shell = drawn.shell.clone();
    let (action, _) = press(drawn, key)?;
    frames(drawn, 4, FRAME)?;
    let offset = drawn.cx.read(|cx| shell.read(cx).swipe.offset);
    let path = drawn.save(name)?;
    settle(drawn)?;
    said.push(format!(
        "{key} ran {action:?} with nothing past: gave {offset:.1} pt and settled back on space {}",
        space(drawn)
    ));
    said.push(format!("saved {}", path.display()));
    Ok(offset)
}

/// A new chat's composer, on the first space's last tile's project, has the keyboard: ⌥← and ⌥→
/// go a word back and on, a press on the chat's background takes the keyboard out
/// (`out_of_the_prompt`), and ⌘⌥→ shows the next project, the next space's, from the prompt.
fn in_the_composer(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    on_tile(drawn, None)?;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            window.dispatch_action(Box::new(NewChat), cx);
        })
        .map_err(|error| error.to_string())?;
    settle(drawn)?;
    type_text(drawn, "hello world")?;
    let back = press(drawn, "alt-left")?;
    type_text(drawn, "X")?;
    let on = press(drawn, "alt-right")?;
    type_text(drawn, "Y")?;
    let path = drawn.save("keys-composer.png")?;
    said.push(format!("in the composer ⌥← ran {:?} in {}", back.0, back.1));
    said.push(format!("in the composer ⌥→ ran {:?}", on.0));
    said.push(format!("saved {} (\"hello XworldY\")", path.display()));
    if back.0 != Some("input::MoveToPreviousWord")
        || on.0 != Some("input::MoveToNextWord")
        || space(drawn) != 0
    {
        return Err("⌥← and ⌥→ didn't move a word in the composer".into());
    }
    out_of_the_prompt(drawn, said)?;
    let (next, contexts) = press(drawn, "cmd-alt-right")?;
    frames(drawn, 5, FRAME)?;
    let path = drawn.save("keys-from-the-composer.png")?;
    settle(drawn)?;
    said.push(format!(
        "in the composer ⌘⌥→ ran {next:?} in {contexts}: space 0 → {}",
        space(drawn)
    ));
    said.push(format!("saved {} (mid-slide)", path.display()));
    if space(drawn) != 1 {
        return Err("⌘⌥→ didn't show the next project from the composer".into());
    }
    Ok(())
}

/// From the prompt, a press on the chat's background takes the keyboard out of it, where it
/// stays: ⌥→ shows the next project, the next space's, and ⌥⇧← comes back, the chat shown again
/// not taking the keyboard, nor once its tab is picked. A press in the prompt puts it back. The
/// chat is the first space's last tile's, so ⌥→ goes on into the next space.
fn out_of_the_prompt(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let window = drawn.window;
    // The chat's empty background, high above the prompt's box.
    (drawn.cx).simulate_click(window, point(px(1100.), px(180.)), Modifiers::default());
    drawn.draw()?;
    let (next, contexts) = press(drawn, "alt-right")?;
    settle(drawn)?;
    said.push(format!(
        "the chat's background pressed, ⌥→ ran {next:?} in {contexts}: space 0 → {}",
        space(drawn)
    ));
    if next != Some("arc::NextProject") || space(drawn) != 1 {
        return Err("a press on the chat's background left the keyboard in its prompt".into());
    }
    // Back to the space, which shows the chat's project again: at the first space ⌥⇧← has none
    // to show, and in the prompt it would select a word back instead.
    let (back, _) = press(drawn, "alt-shift-left")?;
    settle(drawn)?;
    let (still, contexts) = press(drawn, "alt-shift-left")?;
    settle(drawn)?;
    let path = drawn.save("keys-out-of-the-prompt.png")?;
    said.push(format!(
        "⌥⇧← ran {back:?}: back on space {}; again, with the chat shown, {still:?} in {contexts}",
        space(drawn)
    ));
    said.push(format!("saved {}", path.display()));
    if back != Some("arc::PreviousSpace") || still != Some("arc::PreviousSpace") {
        return Err("the chat shown again took the keyboard into its prompt".into());
    }
    // Its tab picked in the sidebar: moved to, the chat still isn't typed in.
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            if let Some((project, branch, tab)) = shell.workspace.shown_tab() {
                shell.pick_tab(project, branch, tab, cx);
            }
        });
    });
    settle(drawn)?;
    let (picked, contexts) = press(drawn, "alt-shift-left")?;
    settle(drawn)?;
    said.push(format!("its tab picked, ⌥⇧← ran {picked:?} in {contexts}"));
    if picked != Some("arc::PreviousSpace") {
        return Err("the chat's tab picked put the keyboard in its prompt".into());
    }
    // A press in the prompt's text, and the person types there again.
    (drawn.cx).simulate_click(window, point(px(700.), px(498.)), Modifiers::default());
    drawn.draw()?;
    let (word, contexts) = press(drawn, "alt-left")?;
    said.push(format!("its prompt pressed, ⌥← ran {word:?} in {contexts}"));
    if word != Some("input::MoveToPreviousWord") {
        return Err("a press in the chat's prompt didn't put the keyboard in it".into());
    }
    Ok(())
}

/// Settings → Keyboard shortcuts, its Spaces section listing ⌥← and ⌥→ for the projects and
/// ⌥⇧← and ⌥⇧→ for the spaces.
fn shortcuts_page(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    let go = |drawn: &mut Drawn, route| {
        (drawn.cx).update(|cx| shell.update(cx, |shell, cx| shell.navigate(route, cx)));
    };
    go(drawn, Route::Settings(SettingsPage::KeyboardShortcuts));
    settle(drawn)?;
    let path = drawn.save("keys-shortcuts-page.png")?;
    said.push(format!("saved {}", path.display()));
    go(drawn, Route::Workspace);
    drawn.draw()
}
