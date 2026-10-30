//! Go to Symbol: ⌘⇧O opens a palette over the window that lists the symbols of the file
//! shown, narrowed as a name's typed; ⌘⌥O one that finds a symbol of the selected checkout
//! by name, as its servers know them. The one picked is shown in the Files panel, its name
//! selected, as Go to File shows a file.

mod render;
mod view;

use gpui_kit::{App, Context, Div, InteractiveElement as _, KeyBinding, Window, actions};

use view::Source;
#[cfg(test)]
pub use view::Symbols;

use crate::shell::Shell;

actions!(symbols, [DocumentSymbols, WorkspaceSymbols]);

/// ⌘⇧O and ⌘⌥O anywhere (⌘T starts a chat); and in the palette's field, the keys that
/// move through what it lists, which the field would otherwise take.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-o", DocumentSymbols, None),
        KeyBinding::new("cmd-alt-o", WorkspaceSymbols, None),
    ]);
    crate::list::bind_keys("Symbols > Input", cx);
}

impl Shell {
    /// What the window does with the palettes' actions.
    pub fn handle_symbols(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(Self::document_symbols))
            .on_action(cx.listener(Self::workspace_symbols))
    }

    /// Opens the palette on the symbols of the file shown, while one is and a server
    /// knows it.
    fn document_symbols(&mut self, _: &DocumentSymbols, window: &mut Window, cx: &mut Context<Self>) {
        let symbols = (self.right_panel.files())
            .and_then(|files| files.read(cx).viewing()?.editor.read(cx).document_symbols(cx));
        let Some(symbols) = symbols else {
            return;
        };
        view::open(Source::Document(symbols), cx.entity().downgrade(), window, cx);
    }

    /// Opens the palette that finds a symbol of the selected checkout by name.
    fn workspace_symbols(&mut self, _: &WorkspaceSymbols, window: &mut Window, cx: &mut Context<Self>) {
        let Some(checkout) = self.right_panel.checkout() else {
            return;
        };
        let root = checkout.read(cx).root().clone();
        view::open(Source::Workspace(root), cx.entity().downgrade(), window, cx);
    }
}
