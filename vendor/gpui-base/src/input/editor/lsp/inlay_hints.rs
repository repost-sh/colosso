//! Inlay hints: labels the language server puts into the text without their being in it,
//! the types and parameter names it infers. Fetched for the rows in view, kept in step
//! with the text as it's edited until the next answer, and laid into the rows as
//! fragments of no bytes.
use std::ops::Range;

use anyhow::Result;
use gpui::{App, Context, SharedString, Task, Window};
use instant::Duration;
use ropey::Rope;
use sum_tree::Bias;

use crate::input::edits::translate;
use crate::input::{EditorMode, InputBaseState, Lsp};

/// A label shown in the text at a byte offset, without being in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inlay {
    pub offset: usize,
    pub label: SharedString,
    /// Which text the label belongs to, and so what happens at its offset. With
    /// [`Bias::Right`] it belongs to the text before it (a type after a name): text put in
    /// at the offset pushes it along, and the caret there is painted before it. With
    /// [`Bias::Left`] it belongs to the text after it (a parameter's name before its
    /// value): it stays put, and the caret is painted after it.
    pub bias: Bias,
}

/// A provider of inlay hints, the editor counterpart of `textDocument/inlayHint`.
///
/// <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_inlayHint>
pub trait InlayHintProvider {
    /// The inlays for the byte `range` of `text`, in any order.
    fn inlay_hints(
        &self,
        text: &Rope,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<Inlay>>>;
}

/// How long typing or scrolling has to pause before the server is asked.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// The inlays of the part of the text last asked about, kept in step with the text.
#[derive(Default)]
pub(crate) struct Inlays {
    /// In ascending order of offset.
    inlays: Vec<Inlay>,
    /// The bytes the inlays answer for, in the text as it is now; `None` while they're
    /// out of date, the text having changed or the host having asked again, and before
    /// the first answer.
    covering: Option<Range<usize>>,
    /// The range a fetch is scheduled or under way for, while one is.
    pending: Option<Range<usize>>,
}

impl Inlays {
    /// The inlays at bytes `range`, both ends included.
    pub(crate) fn within(&self, range: &Range<usize>) -> &[Inlay] {
        let first = self.inlays.partition_point(|inlay| inlay.offset < range.start);
        let end = self.inlays.partition_point(|inlay| inlay.offset <= range.end);
        &self.inlays[first..end]
    }

    /// The bytes `old` of the text were replaced by `new_len` bytes: the inlays move with
    /// the edit, each by its bias, until the server answers for the new text; one inside
    /// the replaced bytes is about text that is gone, and goes with it.
    pub(crate) fn edited(&mut self, old: &Range<usize>, new_len: usize) {
        self.inlays.retain_mut(|inlay| {
            if old.start < inlay.offset && inlay.offset < old.end {
                return false;
            }
            inlay.offset = translate(inlay.offset, old, new_len, inlay.bias);
            true
        });
        // Two at one offset part when only one moves along.
        self.inlays.sort_by_key(|inlay| inlay.offset);
    }

    /// Takes the server's answer for `range`; whether it changed what's shown.
    fn replace(&mut self, range: Range<usize>, mut inlays: Vec<Inlay>) -> bool {
        inlays.sort_by_key(|inlay| inlay.offset);
        self.covering = Some(range);
        let changed = inlays != self.inlays;
        self.inlays = inlays;
        changed
    }
}

impl Lsp {
    /// Every inlay cached, in ascending order of offset.
    pub fn inlays(&self) -> &[Inlay] {
        &self.inlays.inlays
    }

    /// Asks the provider again at the next frame: the server said the hints changed, or
    /// the provider was put in or taken out. Notify the editor so a frame comes.
    pub fn refresh_inlay_hints(&mut self) {
        if self.inlay_hint_provider.is_none() {
            self.inlays = Inlays::default();
        }
        self.outdate_inlays();
    }

    /// What's cached is out of date: a fetch under way answers for an older text, or an
    /// older server state, and is let go of; the next frame asks again once things have
    /// settled.
    pub(crate) fn outdate_inlays(&mut self) {
        self.inlays.covering = None;
        self.inlays.pending = None;
        self._inlay_hints_task = Task::ready(());
    }

    /// The rows at bytes `viewport` are in view: the provider is asked, once things have
    /// settled, when what's cached doesn't cover them, for them and a viewport's worth of
    /// text each side.
    pub(crate) fn viewport_shown(
        &mut self,
        viewport: Range<usize>,
        window: &mut Window,
        cx: &mut Context<InputBaseState<EditorMode>>,
    ) {
        let Some(provider) = &self.inlay_hint_provider else {
            return;
        };
        let covered = (self.inlays.covering.as_ref())
            .is_some_and(|covering| covering.start <= viewport.start && viewport.end <= covering.end);
        if covered {
            return;
        }
        let margin = viewport.len();
        let range = viewport.start.saturating_sub(margin)..viewport.end + margin;
        if self.inlays.pending.as_ref() == Some(&range) {
            return;
        }
        self.inlays.pending = Some(range.clone());
        let provider = provider.clone();
        self._inlay_hints_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            // The text as it is now, which may have grown or shrunk since the frame. An
            // edit from here on outdates the cache, and this task with it.
            let asked = cx.update(|window, cx| {
                let state = this.upgrade()?;
                let text = state.read(cx).text().clone();
                let range = range.start.min(text.len())..range.end.min(text.len());
                Some((provider.inlay_hints(&text, range.clone(), window, cx), range))
            });
            let Ok(Some((task, range))) = asked else {
                return;
            };
            let answer = task.await;
            let _ = this.update(cx, |state, cx| {
                let inlays = &mut state.extras.lsp.inlays;
                inlays.pending = None;
                match answer {
                    Ok(found) => {
                        if inlays.replace(range, found) {
                            cx.notify();
                        }
                    }
                    // Not asked again until the text changes, rather than at every frame;
                    // the provider is the one to say what went wrong.
                    Err(_) => inlays.covering = Some(range),
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inlay(offset: usize, bias: Bias) -> Inlay {
        Inlay {
            offset,
            label: "x".into(),
            bias,
        }
    }

    #[test]
    fn inlays_follow_an_edit_by_their_bias_and_go_with_replaced_text() {
        let mut inlays = Inlays::default();
        let changed = inlays.replace(
            0..20,
            vec![
                inlay(12, Bias::Left),
                inlay(4, Bias::Right),
                inlay(4, Bias::Left),
                inlay(8, Bias::Right),
            ],
        );
        assert!(changed);
        assert_eq!(
            inlays.inlays.iter().map(|i| i.offset).collect::<Vec<_>>(),
            [4, 4, 8, 12],
            "kept in order"
        );
        // Two bytes typed at 4: the type hint moves along, the parameter hint stays.
        inlays.edited(&(4..4), 2);
        let offsets = |inlays: &Inlays| {
            inlays
                .inlays
                .iter()
                .map(|i| (i.offset, i.bias))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            offsets(&inlays),
            [
                (4, Bias::Left),
                (6, Bias::Right),
                (10, Bias::Right),
                (14, Bias::Left)
            ]
        );
        // Bytes 9..12 replaced by one: the hint at 10 was inside them and is gone.
        inlays.edited(&(9..12), 1);
        assert_eq!(
            offsets(&inlays),
            [(4, Bias::Left), (6, Bias::Right), (12, Bias::Left)]
        );
        assert_eq!(inlays.within(&(5..6)), [inlay(6, Bias::Right)]);
        assert_eq!(inlays.within(&(0..4)).len(), 1);
        assert!(inlays.within(&(7..11)).is_empty());
        // The same answer again changes nothing to repaint for.
        let same = inlays.inlays.clone();
        assert!(!inlays.replace(0..20, same));
    }
}
