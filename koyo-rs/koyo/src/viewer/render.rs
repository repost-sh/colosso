//! How a viewer looks: its pages filling the tab, and under them a bar saying what the file is
//! (its kind, its size in pixels or which page is in sight, and how big it is on disk), with
//! the zoom, the buttons that change it and one that opens the file in the app the system
//! opens it with.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::CONTEXT;
use super::Kind;
use super::canvas::{layout, page_in_sight};
use super::element::Pages;
use super::input::{ActualSize, ZoomIn, ZoomOut, ZoomToFit};
use super::view::{State, Viewer};
use crate::modal::spinner;
use crate::shell::{icon, icon_button};
use crate::theme::{self, Palette};

/// The bar's height.
const BAR: f32 = 26.;

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let body = (div().flex_1().min_h_0().relative().overflow_hidden())
            .debug_selector(|| "viewer-pages".into());
        let body = match &self.state {
            State::Opening => body.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(spinner(p.muted)),
            ),
            State::Failed(why) => body.child(failed(why.clone(), p)),
            State::Open => body.child(self.pages(p, cx)),
        };
        div()
            .id("viewer")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_to_fit))
            .on_action(cx.listener(Self::actual_size))
            .on_action(cx.listener(Self::scroll_up))
            .on_action(cx.listener(Self::scroll_down))
            .on_action(cx.listener(Self::scroll_left))
            .on_action(cx.listener(Self::scroll_right))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .child(body)
            .child(self.bar(p, cx))
    }
}

impl Viewer {
    /// The element that paints the pages, with what's drawn of them: older tiles first, so
    /// those drawn at the zoom shown lie on top.
    fn pages(&self, p: Palette, cx: &mut Context<Self>) -> Pages {
        let mut tiles: Vec<_> = self.stale.clone();
        let mut drawn: Vec<_> = (self.tiles.iter())
            .map(|(key, image)| (*key, image.clone()))
            .collect();
        drawn.sort_by_key(|(key, _)| (key.level == self.level, key.level));
        tiles.extend(drawn);
        let see_through = match self.kind {
            Kind::Picture => self.probe.as_ref().is_some_and(|probe| probe.see_through),
            Kind::Svg => true,
            Kind::Pdf => false,
        };
        Pages {
            view: cx.entity(),
            sizes: self.pages.clone(),
            camera: self.camera,
            fit: self.kind.fit(),
            picture: self.picture.clone().map(|picture| (picture, self.frame)),
            tiles,
            paper: (self.kind == Kind::Pdf).then_some(p.hairline),
            checker: see_through.then(|| cx.theme().mode.is_dark()),
            dragging: self.drag.is_some(),
        }
    }

    /// The bar under the pages.
    fn bar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let open = matches!(self.state, State::Open);
        let path = self.path.clone();
        let zoom = div()
            .id("viewer-zoom")
            .px_1()
            .min_w(px(42.))
            .flex()
            .justify_center()
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(p.hover))
            .child(format!("{:.0}%", self.camera.zoom * 100.))
            .tooltip(|window, cx| {
                Tooltip::new("Actual Size")
                    .action(&ActualSize, Some(CONTEXT))
                    .build(window, cx)
            })
            .on_click(cx.listener(|this, _, _, cx| this.zoom_to(1., None, cx)));
        div()
            .h(px(BAR))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .pl_3()
            .pr_1()
            .border_t_1()
            .border_color(p.hairline)
            .text_xs()
            .text_color(p.muted)
            .child(div().min_w_0().truncate().child(self.facts()))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_0p5()
                    .when(open, |bar| {
                        bar.child(button(
                            "viewer-zoom-out",
                            IconName::ZoomOut,
                            "Zoom Out",
                            &ZoomOut,
                            cx,
                        ))
                        .child(zoom)
                        .child(button(
                            "viewer-zoom-in",
                            IconName::ZoomIn,
                            "Zoom In",
                            &ZoomIn,
                            cx,
                        ))
                        .child(button(
                            "viewer-fit",
                            IconName::Scan,
                            "Zoom to Fit",
                            &ZoomToFit,
                            cx,
                        ))
                    })
                    .child(
                        icon_button(
                            "viewer-open",
                            IconName::SquareArrowOutUpRight,
                            "Open in Default App",
                            cx,
                        )
                        .xsmall()
                        .on_click(move |_, _, cx| cx.open_with_system(&path)),
                    ),
            )
    }

    /// What the file is: its kind, its size in pixels or the page in sight, and its size on
    /// disk.
    fn facts(&self) -> SharedString {
        let mut facts = Vec::new();
        if let Some(extension) = self.path.extension() {
            facts.push(extension.to_string_lossy().to_uppercase());
        }
        match (&self.probe, self.kind) {
            (Some(probe), Kind::Picture) => {
                facts.push(format!("{} × {}", probe.pixels.0, probe.pixels.1));
                if probe.density > 1 {
                    facts.push(format!("@{}x", probe.density));
                }
                if probe.delays.len() > 1 {
                    facts.push(format!("{} frames", probe.delays.len()));
                }
            }
            (_, Kind::Svg) => {
                if let Some((width, height)) = self.pages.first() {
                    facts.push(format!("{} × {}", width.round(), height.round()));
                }
            }
            (_, Kind::Pdf) if !self.pages.is_empty() => {
                let laid = layout(&self.pages, self.camera.zoom);
                let page = page_in_sight(&laid, self.viewport, self.camera.scroll) + 1;
                facts.push(format!("Page {page} of {}", self.pages.len()));
            }
            _ => {}
        }
        if let Some((bytes, _)) = self.stamp {
            facts.push(size_on_disk(bytes));
        }
        facts.join(" · ").into()
    }
}

/// A button doing `action`'s work to this viewer, its tooltip naming the key for it.
fn button(
    id: &'static str,
    name: IconName,
    tooltip: &'static str,
    action: &dyn Action,
    cx: &mut Context<Viewer>,
) -> impl IntoElement + use<> {
    let action = action.boxed_clone();
    icon_button(id, name, tooltip, cx)
        .xsmall()
        .tooltip_with_action(tooltip, action.as_ref(), Some(CONTEXT))
        .on_click(cx.listener(move |this, _, window, cx| {
            // The viewer takes the keys, as clicking its pages would.
            window.focus(&this.focus, cx);
            window.dispatch_action(action.boxed_clone(), cx);
        }))
}

/// Why the file isn't shown, where its pages would be.
fn failed(why: SharedString, p: Palette) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .px_4()
        .text_sm()
        .text_color(p.muted)
        .child(icon(IconName::FileX, 14.))
        .child(why)
}

/// `bytes` as a person reads a file's size: in bytes, or kilobytes and up to one decimal.
pub fn size_on_disk(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut size = bytes as f64 / 1000.;
    let mut unit = 0;
    while size >= 1000. && unit + 1 < UNITS.len() {
        size /= 1000.;
        unit += 1;
    }
    if size < 10. {
        format!("{size:.1} {}", UNITS[unit])
    } else {
        format!("{size:.0} {}", UNITS[unit])
    }
}
