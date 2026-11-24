// Modified by Longbridge for gpui-fast.
use super::{mount_in_order, whole_scene_layer};
use gpui::{
    AppContext as _, Bounds, ComposedSceneLayer, CompositionSurfaceId, ContentMask, Context,
    DevicePixels, IntoElement, ParentElement as _, PlatformHeadlessRenderer as _, Quad, Render,
    ScaledPixels, Scene, Styled as _, TestAppContext, Window, div, point, px, size,
};

use crate::metal_renderer::MetalHeadlessRenderer;

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
