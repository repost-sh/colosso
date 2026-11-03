//! The theme picker: a popup over the rails for picking the window's gradient. Dots on a
//! disc pick its colors, presets fill them in, a wavy slider sets how opaque they lie over
//! the blur, and a dial sets the grain. It sits in a blurred window of its own, like the
//! dropdown menus, and closes on Escape, a press in the window behind it, or that window
//! losing focus.

use std::time::{Duration, Instant};

use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, BoxShadow, Context, Corners, DispatchPhase,
    ElementId, FontWeight, Global, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PathBuilder, Pixels, Point,
    Render, SpringConfig, SpringState, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div,
    ease_in_out, linear_color_stop, linear_gradient, point, px, size,
};

use crate::backdrop;
use crate::gradient::{self, GradientTheme, MAX_OPACITY, MIN_OPACITY, PAD, PAGES, Swatch};
use crate::theme::{self, ThemePreference, gray};

const WIDTH: f32 = 380.;
const HEIGHT: f32 = 514.;
/// The pad's offset from the panel's top-left corner.
const INSET: f32 = 11.;
const PAD_RADIUS: f32 = 12.;
/// The scheme and dot buttons: their side, and the distance between their centers.
const BUTTON: f32 = 30.;
const BUTTON_SPACING: f32 = 51.;
/// Centers of the scheme row and the dot buttons' row, down the pad.
const SCHEME_ROW: f32 = 30.;
const DOT_ROW: f32 = PAD - 27.;
/// The primary dot and the others, borders included.
const PRIMARY_DOT: f32 = 50.;
const PRIMARY_BORDER: f32 = 6.;
const DOT: f32 = 22.;
const DOT_BORDER: f32 = 3.;
/// The grid of faint dots on the pad.
const GRID_SPACING: f32 = 6.;
const GRID_OFFSET: f32 = 4.;

/// The swatch row's center, the arrows' centers from either side, and the window onto its pages.
const SWATCH_ROW: f32 = 393.;
const SWATCH: f32 = 26.;
const ARROW: f32 = 28.;
const ARROW_CENTER: f32 = 24.5;
const PAGES_LEFT: f32 = 48.;
const PAGE_WIDTH: f32 = 284.;
const PAGE_SCROLL: Duration = Duration::from_millis(300);

/// The center of the slider and dial row.
const CONTROLS_ROW: f32 = 460.;
const TRACK_LEFT: f32 = 24.;
const TRACK_WIDTH: f32 = 242.;
const TRACK_HEIGHT: f32 = 16.;
/// Room the range input leaves at either end of the track.
const TRACK_PADDING: f32 = 5.;
/// Half the slider row's height, which takes presses this far above and below the track.
const SLIDER_REACH: f32 = 30.;
/// The wave starts and ends this far inside the track, and makes this many full turns.
const WAVE_INSET: f32 = 5.5;
const WAVE_TURNS: f32 = 6.;
const WAVE_AMPLITUDE: f32 = 19.5;
const WAVE_STROKE: f32 = 5.5;
const DIAL: f32 = 68.;
const DIAL_CENTER: f32 = 326.;
const DIAL_TICK: f32 = 4.;
/// The knob's share of the dial.
const KNOB: f32 = 0.6;

/// How dots glide to a new spot: a spring that settles in about 0.4s with a little bounce.
const GLIDE: SpringConfig = SpringConfig::new(246.7, 22., 1.);
const GLIDE_TIME: f32 = 0.8;

const SPARKLES: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M14 4.5C14.7 10 18 13.3 23.5 14 18 14.7 14.7 18 14 23.5 13.3 18 10 14.7 4.5 14 10 13.3 13.3 10 14 4.5Z"/><path d="M5.5 1.5C5.8 4 7 5.2 9.5 5.5 7 5.8 5.8 7 5.5 9.5 5.2 7 4 5.8 1.5 5.5 4 5.2 5.2 4 5.5 1.5Z"/></svg>"#;
const SUN: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5"/><g fill="none" stroke="black" stroke-width="1.8" stroke-linecap="round"><path d="M12 2.5v1.6M12 19.9v1.6M2.5 12h1.6M19.9 12h1.6M5.3 5.3l1.1 1.1M17.6 17.6l1.1 1.1M5.3 18.7l1.1-1.1M17.6 6.4l1.1-1.1"/></g></svg>"#;
const MOON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20.5 14.6A8.6 8.6 0 1 1 9.4 3.5 6.8 6.8 0 0 0 20.5 14.6Z"/></svg>"#;
const PLUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
const MINUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 12h14" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
const HARMONY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="black" stroke-width="1.6"><circle cx="12" cy="6.5" r="2.5"/><circle cx="7.5" cy="15" r="2.5"/><circle cx="16.5" cy="15" r="2.5"/></g></svg>"#;
const CHEVRON_LEFT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M15 5l-7 7 7 7" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHEVRON_RIGHT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 5l7 7-7 7" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

/// The picker on screen, if any.
#[derive(Default)]
struct OpenPicker(Option<AnyWindowHandle>);

impl Global for OpenPicker {}

/// Opens the picker with its top-left corner at `position` in `window`, kept on screen.
pub fn open(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    close(cx);
    let display = window.display(cx);
    let mut origin = window.bounds().origin + position;
    if let Some(area) = display.as_ref().map(|display| display.bounds()) {
        origin.x = origin.x.min(area.right() - px(WIDTH)).max(area.left());
        origin.y = origin.y.min(area.bottom() - px(HEIGHT)).max(area.top());
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::new(
            origin,
            size(px(WIDTH), px(HEIGHT)),
        ))),
        titlebar: None,
        focus: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
        display_id: display.map(|display| display.id()),
        ..Default::default()
    };
    let parent = window.window_handle();
    let opened = cx.open_window(options, |picker_window, cx| {
        cx.new(|cx| Picker {
            _subscriptions: [
                // Pressing the picker takes focus from the window behind, so either one
                // losing it only closes the picker once focus has left them both.
                cx.observe_window_activation(window, move |_, _, cx| close_once_left(parent, cx)),
                cx.observe_window_activation(picker_window, move |_, _, cx| {
                    close_once_left(parent, cx);
                }),
                cx.intercept_keystrokes(|event, _, cx| {
                    if event.keystroke.key == "escape" {
                        close(cx);
                    }
                }),
            ],
            drag: None,
            shown: Vec::new(),
            page: 0,
            page_scroll: None,
        })
    });
    if let Ok(handle) = opened {
        cx.set_global(OpenPicker(Some(handle.into())));
    }
}

/// Closes the picker if, once focus has settled, neither it nor `parent` holds it.
fn close_once_left(parent: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx| {
        // The window gaining focus only hears so after the one losing it.
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        cx.update(|cx| {
            let picker = cx.try_global::<OpenPicker>().and_then(|open| open.0);
            // `App::active_window` only knows main windows, and the picker is a panel.
            let focused = |window: AnyWindowHandle, cx: &mut App| {
                window
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false)
            };
            if picker.is_some_and(|picker| !focused(picker, cx) && !focused(parent, cx)) {
                close(cx);
            }
        });
    })
    .detach();
}

/// Closes the picker, if open. It can't reach its own window from inside it.
pub fn close(cx: &mut App) {
    let Some(window) = cx.default_global::<OpenPicker>().0.take() else {
        return;
    };
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

/// A dot button's icon, whether it can act, and what it does to the theme.
type DotAction = (&'static [u8], bool, fn(&mut GradientTheme));

/// A control being dragged; it keeps following the mouse outside its bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Dot,
    Opacity,
    Texture,
}

/// A dot gliding from one spot on the pad to another.
#[derive(Clone, Copy)]
struct Glide {
    from: Point<f32>,
    to: Point<f32>,
    /// When it set off, unless it's resting where it is.
    start: Option<Instant>,
}

impl Glide {
    fn resting(at: Point<f32>) -> Self {
        Glide {
            from: at,
            to: at,
            start: None,
        }
    }

    /// Where the dot is now, and whether it is still moving.
    fn position(&self) -> (Point<f32>, bool) {
        let elapsed = self
            .start
            .map_or(GLIDE_TIME, |start| start.elapsed().as_secs_f32());
        if elapsed >= GLIDE_TIME {
            return (self.to, false);
        }
        let progress = GLIDE.step(SpringState::default(), 1., elapsed).position;
        let at = point(
            self.from.x + (self.to.x - self.from.x) * progress,
            self.from.y + (self.to.y - self.from.y) * progress,
        );
        (at, true)
    }
}

pub struct Picker {
    drag: Option<Control>,
    /// Where each dot is drawn, chasing its spot in the theme.
    shown: Vec<Glide>,
    page: usize,
    /// The page the swatch row scrolled from, and when, while it slides to `page`.
    page_scroll: Option<(f32, Instant)>,
    _subscriptions: [Subscription; 3],
}

impl Picker {
    fn press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let at = event.position.map(Pixels::as_f32);
        let on_pad = point(at.x - INSET, at.y - INSET);
        if inside_pad(on_pad) {
            if on_button(on_pad) {
                return;
            }
            let theme = gradient::current(cx);
            let on_primary = theme
                .dots
                .first()
                .is_some_and(|&primary| distance(primary, on_pad) <= PRIMARY_DOT / 2.);
            match event.button {
                MouseButton::Left if on_primary => self.drag = Some(Control::Dot),
                MouseButton::Left => gradient::update(cx, |theme| theme.move_primary(on_pad)),
                MouseButton::Right if on_primary => {
                    if !self.shown.is_empty() {
                        self.shown.remove(0);
                    }
                    gradient::update(cx, GradientTheme::remove_primary);
                }
                _ => {}
            }
        } else if event.button == MouseButton::Left {
            let slider = !gradient::current(cx).dots.is_empty()
                && (at.y - CONTROLS_ROW).abs() <= SLIDER_REACH
                && (TRACK_LEFT..=TRACK_LEFT + TRACK_WIDTH).contains(&at.x);
            let dial = distance(at, point(DIAL_CENTER, CONTROLS_ROW)) <= DIAL / 2. + DIAL_TICK;
            if slider {
                self.drag = Some(Control::Opacity);
                self.drag_to(at, cx);
            } else if dial {
                self.drag = Some(Control::Texture);
                self.drag_to(at, cx);
            }
        }
        cx.notify();
    }

    fn drag_to(&mut self, at: Point<f32>, cx: &mut Context<Self>) {
        match self.drag {
            Some(Control::Dot) => {
                gradient::update(cx, |theme| {
                    theme.move_primary(point(at.x - INSET, at.y - INSET));
                });
            }
            Some(Control::Opacity) => {
                let before = gradient::current(cx).opacity;
                let (left, travel) = thumb_travel(opacity_progress(before));
                let along = ((at.x - left) / travel).clamp(0., 1.);
                let opacity = MIN_OPACITY + along * (MAX_OPACITY - MIN_OPACITY);
                gradient::update(cx, |theme| theme.set_opacity(opacity));
                // A tick for every tenth crossed.
                let tenth = |opacity: f32| (opacity * 10.).round() as i32;
                if tenth(opacity) != tenth(before) {
                    haptic_tick();
                }
            }
            Some(Control::Texture) => {
                let before = gradient::current(cx).texture;
                let turn = ((at.y - CONTROLS_ROW).atan2(at.x - DIAL_CENTER).to_degrees() + 90.)
                    .rem_euclid(360.)
                    / 360.;
                gradient::update(cx, |theme| theme.set_texture(turn));
                #[expect(
                    clippy::float_cmp,
                    reason = "the texture snaps to detents, so any change is a whole detent"
                )]
                let moved = gradient::current(cx).texture != before;
                if moved {
                    haptic_tick();
                }
            }
            None => {}
        }
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn show_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page_scroll = Some((self.page_offset().0, Instant::now()));
        self.page = page;
        cx.notify();
    }

    /// How far the swatch row has scrolled, in pages, and whether it is still sliding.
    fn page_offset(&self) -> (f32, bool) {
        let target = self.page as f32;
        let Some((from, start)) = self.page_scroll else {
            return (target, false);
        };
        let progress = start.elapsed().as_secs_f32() / PAGE_SCROLL.as_secs_f32();
        if progress >= 1. {
            return (target, false);
        }
        (from + (target - from) * ease_in_out(progress), true)
    }

    /// Moves each drawn dot toward its spot: gliding there, or straight there while dragged.
    fn chase_dots(&mut self, theme: &GradientTheme) -> bool {
        self.shown.truncate(theme.dots.len());
        for (index, &target) in theme.dots.iter().enumerate() {
            match self.shown.get_mut(index) {
                None => self.shown.push(Glide::resting(target)),
                Some(glide) if glide.to != target => {
                    *glide = if self.drag == Some(Control::Dot) {
                        Glide::resting(target)
                    } else {
                        Glide {
                            from: glide.position().0,
                            to: target,
                            start: Some(Instant::now()),
                        }
                    };
                }
                Some(_) => {}
            }
        }
        self.shown.iter().any(|glide| glide.position().1)
    }

    fn pad(&self, theme: &GradientTheme, colors: &Colors, cx: &App) -> impl IntoElement {
        let preference = theme::preference(cx);
        let has_dots = !theme.dots.is_empty();
        let schemes = [
            (ThemePreference::System, SPARKLES),
            (ThemePreference::Light, SUN),
            (ThemePreference::Dark, MOON),
        ];
        let actions: [DotAction; 3] = [
            (PLUS, theme.can_add(), GradientTheme::add_dot),
            (MINUS, has_dots, GradientTheme::remove_dot),
            (
                HARMONY,
                theme.can_cycle_harmony(),
                GradientTheme::cycle_harmony,
            ),
        ];
        let grid = colors.grid;
        div()
            .absolute()
            .left(px(INSET))
            .top(px(INSET))
            .size(px(PAD))
            .rounded(px(PAD_RADIUS))
            .overflow_hidden()
            .bg(colors.pad)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| {
                        let mut y = GRID_OFFSET;
                        while y < PAD {
                            let mut x = GRID_OFFSET;
                            while x < PAD {
                                let origin = bounds.origin + point(px(x - 1.), px(y - 1.));
                                window.paint_quad(
                                    gpui_kit::fill(
                                        gpui_kit::Bounds::new(origin, size(px(2.), px(2.))),
                                        grid,
                                    )
                                    .corner_radii(px(1.)),
                                );
                                x += GRID_SPACING;
                            }
                            y += GRID_SPACING;
                        }
                    },
                )
                .size_full(),
            )
            .when(!has_dots, |pad| {
                pad.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.icon)
                        .child("Click to add a color"),
                )
            })
            .children(self.shown.iter().zip(theme.colors()).enumerate().rev().map(
                |(index, (glide, color))| {
                    let dragged = index == 0 && self.drag == Some(Control::Dot);
                    dot(index, glide.position().0, color, dragged, colors.dark)
                },
            ))
            .children(
                schemes
                    .into_iter()
                    .enumerate()
                    .map(|(index, (scheme, svg))| {
                        pad_button(("scheme", index), index, SCHEME_ROW, svg, true, colors)
                            .when(scheme == preference, |button| button.bg(colors.hover))
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(scheme.action(), cx);
                            })
                    }),
            )
            .children(
                actions
                    .into_iter()
                    .enumerate()
                    .map(|(index, (svg, enabled, action))| {
                        pad_button(("dot-action", index), index, DOT_ROW, svg, enabled, colors)
                            .when(enabled, |button| {
                                button.on_click(move |_, _, cx| gradient::update(cx, action))
                            })
                    }),
            )
    }

    fn swatches(&self, colors: &Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let last = PAGES.len() - 1;
        let (offset, _) = self.page_offset();
        let page = self.page;
        div()
            .absolute()
            .left_0()
            .top(px(SWATCH_ROW - ARROW / 2.))
            .w(px(WIDTH))
            .h(px(ARROW))
            .child(
                arrow("page-left", ARROW_CENTER, CHEVRON_LEFT, page > 0, colors).when(
                    page > 0,
                    |arrow| {
                        arrow.on_click(
                            cx.listener(move |this, _, _, cx| this.show_page(page - 1, cx)),
                        )
                    },
                ),
            )
            .child(
                arrow(
                    "page-right",
                    WIDTH - ARROW_CENTER,
                    CHEVRON_RIGHT,
                    page < last,
                    colors,
                )
                .when(page < last, |arrow| {
                    arrow.on_click(cx.listener(move |this, _, _, cx| this.show_page(page + 1, cx)))
                }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(PAGES_LEFT))
                    .w(px(PAGE_WIDTH))
                    .h_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .left(px(-offset * PAGE_WIDTH))
                            .h_full()
                            .flex()
                            .children(PAGES.iter().enumerate().map(|(page, presets)| {
                                // Only the pages in view get swatches; the rest hold their place.
                                let shown = (page as f32 - offset).abs() < 1.;
                                div()
                                    .w(px(PAGE_WIDTH))
                                    .h_full()
                                    .flex_shrink_0()
                                    .px_px()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .when(shown, |page_row| {
                                        page_row.children(presets.iter().enumerate().map(
                                            move |(index, preset)| {
                                                swatch((page, index), preset.swatch).on_click(
                                                    move |_, _, cx| {
                                                        gradient::update(cx, |theme| {
                                                            theme.apply_preset(&PAGES[page][index]);
                                                        });
                                                    },
                                                )
                                            },
                                        ))
                                    })
                            })),
                    ),
            )
    }

    fn slider(&self, theme: &GradientTheme, colors: &Colors) -> impl IntoElement {
        // Without colors there is nothing to see through: the wave lies dim at full swing
        // and the thumb is gone.
        let enabled = !theme.dots.is_empty();
        let progress = if enabled {
            opacity_progress(theme.opacity)
        } else {
            1.
        };
        let (thumb_width, thumb_height) = thumb_size(progress);
        let (left, travel) = thumb_travel(progress);
        let thumb_x = left + progress * travel;
        let split = if enabled { thumb_x } else { f32::NEG_INFINITY };
        let wave = Wave {
            progress,
            split,
            track: colors.track,
            bright: colors.wave,
            dim: colors.wave_dim,
        };
        let top = CONTROLS_ROW - SLIDER_REACH;
        div()
            .absolute()
            .left_0()
            .top(px(top))
            .w(px(WIDTH))
            .h(px(SLIDER_REACH * 2.))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| wave.paint(bounds, window),
                )
                .size_full(),
            )
            .when(enabled, |slider| {
                let thumb = |scale: f32| {
                    let (w, h) = (thumb_width * scale, thumb_height * scale);
                    (
                        px(thumb_x - w / 2.),
                        px(SLIDER_REACH - h / 2.),
                        px(w),
                        px(h),
                    )
                };
                let (left, top, w, h) = thumb(1.);
                let (hover_left, hover_top, hover_w, hover_h) = thumb(1.1);
                slider.child(
                    div()
                        .id("opacity-thumb")
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(w)
                        .h(h)
                        .rounded_full()
                        .bg(colors.thumb)
                        .cursor_pointer()
                        .hover(move |style| {
                            style.left(hover_left).top(hover_top).w(hover_w).h(hover_h)
                        }),
                )
            })
    }

    fn dial(&self, theme: &GradientTheme, colors: &Colors) -> impl IntoElement {
        let texture = theme.texture;
        let (tick, handle) = (colors.tick, colors.handle);
        let knob = DIAL * KNOB;
        div()
            .id("texture-dial")
            .group("texture-dial")
            .absolute()
            .left(px(DIAL_CENTER - DIAL / 2.))
            .top(px(CONTROLS_ROW - DIAL / 2.))
            .size(px(DIAL))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left(px((DIAL - knob) / 2.))
                    .top(px((DIAL - knob) / 2.))
                    .size(px(knob))
                    .rounded_full()
                    .overflow_hidden()
                    .border_1()
                    .border_color(colors.knob_edge)
                    .bg(linear_gradient(
                        315.,
                        linear_color_stop(colors.knob_shine.opacity(0.), 0.),
                        linear_color_stop(colors.knob_shine, 1.),
                    ))
                    .when(texture > 0., |knob_face| {
                        knob_face.child(backdrop::grain(texture, Corners::all(px(knob / 2.))))
                    }),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| {
                        let center = bounds.center();
                        let radius = DIAL / 2.;
                        for index in 0..16 {
                            let angle = index as f32 / 16. * std::f32::consts::TAU;
                            // The first tick sits a quarter turn past the top, where the dial starts.
                            let value = ((index + 4) % 16) as f32 / 16.;
                            let alpha = if value <= texture { 1. } else { 0.4 };
                            let at = center
                                + point(px(angle.cos() * radius), px(angle.sin() * radius))
                                - point(px(DIAL_TICK / 2.), px(DIAL_TICK / 2.));
                            window.paint_quad(
                                gpui_kit::fill(
                                    gpui_kit::Bounds::new(at, size(px(DIAL_TICK), px(DIAL_TICK))),
                                    tick.opacity(tick.a * alpha),
                                )
                                .corner_radii(px(DIAL_TICK / 2.)),
                            );
                        }
                        paint_handle(center, radius, texture, handle, window);
                    },
                )
                .size_full(),
            )
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let colors = Colors::new(cx.theme().mode.is_dark());
        let theme = gradient::current(cx).clone();
        let gliding = self.chase_dots(&theme);
        let (_, scrolling) = self.page_offset();
        if gliding || scrolling {
            window.request_animation_frame();
        }
        let picker = cx.entity().downgrade();
        div()
            .size_full()
            .relative()
            .bg(p.menu)
            .text_color(p.text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.press(event, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event, _, cx| this.press(event, cx)),
            )
            .child(self.pad(&theme, &colors, cx))
            .child(self.swatches(&colors, cx))
            .child(self.slider(&theme, &colors))
            .child(self.dial(&theme, &colors))
            .when(self.drag.is_some(), |root| {
                // Follows the mouse anywhere until the button comes up.
                root.child(
                    canvas(
                        |_, _, _| {},
                        move |_, (), window, _| {
                            let moved = picker.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    let at = event.position.map(Pixels::as_f32);
                                    moved.update(cx, |this, cx| this.drag_to(at, cx)).ok();
                                }
                            });
                            let released = picker;
                            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    released.update(cx, Picker::release).ok();
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            })
    }
}

/// The picker's own colors, for light and dark.
struct Colors {
    dark: bool,
    pad: Hsla,
    grid: Hsla,
    icon: Hsla,
    hover: Hsla,
    track: Hsla,
    wave: Hsla,
    wave_dim: Hsla,
    thumb: Hsla,
    tick: Hsla,
    handle: Hsla,
    knob_edge: Hsla,
    knob_shine: Hsla,
}

impl Colors {
    fn new(dark: bool) -> Self {
        // Light ink on dark, dark ink on light.
        let ink = |alpha: f32| {
            if dark {
                gray(1., alpha)
            } else {
                gray(0., alpha)
            }
        };
        Colors {
            dark,
            pad: ink(if dark { 0.03 } else { 0.05 }),
            grid: ink(if dark { 0.05 } else { 0.1 }),
            icon: ink(if dark { 0.9 } else { 0.7 }),
            hover: ink(0.1),
            track: ink(0.1),
            wave: if dark { gray(0.63, 1.) } else { gray(0.35, 1.) },
            wave_dim: if dark {
                gray(0.63, 0.5)
            } else {
                gray(0.3, 0.5)
            },
            thumb: ink(1.),
            tick: ink(0.3),
            handle: if dark { gray(0.82, 1.) } else { gray(0.46, 1.) },
            knob_edge: ink(0.05),
            knob_shine: ink(0.1),
        }
    }
}

fn dot(
    index: usize,
    at: Point<f32>,
    color: gradient::Rgb,
    dragged: bool,
    dark: bool,
) -> AnyElement {
    let primary = index == 0;
    let (side, border) = if primary {
        (PRIMARY_DOT, PRIMARY_BORDER)
    } else {
        (DOT, DOT_BORDER)
    };
    let place = |scale: f32| {
        let side = side * scale;
        (px(at.x - side / 2.), px(at.y - side / 2.), px(side))
    };
    let (left, top, side) = place(if dragged { 1.2 } else { 1. });
    let (hover_left, hover_top, hover_side) = place(1.05);
    div()
        .id(("dot", index))
        .absolute()
        .left(left)
        .top(top)
        .size(side)
        .rounded_full()
        .border(px(border))
        .border_color(gray(1., 1.))
        .bg(gradient::rgba(color, 1.))
        .when(dark, |dot| {
            dot.shadow(vec![BoxShadow {
                color: gray(0., 0.1),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: false,
            }])
        })
        .when(primary && !dragged, |dot| {
            dot.cursor_pointer()
                .hover(move |style| style.left(hover_left).top(hover_top).size(hover_side))
        })
        .into_any_element()
}

/// A button on the pad, the `index`th of three centered on a row at `row` down it.
fn pad_button(
    id: impl Into<ElementId>,
    index: usize,
    row: f32,
    svg: &'static [u8],
    enabled: bool,
    colors: &Colors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let center = PAD / 2. + (index as f32 - 1.) * BUTTON_SPACING;
    let hover = colors.hover;
    div()
        .id(id)
        .absolute()
        .left(px(center - BUTTON / 2.))
        .top(px(row - BUTTON / 2.))
        .size(px(BUTTON))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(colors.icon)
        .when(enabled, |button| {
            button.cursor_pointer().hover(move |style| style.bg(hover))
        })
        .when(!enabled, |button| button.opacity(0.5))
        .child(Icon::default().data(svg).size(px(18.)))
}

fn arrow(
    id: &'static str,
    center: f32,
    svg: &'static [u8],
    enabled: bool,
    colors: &Colors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover = colors.hover;
    div()
        .id(id)
        .absolute()
        .left(px(center - ARROW / 2.))
        .size(px(ARROW))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(colors.icon)
        .when(enabled, |arrow| {
            arrow.cursor_pointer().hover(move |style| style.bg(hover))
        })
        .when(!enabled, |arrow| arrow.opacity(0.4))
        .child(Icon::default().data(svg).size(px(16.)))
}

/// A preset's swatch: grows a little under the mouse and shrinks while pressed.
fn swatch(id: (usize, usize), swatch: Swatch) -> gpui_kit::Stateful<gpui_kit::Div> {
    let group: gpui_kit::SharedString = format!("swatch-{}-{}", id.0, id.1).into();
    let face = div()
        .absolute()
        .size(px(SWATCH))
        .rounded_full()
        .overflow_hidden()
        .shadow(vec![BoxShadow {
            color: gray(0., 0.1),
            offset: point(px(0.), px(0.)),
            blur_radius: px(1.),
            spread_radius: px(1.),
            inset: false,
        }])
        .group_hover(group.clone(), |style| {
            let grown = SWATCH * 1.05;
            style
                .size(px(grown))
                .left(px((SWATCH - grown) / 2.))
                .top(px((SWATCH - grown) / 2.))
        });
    let face = match swatch {
        Swatch::Solid(color) => face.bg(gradient::rgba(color, 1.)),
        Swatch::Blend(colors) => {
            face.child(backdrop::swatch(colors, Corners::all(px(SWATCH / 2.))))
        }
    };
    div()
        .id(ElementId::from(("swatch", id.0 * 16 + id.1)))
        .group(group)
        .relative()
        .size(px(SWATCH))
        .flex_shrink_0()
        .cursor_pointer()
        .child(face)
}

/// Draws the dial's handle: a small bar on the rim at `texture`'s turn, pointing outward.
fn paint_handle(
    center: Point<Pixels>,
    radius: f32,
    texture: f32,
    color: Hsla,
    window: &mut Window,
) {
    let angle = texture * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    // Along the radius, then across it.
    let outward = (sin, -cos);
    let across = (cos, sin);
    let (half_length, half_width, corner) = (6., 3., 2.);
    let rim = (
        center.x.as_f32() + outward.0 * radius,
        center.y.as_f32() + outward.1 * radius,
    );
    let at = |along: f32, side: f32| {
        point(
            px(rim.0 + outward.0 * along + across.0 * side),
            px(rim.1 + outward.1 * along + across.1 * side),
        )
    };
    // A rounded rectangle, its corners cut into short arcs.
    let mut path = PathBuilder::fill();
    let corners = [
        (half_length - corner, half_width - corner, 0.),
        (-(half_length - corner), half_width - corner, 90.),
        (-(half_length - corner), -(half_width - corner), 180.),
        (half_length - corner, -(half_width - corner), 270.),
    ];
    let mut first = true;
    for (along, side, start) in corners {
        for step in 0..=4 {
            let turn = (start + step as f32 * 22.5_f32).to_radians();
            let point = at(along + turn.cos() * corner, side + turn.sin() * corner);
            if first {
                path.move_to(point);
                first = false;
            } else {
                path.line_to(point);
            }
        }
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn inside_pad(at: Point<f32>) -> bool {
    (0. ..=PAD).contains(&at.x) && (0. ..=PAD).contains(&at.y)
}

/// Whether `at` on the pad falls on one of its buttons, which take their own presses.
fn on_button(at: Point<f32>) -> bool {
    [SCHEME_ROW, DOT_ROW].into_iter().any(|row| {
        (0..3).any(|index| {
            let center = point(PAD / 2. + (index as f32 - 1.) * BUTTON_SPACING, row);
            (at.x - center.x).abs() <= BUTTON / 2. && (at.y - center.y).abs() <= BUTTON / 2.
        })
    })
}

fn distance(a: Point<f32>, b: Point<f32>) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Where `opacity` sits on the slider, from 0 to 1.
fn opacity_progress(opacity: f32) -> f32 {
    ((opacity - MIN_OPACITY) / (MAX_OPACITY - MIN_OPACITY)).clamp(0., 1.)
}

/// The thumb grows with the slider's value.
fn thumb_size(progress: f32) -> (f32, f32) {
    (10. + progress * 15., 40. + progress * 15.)
}

/// Where the thumb's center sits at the start of the track, and how far it travels.
fn thumb_travel(progress: f32) -> (f32, f32) {
    let width = thumb_size(progress).0;
    (
        TRACK_LEFT + TRACK_PADDING + width / 2.,
        TRACK_WIDTH - TRACK_PADDING * 2. - width,
    )
}

/// The opacity slider's track and wave, bright up to the thumb at `split` and dim past it,
/// swinging wider the higher `progress` is.
#[derive(Clone, Copy)]
struct Wave {
    progress: f32,
    split: f32,
    track: Hsla,
    bright: Hsla,
    dim: Hsla,
}

impl Wave {
    fn paint(&self, bounds: gpui_kit::Bounds<Pixels>, window: &mut Window) {
        let origin = bounds.origin;
        let at = |x: f32, y: f32| origin + point(px(x), px(y));
        let center = SLIDER_REACH;
        window.paint_quad(
            gpui_kit::fill(
                gpui_kit::Bounds::new(
                    at(TRACK_LEFT, center - TRACK_HEIGHT / 2.),
                    size(px(TRACK_WIDTH), px(TRACK_HEIGHT)),
                ),
                self.track,
            )
            .corner_radii(px(TRACK_HEIGHT / 2.)),
        );
        let start = TRACK_LEFT + WAVE_INSET;
        let end = TRACK_LEFT + TRACK_WIDTH - WAVE_INSET;
        let amplitude = WAVE_AMPLITUDE * self.progress;
        let wave_y = |x: f32| {
            let turn = (x - start) / (end - start) * WAVE_TURNS;
            center - amplitude * (turn * std::f32::consts::TAU).sin()
        };
        // The wave is bright up to the thumb and dim past it.
        for (from, to, color) in [
            (start, self.split.min(end), self.bright),
            (self.split.max(start), end, self.dim),
        ] {
            if to - from < 0.5 {
                continue;
            }
            let mut path = PathBuilder::stroke(px(WAVE_STROKE));
            path.move_to(at(from, wave_y(from)));
            let mut x = from;
            while x < to {
                x = (x + 2.).min(to);
                path.line_to(at(x, wave_y(x)));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
            // Round caps.
            for x in [from, to] {
                let cap = gpui_kit::Bounds::new(
                    at(x - WAVE_STROKE / 2., wave_y(x) - WAVE_STROKE / 2.),
                    size(px(WAVE_STROKE), px(WAVE_STROKE)),
                );
                window.paint_quad(gpui_kit::fill(cap, color).corner_radii(px(WAVE_STROKE / 2.)));
            }
        }
    }
}

/// A light tap on the trackpad, as each detent passes.
// objc's `msg_send!` checks a `cargo-clippy` cfg that current Rust no longer declares.
#[expect(unexpected_cfgs, reason = "objc's `msg_send!` checks the cfg itself")]
fn haptic_tick() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};
        // NSHapticFeedbackPatternLevelChange, performed now.
        let performer: *mut Object = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
        let _: () = msg_send![performer, performFeedbackPattern: 2isize performanceTime: 1usize];
    }
}
