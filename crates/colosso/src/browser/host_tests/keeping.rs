//! What a page keeps: its paint while out of sight, and its site data until cleared.

use std::path::Path;

use colosso_browser_ipc::Go;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};

use super::super::host::HostStatus;
use super::super::view::BrowserView;
use super::{finish, open, prepare, serve, wait_for, wait_for_title};

/// A window showing its page or not, as a tab does once another is picked.
struct Tab {
    page: Entity<BrowserView>,
    shown: bool,
}

impl Render for Tab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.shown.then(|| self.page.clone()))
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_page_out_of_sight_is_hidden_and_paints_again_once_back(cx: &mut TestAppContext) {
    let url = "data:text/html,<title>visible</title><body style='margin:0;background:%230f0'>\
        <script>document.addEventListener('visibilitychange',\
        ()=>document.title=document.visibilityState)</script>";
    let scratch = prepare("sight", cx);
    let (tab, cx) = cx.add_window_view(|window, cx| {
        let page =
            cx.new(|cx| BrowserView::new(url.into(), Path::new("/work/browser-test"), window, cx));
        Tab { page, shown: true }
    });
    let view = tab.read_with(cx, |tab, _| tab.page.clone());
    wait_for(cx, &view, "the page on screen", |view| {
        view.page.title == "visible" && view.frame.is_some()
    });

    tab.update(cx, |tab, cx| {
        tab.shown = false;
        cx.notify();
    });
    wait_for_title(cx, &view, "hidden");
    let painted = view.read_with(cx, |view, _| view.frames_shown);

    tab.update(cx, |tab, cx| {
        tab.shown = true;
        cx.notify();
    });
    wait_for(
        cx,
        &view,
        "the page back on screen and painted afresh",
        |view| view.page.title == "visible" && view.frames_shown > painted,
    );
    finish(cx, scratch);
}

/// A page saying whether the site kept what its last visit stored.
fn store(name: &str) -> Option<String> {
    (name == "store").then(|| {
        "<script>const kept=localStorage.getItem('visited');\
         localStorage.setItem('visited','yes');document.title=kept?'kept':'fresh'</script>"
            .to_owned()
    })
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn clearing_browsing_data_restarts_the_engine_with_nothing_kept(cx: &mut TestAppContext) {
    let port = serve(store);
    let (view, cx, scratch) = open("clear", &format!("http://127.0.0.1:{port}/store"), cx);
    wait_for_title(cx, &view, "fresh");
    view.update(cx, |view, cx| view.go(Go::Reload, cx));
    wait_for_title(cx, &view, "kept");

    cx.update(|_, cx| crate::browser::clear_data(cx));
    wait_for_title(cx, &view, "fresh");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.status(cx), HostStatus::Running);
    });
    finish(cx, scratch);
}
