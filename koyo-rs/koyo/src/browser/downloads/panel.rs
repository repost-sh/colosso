//! The list the top bar's downloads button opens (`button.rs`), as Safari's: each download
//! with its file's kind, its name, and how it stands (how much has come of how much, done,
//! failed and why, canceled); Show in Finder while its file is there and Cancel while it comes
//! in; a press on a done one's row opens its file. Clear takes off what has ended.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Render, Size, Stateful, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, px, relative, size,
};

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
        Stage::Done => format!("{} · Done", bytes(entry.received)),
        Stage::Canceled => "Canceled".to_owned(),
        Stage::Failed(reason) => format!("Failed · {reason}"),
    };
    format!("{said}{by}")
}

/// The list, in a menu's window under the button.
pub(super) struct DownloadsPanel {
    at: menu::Trigger,
    _changed: Subscription,
}

impl DownloadsPanel {
    pub(super) fn new(at: menu::Trigger, cx: &mut Context<Self>) -> Self {
        let downloads = Downloads::global(cx);
        let changed = cx.observe(&downloads, |_, _, cx| cx.notify());
        DownloadsPanel { at, _changed: changed }
    }

    fn row(index: usize, entry: &Entry, t: Tones, cx: &mut Context<Self>) -> Stateful<Div> {
        let key = entry.key;
        let path = entry.path.clone();
        let done = entry.stage == Stage::Done;
        let engine = matches!(key, Key::Engine(_));
        let there = done || (entry.stage == Stage::Going && engine);
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
        let reveal = path.clone().filter(|_| there).map(|path| {
            Self::side(("download-reveal", index), IconName::Search, "Show in Finder", t).on_click(
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    cx.reveal_path(&path);
                    this.at.close(cx);
                }),
            )
        });
        let cancel = (!entry.stage.ended() && engine).then(|| {
            Self::side(("download-cancel", index), IconName::X, "Cancel", t).on_click(cx.listener(
                move |_, _, _, cx| {
                    cx.stop_propagation();
                    Downloads::global(cx).update(cx, |downloads, cx| downloads.cancel(key, cx));
                },
            ))
        });
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
            .child(icon(kind(&entry.name), 20.).text_color(t.muted))
            .child(text)
            .children(reveal)
            .children(cancel)
            .when_some(path.filter(|_| done), |row, path| {
                row.on_click(cx.listener(move |this, _, _, cx| {
                    cx.open_with_system(&path);
                    this.at.close(cx);
                }))
            })
    }

    fn side(id: (&'static str, usize), mark: IconName, says: &'static str, t: Tones) -> Stateful<Div> {
        let (name, at) = id;
        div()
            .id(id)
            .debug_selector(move || format!("{name}-{at}"))
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

/// The mark of a file of `name`, by the kind its extension says it is, as the Finder's icons
/// tell kinds apart.
fn kind(name: &str) -> IconName {
    let extension = Path::new(name).extension().and_then(|extension| extension.to_str());
    match extension.unwrap_or_default().to_ascii_lowercase().as_str() {
        "pdf" | "txt" | "md" | "rtf" | "doc" | "docx" | "pages" | "epub" => IconName::FileText,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "svg" | "tif" | "tiff" | "avif" => {
            IconName::FileImage
        }
        "zip" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "tar" | "dmg" | "pkg"
        | "iso" => IconName::FileArchive,
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "aiff" => IconName::FileMusic,
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" => IconName::FileVideoCamera,
        "csv" | "tsv" | "xls" | "xlsx" | "numbers" => IconName::FileSpreadsheet,
        "json" | "js" | "ts" | "rs" | "py" | "html" | "css" | "sh" | "xml" | "yaml" | "yml"
        | "toml" | "swift" | "go" => IconName::FileCode,
        _ => IconName::File,
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
                .on_click(cx.listener(|this, _, _, cx| {
                    Downloads::global(cx).update(cx, Downloads::clear);
                    if Downloads::listed(cx).is_empty() {
                        this.at.close(cx);
                    }
                }))
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
