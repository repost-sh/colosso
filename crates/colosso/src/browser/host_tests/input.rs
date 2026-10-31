//! The person's pointer, wheel and keys reach the page: links followed, fields typed into,
//! pages scrolled, popups answered, addresses entered.

use std::time::Duration;

use gpui_kit::{
    KeyUpEvent, Keystroke, Modifiers, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};

use super::{click, finish, open, page_origin, pause, pixel, serve, wait_for, wait_for_title};

/// Two linked pages and a long one, served locally.
fn site(name: &str) -> Option<String> {
    let link = "position:absolute;left:0;top:0;width:300px;height:60px;font-size:40px";
    match name {
        "one" => Some(format!(
            "<title>one</title><body style='margin:0'><a href='/two' style='{link}'>two</a>"
        )),
        "two" => Some(format!(
            "<title>two</title><body style='margin:0'><a href='/one' style='{link}'>one</a>"
        )),
        "long" => Some(
            "<title>long</title><body style='margin:0;height:5000px;\
             background:linear-gradient(red,blue)'>\
             <script>addEventListener('scroll',()=>document.title='at '+scrollY)</script>"
                .into(),
        ),
        _ => None,
    }
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_clicked_link_is_followed_and_the_keys_go_back_and_forward(cx: &mut TestAppContext) {
    let port = serve(site);
    let (view, cx, scratch) = open("link", &format!("http://127.0.0.1:{port}/one"), cx);
    wait_for(cx, &view, "page one", |view| {
        view.page.title == "one" && view.frame.is_some()
    });
    let (x, y) = page_origin(cx, &view);
    click(cx, x + 30., y + 30.);
    wait_for_title(cx, &view, "two");
    view.read_with(cx, |view, _| assert!(view.page.can_go_back));

    cx.simulate_keystrokes("cmd-[");
    wait_for_title(cx, &view, "one");
    cx.simulate_keystrokes("cmd-]");
    wait_for_title(cx, &view, "two");
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn the_wheel_scrolls_the_page(cx: &mut TestAppContext) {
    let port = serve(site);
    let (view, cx, scratch) = open("wheel", &format!("http://127.0.0.1:{port}/long"), cx);
    wait_for(cx, &view, "the long page", |view| {
        view.page.title == "long" && view.frame.is_some()
    });
    let (x, y) = page_origin(cx, &view);
    let at = point(px(x + 200.), px(y + 200.));
    cx.simulate_mouse_move(at, None, Modifiers::default());
    for _ in 0..5 {
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        pause(Duration::from_millis(16));
    }
    wait_for(cx, &view, "the page scrolled down", |view| {
        view.page
            .title
            .strip_prefix("at ")
            .and_then(|at| at.parse::<f32>().ok())
            .is_some_and(|at| at > 100.)
    });
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn typing_reaches_the_page_and_a_select_popup_answers_keys(cx: &mut TestAppContext) {
    let page = "data:text/html,<title>input</title><body style='margin:0;background:%23f00'>\
        <input oninput=\"document.title='typed '+this.value\"\
          style='position:absolute;left:0;top:0;width:200px;height:30px'>\
        <input oninput=\"document.title='second '+this.value\"\
          style='position:absolute;left:220px;top:0;width:200px;height:30px'>\
        <select onchange=\"document.title='picked '+this.value\"\
          style='position:absolute;left:0;top:60px;width:200px;height:30px'>\
          <option>a</option><option>b</option><option>c</option></select>";
    let (view, cx, scratch) = open("typing", page, cx);
    let scale = cx.update(|window, _| window.scale_factor());
    let device = move |v: f32| (v * scale) as usize;
    // Under the select, where its list opens.
    let below = move |frame: &super::Frame| pixel(frame, device(20.), device(110.));
    wait_for(cx, &view, "the page", |view| {
        view.page.title == "input"
            && view
                .frame
                .as_ref()
                .is_some_and(|f| below(f) == [0, 0, 255, 255])
    });
    let (x, y) = page_origin(cx, &view);

    click(cx, x + 20., y + 15.);
    cx.run_until_parked();
    cx.simulate_input("héllo");
    wait_for_title(cx, &view, "typed héllo");
    cx.simulate_keystrokes("backspace");
    wait_for_title(cx, &view, "typed héll");

    // Tab moves on to the next field, where typing carries on.
    cx.simulate_keystrokes("tab");
    cx.simulate_input("ok");
    wait_for_title(cx, &view, "second ok");

    click(cx, x + 20., y + 75.);
    wait_for(cx, &view, "the open list", |view| {
        view.frame.as_ref().is_some_and(|f| below(f)[2] < 200)
    });
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    wait_for_title(cx, &view, "picked b");
    wait_for(cx, &view, "the closed list", |view| {
        view.frame
            .as_ref()
            .is_some_and(|f| below(f) == [0, 0, 255, 255])
    });
    finish(cx, scratch);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_address_typed_into_the_bar_opens_and_the_page_takes_the_keys(cx: &mut TestAppContext) {
    let port = serve(site);
    let (view, cx, scratch) = open("address", &format!("http://127.0.0.1:{port}/one"), cx);
    wait_for_title(cx, &view, "one");
    cx.update(|window, cx| view.update(cx, |view, cx| view.focus_address(window, cx)));
    cx.simulate_input(&format!("127.0.0.1:{port}/two"));
    cx.simulate_keystrokes("enter");
    wait_for_title(cx, &view, "two");
    let page_focused = cx.update(|window, cx| view.read(cx).focus.is_focused(window));
    assert!(
        page_focused,
        "the page has the keys once its address is entered"
    );
    finish(cx, scratch);
}

/// Presses and lets go of `key`, as a person does.
fn tap(cx: &mut VisualTestContext, key: &'static str) {
    cx.simulate_keystrokes(key);
    let keystroke = Keystroke::parse(key).expect("a key");
    cx.simulate_event(KeyUpEvent { keystroke });
    cx.run_until_parked();
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn each_key_reaches_the_page_once_pressed_and_once_let_go(cx: &mut TestAppContext) {
    let page = "data:text/html,<title>keys</title><body style='margin:0'>\
        <input id=f style='position:absolute;left:0;top:0;width:200px;height:30px'>\
        <script>const log=[];const add=s=>{log.push(s);document.title=log.join(' ')};\
        f.onkeydown=e=>add('d'+e.key);f.onkeyup=e=>add('u'+e.key)</script>";
    let (view, cx, scratch) = open("keys", page, cx);
    wait_for(cx, &view, "the page", |view| {
        view.page.title == "keys" && view.frame.is_some()
    });
    let (x, y) = page_origin(cx, &view);
    click(cx, x + 20., y + 15.);
    cx.run_until_parked();
    tap(cx, "left");
    tap(cx, "a");
    cx.simulate_modifiers_change(Modifiers::shift());
    tap(cx, "shift-b");
    cx.simulate_modifiers_change(Modifiers::default());
    for key in ["backspace", "escape", "enter"] {
        tap(cx, key);
    }
    wait_for_title(
        cx,
        &view,
        "dArrowLeft uArrowLeft da ua dShift dB uB uShift dBackspace uBackspace \
         dEscape uEscape dEnter uEnter",
    );
    finish(cx, scratch);
}
