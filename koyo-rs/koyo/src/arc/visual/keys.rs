//! ⌥← and ⌥→ as a person presses them (`docs/arc-sidebar.md` 7): with nothing focused they
//! slide the space over as a swipe does, a frame of it caught mid-way; at the last space the
//! space gives and settles back; in a new chat's composer they go a word back and on instead.
//! Then Settings → Keyboard shortcuts, which lists them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{KeyUpEvent, Keystroke, PlatformInput};

use super::acts::{frames, settle};
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

/// The keys between spaces, in a composer, and on the shortcuts page; `said` gets the pictures
/// and what each key did.
pub fn space_keys(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    between_spaces(drawn, said)?;
    in_the_composer(drawn, said)?;
    shortcuts_page(drawn, said)
}

/// With nothing focused: ⌥→ slides to the next space, gives at the last, and ⌥← comes back.
fn between_spaces(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    drawn
        .cx
        .update(|cx| shell.update(cx, |shell, cx| shell.show_space(0, cx)));
    settle(drawn)?;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| window.blur(cx))
        .map_err(|error| error.to_string())?;
    drawn.draw()?;

    let (action, _) = press(drawn, "alt-right")?;
    frames(drawn, 5, FRAME)?;
    let path = drawn.save("keys-sliding.png")?;
    said.push(format!("saved {} (⌥→ mid-slide)", path.display()));
    settle(drawn)?;
    let path = drawn.save("keys-next-space.png")?;
    said.push(format!("⌥→ ran {action:?}: space 0 → {}", space(drawn)));
    said.push(format!("saved {}", path.display()));
    if space(drawn) != 1 {
        return Err("⌥→ didn't show the second space".into());
    }
    press(drawn, "alt-right")?;
    settle(drawn)?;
    let last = space(drawn);
    press(drawn, "alt-right")?;
    frames(drawn, 4, FRAME)?;
    let offset = drawn.cx.read(|cx| shell.read(cx).swipe.offset);
    let path = drawn.save("keys-last-gives.png")?;
    settle(drawn)?;
    said.push(format!(
        "⌥→ at the last space ({last}) gave {offset:.1} pt and settled back on {}",
        space(drawn)
    ));
    said.push(format!("saved {}", path.display()));
    if space(drawn) != last || offset >= 0. {
        return Err("⌥→ at the last space didn't give and stay".into());
    }
    press(drawn, "alt-left alt-left")?;
    settle(drawn)?;
    said.push(format!("⌥← twice: back on space {}", space(drawn)));
    if space(drawn) != 0 {
        return Err("⌥← didn't come back to the first space".into());
    }
    Ok(())
}

/// A new chat's composer has the keyboard: a word back and on, the space staying put.
fn in_the_composer(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
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
    Ok(())
}

/// Settings → Keyboard shortcuts, its Spaces section listing ⌥← and ⌥→.
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
