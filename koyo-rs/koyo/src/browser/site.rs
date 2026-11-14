//! Site controls (`docs/browser.md` 12): two buttons at the end of the address field, one that
//! copies the page's address and one that opens a popover as Arc's and Zen's: a row of the
//! page's actions, what its site is allowed and blocked from, each turned to the other by a
//! press, and how it's connected. Chromium keeps what a site may do, in the profile; the app
//! asks the host for it and tells it of each change.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Div, ElementId, Entity, Hsla, Image,
    ImageFormat, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use koyo_browser_ipc::{AppRequest, Permission, SiteSetting};

use super::view::BrowserView;
use crate::shell::icon;
use crate::theme::{self, Palette};

mod panel;
mod prompt;
use panel::SitePanel;
pub(super) use prompt::Prompt;

/// What `pane`'s popover opens from.
pub(super) fn trigger(pane: koyo_browser_ipc::PaneId) -> crate::menu::Trigger {
    crate::menu::Trigger::new(ElementId::NamedInteger("site-controls-menu".into(), pane))
}

/// How long the copy button says the address was copied.
const COPIED_FOR: Duration = Duration::from_millis(1500);

impl BrowserView {
    /// The host says what the page's site is allowed and blocked from.
    pub(super) fn site_settings(&mut self, settings: Vec<SiteSetting>, cx: &mut Context<Self>) {
        self.site = Some(settings);
        cx.notify();
    }

    /// Asks the host what the page's site is allowed and blocked from.
    fn ask_site(&mut self, cx: &mut Context<Self>) {
        let pane = self.pane;
        self.send_to_page(AppRequest::SiteSettings { pane }, cx);
    }

    /// Allows the page's site `permission` or blocks it: shown at once, and the host answers
    /// with how its settings stand.
    fn set_site_setting(&mut self, permission: Permission, allowed: bool, cx: &mut Context<Self>) {
        if let Some(settings) = &mut self.site
            && let Some(setting) = (settings.iter_mut()).find(|s| s.permission == permission)
        {
            setting.allowed = allowed;
        }
        let pane = self.pane;
        self.send_to_page(
            AppRequest::SetSiteSetting {
                pane,
                permission,
                allowed,
            },
            cx,
        );
        cx.notify();
    }

    /// The page's whole address to the clipboard; the button says so for a while.
    fn copy_link(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.page.url.clone()));
        self.link_copied = Some(Instant::now());
        // The field's buttons are drawn by whoever lays the field out: the sidebar in the Arc
        // layout, not this view.
        cx.notify();
        cx.refresh_windows();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |_, cx| {
                cx.notify();
                cx.refresh_windows();
            });
        })
        .detach();
    }

    /// A picture of the page as its pane shows it, to the clipboard.
    fn copy_picture(&mut self, cx: &mut Context<Self>) {
        use base64::Engine as _;
        let pane = self.pane;
        let params = serde_json::json!({ "format": "png" });
        let answered = (self.browsers)
            .update(cx, |browsers, _| browsers.call(pane, "Page.captureScreenshot", params));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(shot)) = answered.await else {
                return;
            };
            let data = shot.get("data").and_then(serde_json::Value::as_str);
            let png = data.and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok());
            if let Some(png) = png {
                cx.update(|cx| {
                    let picture = Image::from_bytes(ImageFormat::Png, png);
                    cx.write_to_clipboard(ClipboardItem::new_image(&picture));
                });
            }
        })
        .detach();
    }

    /// Whether the site controls' button is drawn for the page, for a question to come out
    /// of: it has a site, and its address field has been laid out.
    pub(super) fn has_site_button(&self) -> bool {
        self.has_site() && self.site_trigger.drawn().size.width > px(0.)
    }

    /// Whether the page has a site to control: a page of the web's, not its own tools.
    fn has_site(&self) -> bool {
        let url = &self.page.url;
        !self.is_inspector() && (url.starts_with("https://") || url.starts_with("http://"))
    }
}

/// How long the address field's buttons take to come and go with the pointer.
pub const FADE: Duration = Duration::from_millis(150);

/// The buttons at the end of the address field of `page`, which `view` is: copy link, and site
/// controls. None for a page with no site. Their icons are `tint`. They're there while the
/// pointer is over the field, `hovered` of the way in (`theme::hover::eased`), as a tab's
/// close button is; and whatever the pointer does while the popover is up or the address was
/// just copied.
pub fn buttons(
    page: &BrowserView,
    view: &Entity<BrowserView>,
    tint: Hsla,
    hovered: f32,
    cx: &App,
) -> Option<AnyElement> {
    if !page.has_site() {
        return None;
    }
    let p = theme::palette(cx);
    let number = page.pane;
    let copied = (page.link_copied).is_some_and(|at| at.elapsed() < COPIED_FOR);
    let open = page.site_trigger.is_open(cx);
    let copy = {
        let view = view.clone();
        button(("site-copy-link", number), p)
            .debug_selector(|| "site-copy-link".to_owned())
            .when(copied, |button| button.bg(p.selected))
            .child(icon(if copied { IconName::Check } else { IconName::Link }, 14.).text_color(tint))
            .tooltip(|window, cx| Tooltip::new("Copy link").build(window, cx))
            // The bar it's in opens the launcher at a press: this one is the button's alone.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                view.update(cx, BrowserView::copy_link);
            })
    };
    let controls = button(("site-controls", number), p)
        .debug_selector(|| "site-controls".to_owned())
        .when(open, |button| button.bg(p.selected))
        .child(icon(IconName::SlidersHorizontal, 14.).text_color(tint))
        .tooltip(|window, cx| Tooltip::new("Site controls").build(window, cx));
    let trigger = page.site_trigger.clone();
    let controls = {
        let (view, opens) = (view.clone(), trigger.clone());
        trigger.wraps(controls, move |_, window, cx| {
            cx.stop_propagation();
            // Asked for as it opens, not by the press that closes it.
            if !opens.is_open(cx) {
                view.update(cx, BrowserView::ask_site);
            }
            let (page, parent) = (view.downgrade(), window.window_handle());
            let at = opens.clone();
            opens.toggle_panel(|cx| cx.new(|cx| SitePanel::new(page, parent, at, cx)), window, cx);
        })
    };
    let shown = if open || copied { 1. } else { hovered };
    let row = div().flex().items_center().flex_shrink_0().gap(px(2.)).opacity(shown);
    Some(row.child(copy).child(controls).into_any_element())
}

/// A button in the address field.
fn button(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |button| button.bg(p.hover))
        .active(move |button| button.bg(p.selected))
}

/// macOS's share sheet for `address`, from `at` in `window`.
#[cfg(all(target_os = "macos", not(test), not(feature = "visual-check")))]
fn share(address: &str, at: gpui_kit::Bounds<Pixels>, window: &Window, cx: &mut App) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    let Some(view) = crate::native::window::gpui_view(window) else {
        return;
    };
    let points = |pixels: Pixels| f64::from(f32::from(pixels));
    // The view's own origin is at its bottom left.
    let from = NSRect::new(
        NSPoint::new(points(at.left()), view.frame().size.height - points(at.bottom())),
        NSSize::new(points(at.size.width), points(at.size.height)),
    );
    let address = NSString::from_str(address);
    // Once the app is free of the press that asked for it: the sheet runs a loop of its own.
    cx.spawn(async move |_| {
        // SAFETY: Foundation's and AppKit's own, on the main thread: an address, a list of
        // it, and a picker shown from a view that's in a window.
        unsafe {
            let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), URLWithString: &*address];
            let Some(url) = url else {
                return;
            };
            let items: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*url];
            let picker: Retained<AnyObject> =
                msg_send![msg_send![class!(NSSharingServicePicker), alloc], initWithItems: &*items];
            let _: () = msg_send![&*picker, showRelativeToRect: from, ofView: &*view, preferredEdge: 1usize];
        }
    })
    .detach();
}

/// Nothing is shared where no person is: in tests and the visual check.
#[cfg(not(all(target_os = "macos", not(test), not(feature = "visual-check"))))]
fn share(_: &str, _: gpui_kit::Bounds<Pixels>, _: &Window, _: &mut App) {}
