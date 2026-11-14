//! Device tabs (`docs/devices.md` 8): one view per pane, as a browser's pages are, and one tab
//! per device: choosing a device that's open shows its tab. The tab's title is the device's
//! name, kept in step as a page's title is.

use gpui_kit::{AppContext as _, Context, Entity};

use super::{Place, Shell};
use crate::devices::{Chosen, DeviceEvent, DeviceView};
use crate::model::Pane;

impl Shell {
    /// A device's view (the picker with none), the shell hearing it. Opening a tab starts
    /// nothing: whoever opens it starts the device, the person through their limit's question.
    pub(crate) fn device_view(&mut self, chosen: Option<Chosen>, cx: &mut Context<Self>) -> Entity<DeviceView> {
        let view = cx.new(|cx| match chosen {
            Some(chosen) => DeviceView::of(chosen, cx),
            None => DeviceView::picker(cx),
        });
        cx.subscribe(&view, |this, view, event, cx| this.device_event(&view, event, cx))
            .detach();
        view
    }

    fn device_event(&mut self, view: &Entity<DeviceView>, event: &DeviceEvent, cx: &mut Context<Self>) {
        let DeviceEvent::Titled(title) = event;
        let Some((at, id)) = self.workspace.find_pane(|pane| pane.device_view() == Some(view)) else {
            return;
        };
        // A device chosen or let go: its bar comes or goes with its name.
        let bar = view.read(cx).bar();
        if let Some(pane) = (self.workspace.tab_mut(at))
            .and_then(|tab| tab.panes_mut().into_iter().find(|pane| pane.id == id))
        {
            pane.title = title.clone().into();
            pane.set_bar(bar);
            cx.notify();
        }
    }

    /// A device's pane: the one open for `chosen`, wherever it is, or a new one, the picker
    /// when `chosen` is `None`. Its tab is shown when `shown`; else a new
    /// one comes after the tab that is, out of sight, as a tab an agent opens does.
    pub fn device_pane(&mut self, chosen: Option<Chosen>, shown: bool, cx: &mut Context<Self>) -> Option<u64> {
        self.device_pane_at(chosen, self.place(shown), cx)
    }

    /// [`Self::device_pane`], a new one's tab at `place`.
    pub(crate) fn device_pane_at(&mut self, chosen: Option<Chosen>, place: Place, cx: &mut Context<Self>) -> Option<u64> {
        let open = chosen.as_ref().and_then(|chosen| {
            self.workspace.find_pane(|pane| {
                (pane.device_view())
                    .and_then(|view| view.read(cx).chosen().cloned())
                    .is_some_and(|open| open.udid == chosen.udid)
            })
        });
        let make = |shell: &mut Self, cx: &mut Context<Self>| {
            let view = shell.device_view(chosen, cx);
            Pane::device(view, cx)
        };
        self.one_pane(open, place, make, cx)
    }

    /// The device view of pane `id`, wherever it is.
    pub fn device_view_of(&self, id: u64) -> Option<Entity<DeviceView>> {
        let (at, _) = self.workspace.find_pane(|pane| pane.id == id)?;
        let tab = self.workspace.tab(at)?;
        tab.panes().into_iter().find(|pane| pane.id == id)?.device_view().cloned()
    }

    /// The device view of the pane the shown tab shows, when it's a device's.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn shown_device_view(&self, _: &gpui_kit::App) -> Option<Entity<DeviceView>> {
        self.workspace.tab_shown()?.current().device_view().cloned()
    }
}
