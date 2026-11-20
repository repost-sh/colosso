//! The downloads button at the end of a page's address field, there once something was
//! downloaded, and the list it opens: each download with how far it's come, a way to cancel
//! one coming in, and, once done, to open its file or show it in the Finder.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, ElementId, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Render, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px, relative, size,
};
use koyo_browser_ipc::PaneId;

use super::{Downloads, Entry, Key, Stage, size as bytes};
use crate::browser::site::Tones;
use crate::menu::{self, MenuBody};
use crate::shell::icon;
use crate::theme;

/// The list: how wide it is, the room round it, its title's height, a row's, and how many
/// rows it shows, the latest.
const WIDTH: f32 = 300.;
const PAD: f32 = 6.;
const TITLE: f32 = 28.;
const ROW: f32 = 46.;
const MOST: usize = 8;

/// The downloads button of `pane`'s address field, `tint` its icon's colour while nothing
/// comes in, with a bar under it as far as what's coming has come; none before the first
/// download.
pub(in crate::browser) fn button(pane: PaneId, tint: Hsla, cx: &App) -> Option<AnyElement> {
    let listed = Downloads::listed(cx);
    if listed.is_empty() {
        return None;
    }
    let p = theme::palette(cx);
    let coming: Vec<&Entry> = listed.iter().filter(|entry| !entry.stage.ended()).collect();
    let share = share(&coming);
    let trigger = menu::Trigger::new(ElementId::NamedInteger("downloads-menu".into(), pane));
    let open = trigger.is_open(cx);
    let face = div()
        .id(("downloads", pane))
        .debug_selector(|| "downloads-button".to_owned())
        .relative()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |button| button.bg(p.hover))
        .when(open, |button| button.bg(p.selected))
        .child(icon(IconName::Download, 14.).text_color(if coming.is_empty() { tint } else { p.accent }))
        .children(share.map(|share| {
            div()
                .absolute()
                .left(px(5.))
                .bottom(px(2.))
                .h(px(2.))
                .w(px(12. * share))
                .rounded_full()
                .bg(p.accent)
        }))
        .tooltip(|window, cx| Tooltip::new("Downloads").build(window, cx));
    let opens = trigger.clone();
    let element = trigger.wraps(face, move |_, window, cx| {
        cx.stop_propagation();
        let at = opens.clone();
        opens.toggle_panel(|cx| cx.new(|cx| DownloadsPanel::new(at, cx)), window, cx);
    });
    Some(element.into_any_element())
}

/// How much of what's coming has come, while the server said how much each is.
pub(super) fn share(coming: &[&Entry]) -> Option<f32> {
    let totals: Option<u64> = coming.iter().map(|entry| entry.total).sum();
    let total = totals.filter(|&total| total > 0)?;
    let received: u64 = coming.iter().map(|entry| entry.received).sum();
    Some((received as f32 / total as f32).clamp(0., 1.))
}

/// What a row says under its name.
pub(super) fn status(entry: &Entry) -> String {
    let by = entry.by.as_ref().map(|agent| format!(" · {agent}")).unwrap_or_default();
    let said = match &entry.stage {
        Stage::Placing => "Waiting for a place…".to_owned(),
        Stage::Going if matches!(entry.key, Key::Pdf(_)) => "Saving…".to_owned(),
        Stage::Going => match entry.total {
            Some(total) => format!("{} of {}", bytes(entry.received), bytes(total)),
            None => bytes(entry.received),
        },
        Stage::Done if matches!(entry.key, Key::Pdf(_)) => "Saved as a PDF".to_owned(),
        Stage::Done => bytes(entry.received),
        Stage::Canceled => "Canceled".to_owned(),
        Stage::Failed(reason) => reason.clone(),
    };
    format!("{said}{by}")
}

/// The list, in a menu's window under the button.
pub(super) struct DownloadsPanel {
    at: menu::Trigger,
    _changed: Subscription,
}

impl DownloadsPanel {
    fn new(at: menu::Trigger, cx: &mut Context<Self>) -> Self {
        let downloads = Downloads::global(cx);
        let changed = cx.observe(&downloads, |_, _, cx| cx.notify());
        DownloadsPanel { at, _changed: changed }
    }

    fn row(index: usize, entry: &Entry, t: Tones, cx: &mut Context<Self>) -> Stateful<Div> {
        let key = entry.key;
        let path = entry.path.clone();
        let done = entry.stage == Stage::Done;
        let mark = match entry.stage {
            Stage::Failed(_) | Stage::Canceled => IconName::CircleX,
            _ if matches!(key, Key::Pdf(_)) => IconName::FileText,
            _ => IconName::FileDown,
        };
        let bar = (entry.stage == Stage::Going).then(|| {
            let share = share(&[entry]).unwrap_or(0.);
            div().mt(px(3.)).h(px(3.)).w_full().rounded_full().bg(t.off).child(
                div().h_full().w(relative(share)).rounded_full().bg(t.text),
            )
        });
        let text = div()
            .min_w_0()
            .flex_1()
            .flex()
            .flex_col()
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).truncate().child(entry.name.clone()))
            .child(div().text_size(px(11.)).text_color(t.muted).truncate().child(status(entry)))
            .children(bar);
        let side = if done {
            path.clone().map(|path| {
                Self::side(("download-reveal", index), IconName::FolderSearch, "Show in Finder", t)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        cx.reveal_path(&path);
                        this.at.close(cx);
                    }))
            })
        } else if !entry.stage.ended() && matches!(key, Key::Engine { .. }) {
            Some(Self::side(("download-cancel", index), IconName::X, "Cancel", t).on_click(cx.listener(
                move |_, _, _, cx| {
                    cx.stop_propagation();
                    Downloads::global(cx).update(cx, |downloads, cx| downloads.cancel(key, cx));
                },
            )))
        } else {
            None
        };
        div()
            .id(("download", index))
            .debug_selector(move || format!("download-{index}"))
            .h(px(ROW))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(6.))
            .rounded(px(8.))
            .when(done, |row| row.cursor_pointer().hover(move |row| row.bg(t.hover)))
            .child(icon(mark, 18.).text_color(t.muted))
            .child(text)
            .children(side)
            .when_some(path.filter(|_| done), |row, path| {
                row.on_click(cx.listener(move |this, _, _, cx| {
                    cx.open_with_system(&path);
                    this.at.close(cx);
                }))
            })
    }

    fn side(id: (&'static str, usize), mark: IconName, says: &'static str, t: Tones) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |button| button.bg(t.hover))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
            .child(icon(mark, 14.).text_color(t.text))
    }
}

/// How tall the list is with `rows` downloads.
fn height(rows: usize) -> f32 {
    PAD + TITLE + ROW * rows.clamp(1, MOST) as f32 + PAD
}

impl MenuBody for DownloadsPanel {
    fn hears(&mut self, _: &str, _: &mut Context<Self>) -> bool {
        false
    }

    fn extent(&self, _: &Window, cx: &App) -> Size<Pixels> {
        size(px(WIDTH), px(height(Downloads::listed(cx).len())))
    }

    fn backdrop(&self, cx: &App) -> Option<Hsla> {
        Some(Tones::of(theme::palette(cx)).glass)
    }

    fn callout(&self) -> bool {
        true
    }
}

impl Render for DownloadsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tones::of(theme::palette(cx));
        let listed = Downloads::listed(cx);
        let ended = listed.iter().any(|entry| entry.stage.ended());
        let shown: Vec<(usize, Entry)> = (listed.iter().cloned().enumerate().rev().take(MOST)).collect();
        let rows: Vec<Stateful<Div>> =
            (shown.iter()).map(|(index, entry)| Self::row(*index, entry, t, cx)).collect();
        let clear = ended.then(|| {
            div()
                .id("downloads-clear")
                .debug_selector(|| "downloads-clear".to_owned())
                .px(px(8.))
                .py(px(2.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(t.muted)
                .hover(move |button| button.bg(t.hover))
                .child("Clear")
                .on_click(|_, _, cx| Downloads::global(cx).update(cx, Downloads::clear))
        });
        let title = div()
            .h(px(TITLE))
            .px(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Downloads"))
            .children(clear);
        let empty = rows.is_empty().then(|| {
            div().h(px(ROW)).px(px(6.)).flex().items_center().text_size(px(13.)).text_color(t.muted).child("No downloads")
        });
        div()
            .debug_selector(|| "downloads-panel".to_owned())
            .size_full()
            .p(px(PAD))
            .flex()
            .flex_col()
            .text_color(t.text)
            .child(title)
            .children(rows)
            .children(empty)
    }
}
