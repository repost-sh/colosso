//! Where a control sends its action: an object of our own class, which says so on a channel.

use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSControl;

define_class!(
    /// Hears a control's action, and says it was sent.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ColossoNativeTarget"]
    #[ivars = UnboundedSender<()>]
    pub struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(acted:))]
        fn acted(&self, _sender: &NSObject) {
            // Unheard only once the page holding the control has gone.
            self.ivars().unbounded_send(()).ok();
        }
    }
);

impl Target {
    /// The target of `control`'s action, telling `tell` each time it's sent. The control
    /// must be let go of with [`Target::detach`] before the target is: `AppKit` doesn't keep
    /// a control's target.
    pub fn attach(
        main: MainThreadMarker,
        control: &NSControl,
        tell: UnboundedSender<()>,
    ) -> Retained<Target> {
        let this = Target::alloc(main).set_ivars(tell);
        // SAFETY: `NSObject`'s `init`, on an instance just allocated with its ivars set.
        let target: Retained<Target> = unsafe { msg_send![super(this), init] };
        // SAFETY: the caller keeps the target as long as the control sends to it, and
        // detaches it before letting it go; `acted:` is the action `Target` defines.
        unsafe {
            control.setTarget(Some(&target));
            control.setAction(Some(sel!(acted:)));
        }
        target
    }

    /// Stops `control` sending to its target.
    pub fn detach(control: &NSControl) {
        // SAFETY: a nil target only stops the control sending.
        unsafe { control.setTarget(None) };
    }
}
