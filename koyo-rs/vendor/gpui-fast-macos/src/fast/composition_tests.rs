// Modified by Longbridge for gpui-fast.
use super::{
    LayerScene, MacComposition, MacGpuiSurface, draw_layer, draws_in_transaction, end_frame,
    link_layer, mount_in_order, set_presents_with_transaction, start_links, stop_links,
    surface_renderer, whole_scene_layer,
};
use crate::{MetalFrames, TestLink, TestLinkState};
use std::{cell::RefCell, ptr::NonNull, rc::Rc};
use gpui::{
    AppContext as _, Bounds, ComposedSceneLayer, CompositionSurfaceId, ContentMask, Context,
    DevicePixels, IntoElement, ParentElement as _, PlatformHeadlessRenderer as _, Quad, Render,
    ScaledPixels, Scene, Styled as _, TestAppContext, Window, div, point, px, size,
};

use crate::metal_renderer::MetalHeadlessRenderer;
use crate::renderer::MetalRenderer;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

#[gpui::test]
fn whole_scene_with_empty_layers_matches_balanced_replay(cx: &mut TestAppContext) {
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size(px(64.)).bg(gpui::white()).child(
                div()
                    .size(px(64.))
                    .rounded(px(24.))
                    .overflow_hidden()
                    .child(div().size(px(64.)).bg(gpui::black())),
            )
        }
    }

    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| View))
            .unwrap()
    });
    let mut renderer = MetalHeadlessRenderer::new().unwrap();
    window
        .update(cx, |_, window, cx| {
            window.draw(cx).clear(cx);
            let scene = window.painted_scene();
            assert!(
                !scene.layers.frames.is_empty(),
                "rounded clips were compiled"
            );
            let full = ComposedSceneLayer {
                surface: CompositionSurfaceId::default(),
                ranges: vec![0..scene.len()],
            };
            let empty = ComposedSceneLayer {
                surface: CompositionSurfaceId::default(),
                ranges: vec![],
            };

            // The existing layer_scene path replays the original operations,
            // including rounded clips, and finishes the new surface scene.
            let mut replayed = Scene::default();
            replayed
                .replay_balanced(full.ranges[0].clone(), scene)
                .unwrap();
            replayed.finish();

            let side = (64. * window.scale_factor()) as i32;
            let extent = size(DevicePixels(side), DevicePixels(side));
            let expected = renderer.render_scene_to_image(&replayed, extent).unwrap();
            for (layers, expected_target) in [
                (vec![full.clone(), empty.clone()], 0),
                (vec![empty.clone(), full.clone()], 1),
                (vec![full.clone()], 0),
                (vec![empty.clone(), full, empty], 1),
            ] {
                let (target_index, borrowed) = whole_scene_layer(scene, &layers).unwrap();
                assert_eq!(target_index, expected_target);
                assert!(std::ptr::eq(borrowed, scene));
                let actual = renderer.render_scene_to_image(borrowed, extent).unwrap();
                assert_eq!(actual.as_raw(), expected.as_raw());
                assert_eq!(actual.get_pixel(1, 1).0, [255, 255, 255, 255]);
                assert_eq!(
                    actual.get_pixel(side as u32 / 2, side as u32 / 2).0,
                    [0, 0, 0, 255]
                );
            }
        })
        .unwrap();
}

#[test]
fn partial_or_split_layers_require_replay_even_when_primitives_match() {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(64.), ScaledPixels(64.)),
    );
    scene.push_layer(bounds);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        ..Default::default()
    });
    scene.pop_layer();
    scene.finish();
    let len = scene.len();
    let layer = |ranges| ComposedSceneLayer {
        surface: CompositionSurfaceId::default(),
        ranges,
    };
    // The inner range has the same sole primitive, but must restore its layer.
    for layers in [
        vec![layer(vec![1..len - 1])],
        vec![layer(vec![0..len - 1])],
        vec![layer(vec![1..len])],
        vec![layer(vec![0..1, 1..len])],
        vec![layer(vec![0..len]), layer(vec![0..len])],
        vec![layer(vec![0..len]), layer(vec![1..len - 1])],
        vec![layer(vec![0..len]), layer(vec![0..0])],
        vec![layer(vec![])],
        vec![layer(vec![]), layer(vec![])],
        vec![layer(vec![0..len + 1])],
        vec![],
    ] {
        assert!(whole_scene_layer(&scene, &layers).is_none(), "{layers:?}");
    }

    let mut empty = Scene::default();
    empty.finish();
    assert!(whole_scene_layer(&empty, &[layer(vec![])]).is_none());
    for (layers, expected_target) in [
        (vec![layer(vec![0..0])], 0),
        (vec![layer(vec![]), layer(vec![0..0]), layer(vec![])], 1),
    ] {
        let (target_index, borrowed) = whole_scene_layer(&empty, &layers).unwrap();
        assert_eq!(target_index, expected_target);
        assert!(std::ptr::eq(borrowed, &empty));
    }
    assert!(whole_scene_layer(&empty, &[layer(vec![0..0]), layer(vec![0..0])]).is_none());
}

static LEFT_PARENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

extern "C" fn tracked_will_move_to_superview(_: &objc2::runtime::AnyObject, _: objc2::runtime::Sel, to: *mut objc2::runtime::AnyObject) {
    if to.is_null() {
        LEFT_PARENT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn tracked_view() -> *mut objc2::runtime::AnyObject {
    use objc2::{class, msg_send, runtime::{AnyClass, ClassBuilder}, sel};
    let class = AnyClass::get(c"KoyoTrackedView").unwrap_or_else(|| {
        let mut decl = ClassBuilder::new(c"KoyoTrackedView", class!(NSView)).unwrap();
        // SAFETY: the method has `viewWillMoveToSuperview:`'s signature.
        unsafe {
            decl.add_method(
                sel!(viewWillMoveToSuperview:),
                tracked_will_move_to_superview as extern "C" fn(_, _, _),
            );
        }
        decl.register()
    });
    // SAFETY: `NSView`'s initializer, on a class made from it.
    unsafe {
        let view: *mut objc2::runtime::AnyObject = msg_send![class, alloc];
        msg_send![view, initWithFrame: objc2_foundation::NSRect::ZERO]
    }
}

fn subviews(parent: *mut objc2::runtime::AnyObject) -> Vec<usize> {
    use objc2::msg_send;
    // SAFETY: a live view's subviews, an array of views.
    unsafe {
        let views: *mut objc2::runtime::AnyObject = msg_send![parent, subviews];
        let count: usize = msg_send![views, count];
        (0..count)
            .map(|index| {
                let view: *mut objc2::runtime::AnyObject = msg_send![views, objectAtIndex: index];
                view as usize
            })
            .collect()
    }
}

#[test]
fn views_put_in_a_new_order_never_leave_their_parent_and_so_never_their_window() {
    use objc2::msg_send;
    let (root, other) = (tracked_view(), tracked_view());
    let (a, b, c) = (tracked_view(), tracked_view(), tracked_view());
    let id = CompositionSurfaceId::default();
    // SAFETY: live views, made above.
    unsafe {
        for view in [other, a, b, c] {
            let _: () = msg_send![root, addSubview: view];
        }
    }
    LEFT_PARENT.store(0, std::sync::atomic::Ordering::SeqCst);

    let order = [(id, c as usize, root as usize), (id, a as usize, root as usize), (id, b as usize, root as usize)];
    // SAFETY: live views, made above.
    unsafe { mount_in_order(&order) };
    assert_eq!(subviews(root), [other, c, a, b].map(|view| view as usize));

    let moved = [(id, c as usize, root as usize), (id, a as usize, other as usize), (id, b as usize, root as usize)];
    // SAFETY: live views, made above.
    unsafe { mount_in_order(&moved) };
    assert_eq!(subviews(root), [other, c, b].map(|view| view as usize));
    assert_eq!(subviews(other), [a as usize]);

    assert_eq!(LEFT_PARENT.load(std::sync::atomic::Ordering::SeqCst), 0, "a view was taken out of its parent");
}

// Koyo: a window that composes a native view, as display links pace it: the window's own
// renderer and one GPUI surface over the view, driven through the functions the window runs
// (`start_display_link`, `stop_display_link`, `metal_frame`, `display_layer`, and the update
// of a layer's link). The links are the test's (`TestLink`); the drawables a link would hand
// out are asked of the layers, which a test can do as no window shows them.
struct ComposedWindow {
    base: MetalRenderer,
    composition: MacComposition,
    frames: MetalFrames<TestLink>,
    window_link: Option<Rc<TestLinkState>>,
    layer_links: RefCell<Vec<Rc<TestLinkState>>>,
}

const SIDE: i32 = 64;

impl ComposedWindow {
    fn new() -> Self {
        let mut base = MetalRenderer::new(Default::default(), false, false).unwrap();
        base.update_drawable_size(size(DevicePixels(SIDE), DevicePixels(SIDE)));
        Self {
            base,
            composition: MacComposition::new(Default::default()),
            frames: MetalFrames::default(),
            window_link: None,
            layer_links: RefCell::new(Vec::new()),
        }
    }

    // `start_display_link`: the window's own link made, and its layers'.
    fn start(&mut self) {
        let (link, seen) = TestLink::new();
        self.window_link = Some(seen);
        let make_link = link_maker(&self.layer_links);
        start_links(&mut self.base, &mut self.composition, &self.frames, link, make_link);
    }

    // `stop_display_link`.
    fn stop(&mut self) {
        stop_links(&mut self.base, &mut self.composition, &self.frames);
    }

    // `set_composition_order`: a GPUI surface made with the renderer `create_gpui_surface`
    // makes joins the window, and its layer gets a link while the window's links run.
    fn join(&mut self) -> CompositionSurfaceId {
        let id = CompositionSurfaceId::default();
        let (renderer, metal_layer) = surface_renderer(
            Default::default(),
            &self.base,
            size(DevicePixels(SIDE), DevicePixels(SIDE)),
            1.,
        )
        .unwrap();
        let surface = MacGpuiSurface {
            view: NonNull::new(plain_view()).unwrap(),
            renderer,
            metal_layer,
            layer: Default::default(),
            input_regions: Default::default(),
        };
        self.composition.gpui_surfaces.insert(id, surface);
        let make_link = link_maker(&self.layer_links);
        let surface = self.composition.gpui_surfaces.get_mut(&id).unwrap();
        link_layer(Some(&self.frames), id, surface, &make_link);
        id
    }

    fn surface(&self) -> &MacGpuiSurface {
        self.composition.gpui_surfaces.values().next().unwrap()
    }

    fn surface_mut(&mut self) -> &mut MacGpuiSurface {
        self.composition.gpui_surfaces.values_mut().next().unwrap()
    }

    // `layer_frame`: the surface's link hands its layer its next drawable.
    fn hand_surface_a_drawable(&mut self) {
        let drawable = next_drawable(&self.surface().renderer);
        self.composition
            .drawable_handed(CompositionSurfaceId::default(), drawable);
    }

    // `metal_frame`: the window's own link hands its layer a drawable, GPUI draws `scene` on
    // the window's layer and `over` on the surface's (`draw_composed`), and the frame ends.
    fn paced_frame(&mut self, scene: Option<LayerScene<'_>>, over: Option<LayerScene<'_>>) {
        let drawable = next_drawable(&self.base);
        drop(self.base.set_frame_drawable(Some(drawable)));
        self.frames.begin_frame();
        self.draw(scene, over);
        end_frame(&mut self.base, &mut self.composition, &self.frames);
    }

    fn draw(&mut self, scene: Option<LayerScene<'_>>, over: Option<LayerScene<'_>>) {
        if let Some(scene) = scene {
            draw_layer(&mut self.base, &mut self.composition.base_layer, scene);
        }
        if let Some(over) = over {
            let surface = self.surface_mut();
            draw_layer(&mut surface.renderer, &mut surface.layer, over);
        }
    }

    // `display_layer`: `AppKit` asks the window to draw, while a person resizes it or not; a
    // frame drawn at once comes of it only when the window says so (`draw_in_transaction`).
    fn appkit_asks(&self, live_resize: bool) -> bool {
        draws_in_transaction(Some(&self.frames), live_resize)
    }

    // `draw_in_transaction`: a frame drawn at once, in Core Animation's transaction, as a live
    // resize has it and the app (`FrameAtOnce::draw`).
    fn frame_at_once(&mut self, scene: LayerScene<'_>, over: LayerScene<'_>, during: impl FnOnce(&mut Self)) {
        self.stop();
        set_presents_with_transaction(&mut self.base, &mut self.composition, true);
        self.draw(Some(scene), Some(over));
        during(self);
        set_presents_with_transaction(&mut self.base, &mut self.composition, false);
        self.start();
    }

    fn resize(&mut self, side: i32) {
        let drawable_size = size(DevicePixels(side), DevicePixels(side));
        self.base.update_drawable_size(drawable_size);
        self.surface_mut().renderer.update_drawable_size(drawable_size);
    }

    fn surface_link(&self) -> Rc<TestLinkState> {
        self.layer_links.borrow().last().unwrap().clone()
    }

    fn links_paused(&self) -> [bool; 2] {
        let (window, surface) = (self.window_link.clone().unwrap(), self.surface_link());
        assert!(window.is_live() && surface.is_live(), "the window holds both links");
        [window.is_paused(), surface.is_paused()]
    }

    // What the surface's layer shows: the side of the drawable last drawn for it.
    fn surface_shows(&self) -> Option<i32> {
        self.surface().layer.shown.as_ref().map(|shown| shown.size.width.0)
    }

    fn surface_drawable_unused(&mut self) -> bool {
        self.surface_mut().renderer.set_frame_drawable(None).is_some()
    }
}

// The links `start_links` and `link_layer` make, which `links` keeps the state of.
fn link_maker(
    links: &RefCell<Vec<Rc<TestLinkState>>>,
) -> impl Fn(CompositionSurfaceId, &CAMetalLayer) -> Option<TestLink> + '_ {
    |_, _| {
        let (link, seen) = TestLink::new();
        links.borrow_mut().push(seen);
        Some(link)
    }
}

fn plain_view() -> *mut objc2::runtime::AnyObject {
    use objc2::{class, msg_send};
    // SAFETY: `NSView`'s initializer.
    unsafe {
        let view: *mut objc2::runtime::AnyObject = msg_send![class!(NSView), alloc];
        msg_send![view, initWithFrame: objc2_foundation::NSRect::ZERO]
    }
}

fn next_drawable(renderer: &MetalRenderer) -> Retained<ProtocolObject<dyn CAMetalDrawable>> {
    renderer.layer().and_then(|layer| layer.nextDrawable()).unwrap()
}

fn square(side: f32) -> LayerScene<'static> {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(side), ScaledPixels(side)),
    );
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        ..Default::default()
    });
    scene.finish();
    LayerScene::Replayed(scene)
}

// Koyo: no paced frame asks a layer over a page for a drawable, which waits on the main thread
// while Core Animation holds the layer's: the layer's own link hands it one, even when the
// layer joins a window whose links idle.
#[test]
fn a_layer_joining_an_idle_window_idles_too_and_draws_only_what_its_link_hands_it() {
    let mut window = ComposedWindow::new();
    window.start();
    window.paced_frame(None, None);
    assert_eq!(window.window_link.as_ref().map(|link| link.is_paused()), Some(true));

    window.join();
    assert!(window.surface_link().is_paused(), "a layer joining an idle window idles too");
    window.frames.wake();
    assert_eq!(window.links_paused(), [false; 2], "and wakes with it");

    window.paced_frame(None, Some(square(16.)));
    assert_eq!(window.surface_shows(), None, "the layer asked for no drawable");
    assert_eq!(window.links_paused(), [false; 2], "the links run on while it waits");

    window.hand_surface_a_drawable();
    assert_eq!(window.surface_shows(), Some(SIDE), "its link's drawable shows the scene");
    window.hand_surface_a_drawable();
    window.paced_frame(None, None);
    assert_eq!(window.links_paused(), [true; 2], "and with nothing waiting, the links idle");
    assert!(
        !window.surface_drawable_unused(),
        "the layer lets go of the drawable its link handed it and nothing was drawn into"
    );
}

// Koyo: a layer whose window's links don't run, as while it can't be seen, has no link and
// asks for its drawables itself; it never waits for a link it hasn't got, which would keep
// every link of the window running for good.
#[test]
fn a_layer_with_no_link_draws_into_a_drawable_it_asks_for() {
    let mut window = ComposedWindow::new();
    window.join();
    assert!(window.layer_links.borrow().is_empty(), "a window whose links don't run makes none");
    window.draw(None, Some(square(16.)));
    assert_eq!(window.surface_shows(), Some(SIDE), "the layer draws at once");
    assert!(window.surface().layer.wanted.is_none(), "and waits for nothing");

    window.start();
    assert_eq!(window.links_paused(), [false; 2], "the layer is linked with the window's own");
    window.paced_frame(None, Some(square(32.)));
    assert!(window.surface().layer.wanted.is_some(), "and now waits for its link's drawable");
}

// Koyo: a frame drawn at once presents every layer in Core Animation's transaction, as the
// page's view moves in it, with no link live: each layer asks for a drawable of its size now,
// never presenting one a link handed it before a resize, and the links run again after. A live
// resize has one, and so does the app (`FrameAtOnce`), which asks no question of `AppKit`.
#[test]
fn a_frame_drawn_at_once_presents_every_layer_in_the_transaction_with_no_link_live() {
    let mut window = ComposedWindow::new();
    window.start();
    window.join();
    window.hand_surface_a_drawable();
    window.resize(SIDE / 2);
    assert!(window.appkit_asks(true), "a live resize is drawn at once");
    let (window_link, surface_link) = (window.window_link.clone().unwrap(), window.surface_link());

    window.frame_at_once(square(8.), square(16.), |window| {
        assert!(!window_link.is_live() && !surface_link.is_live(), "no link is live");
        for (renderer, layer) in [
            (&window.base, window.base.layer().unwrap()),
            (&window.surface().renderer, &*window.surface().metal_layer),
        ] {
            assert!(renderer.presents_with_transaction() && layer.presentsWithTransaction());
        }
        assert_eq!(
            window.composition.base_layer.shown.as_ref().map(|shown| shown.size.width.0),
            Some(SIDE / 2),
            "the window's layer is drawn at its size"
        );
        assert_eq!(window.surface_shows(), Some(SIDE / 2), "and the surface's at its own");
    });

    assert_eq!(window.links_paused(), [false; 2], "every link runs again after");
    for renderer in [&window.base, &window.surface().renderer] {
        assert!(renderer.is_display_link_attached() && !renderer.presents_with_transaction());
    }
}

// Koyo: `AppKit` asking the window to draw with no person resizing it, as when macOS fits the
// window to a display that just came back, while Core Animation holds the layers' drawables,
// draws nothing then: no link stops, so no layer asks Core Animation for a drawable on the main
// thread, and the links wake, so their next update draws the window at its new size.
#[test]
fn appkit_asking_outside_a_live_resize_wakes_the_links_and_draws_at_their_next_update() {
    let mut window = ComposedWindow::new();
    window.start();
    window.join();
    window.paced_frame(None, None);
    assert_eq!(window.links_paused(), [true; 2], "an idle window's links pause");
    window.resize(SIDE / 2);

    assert!(!window.appkit_asks(false), "nothing is drawn at once");
    assert_eq!(window.links_paused(), [false; 2], "every link wakes, and none stopped");
    for renderer in [&window.base, &window.surface().renderer] {
        assert!(renderer.is_display_link_attached() && !renderer.presents_with_transaction());
    }
    assert!(
        window.composition.base_layer.shown.is_none() && window.surface_shows().is_none(),
        "and no layer was drawn"
    );

    window.paced_frame(Some(square(8.)), Some(square(16.)));
    assert_eq!(
        window.composition.base_layer.shown.as_ref().map(|shown| shown.size.width.0),
        Some(SIDE / 2),
        "the next update draws the window's layer at its new size"
    );
    window.hand_surface_a_drawable();
    assert_eq!(window.surface_shows(), Some(SIDE / 2), "and the surface's at its next drawable");
}

// Koyo: a drawable a link handed out before a resize is the old size; the scene drawn into it
// shows stretched, and is drawn again into the next one.
#[test]
fn a_layer_drawn_into_a_drawable_from_before_a_resize_draws_again_at_the_next() {
    let mut window = ComposedWindow::new();
    window.start();
    window.join();
    window.hand_surface_a_drawable();
    window.resize(SIDE / 2);
    window.paced_frame(None, Some(square(16.)));
    assert_eq!(window.surface_shows(), None, "drawn at the old size, the layer still wants it");
    assert_eq!(window.links_paused(), [false; 2]);

    window.hand_surface_a_drawable();
    assert_eq!(window.surface_shows(), Some(SIDE / 2));
}

#[test]
fn a_layer_without_a_drawable_shows_the_latest_scene_at_its_next_drawable() {
    let mut window = ComposedWindow::new();
    window.start();
    window.join();
    window.paced_frame(None, Some(square(16.)));
    window.paced_frame(None, Some(square(32.)));

    window.hand_surface_a_drawable();
    assert!(!window.surface_drawable_unused(), "the layer's next drawable shows what it waited to");

    window.hand_surface_a_drawable();
    window.draw(None, Some(square(32.)));
    assert!(window.surface_drawable_unused(), "the layer shows the latest scene, not drawn again");

    window.hand_surface_a_drawable();
    window.paced_frame(None, Some(square(16.)));
    assert!(!window.surface_drawable_unused(), "the earlier scene is not what the layer shows");
}

// Koyo: one layer with no drawable doesn't hold up the frame, as one Core Animation kept the
// drawables of held up every frame upstream, waiting in `nextDrawable` on the main thread.
#[test]
fn a_layer_without_a_drawable_is_left_out_of_the_frame_and_the_window_drawn() {
    let mut window = ComposedWindow::new();
    window.start();
    window.join();
    window.paced_frame(Some(square(8.)), Some(square(16.)));
    assert_eq!(
        window.composition.base_layer.shown.as_ref().map(|shown| shown.size.width.0),
        Some(SIDE),
        "the window's layer, which has its drawable, is drawn"
    );
    assert_eq!(window.surface_shows(), None);

    window.hand_surface_a_drawable();
    assert_eq!(window.surface_shows(), Some(SIDE), "the other at its own next drawable");
}
