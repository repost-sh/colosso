//! What's attached to the message being written (`docs/chat.md` 5.3): pasted, dropped or chosen,
//! each shown as a tile above what's typed from the moment it's asked for, while it's kept off the
//! main thread. What's sent goes with the message; anything else goes with its tile.

use std::path::PathBuf;

use gpui_kit::component::attachment::{Attachment as Tile, AttachmentMedia, AttachmentStatus};
use gpui_kit::{
    App, AppContext as _, Axis, ClickEvent, ClipboardItem, Context, Div, ExternalPaths,
    InteractiveElement as _, ParentElement as _, PathPromptOptions, Styled as _, Task, Window, div,
    px,
};
use koyo_agent::protocol::model::Attachment;

use super::ChatComposer;
use crate::chat::attachments::{self, Source};
use crate::theme::Palette;

/// How wide and tall a tile of a picture is.
const TILE: f32 = 56.;

/// Something attached, or on its way to being.
pub(super) struct Attached {
    /// Tells its tile from the others.
    id: usize,
    /// What the message will name, once it's kept.
    kept: Option<Attachment>,
    _keeping: Task<()>,
}

#[cfg(test)]
impl Attached {
    pub(super) fn id(&self) -> usize {
        self.id
    }
}

impl ChatComposer {
    /// Whether something's on its way to being attached, which a message waits for.
    pub(super) fn attaching(&self) -> bool {
        self.attached.iter().any(|attached| attached.kept.is_none())
    }

    /// Attaches `sources`, each with a tile at once, kept off the main thread.
    pub(super) fn attach(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        let Some(folder) = self.attachments.clone() else {
            return;
        };
        for source in sources {
            let id = self.next_tile();
            let folder = folder.clone();
            let kept = cx.background_spawn(async move { attachments::keep(source, &folder) });
            let keeping = cx.spawn(async move |this, cx| {
                let kept = kept.await;
                this.update(cx, |this, cx| this.was_kept(id, kept, cx)).ok();
            });
            self.attached.push(Attached {
                id,
                kept: None,
                _keeping: keeping,
            });
        }
        self.error = None;
        cx.notify();
    }

    fn next_tile(&mut self) -> usize {
        self.tiles_made += 1;
        self.tiles_made - 1
    }

    fn was_kept(&mut self, id: usize, kept: Result<Attachment, String>, cx: &mut Context<Self>) {
        match kept {
            Ok(kept) => {
                if let Some(attached) = self.attached.iter_mut().find(|held| held.id == id) {
                    attached.kept = Some(kept);
                }
            }
            Err(why) => {
                log::warn!("{why}");
                self.attached.retain(|held| held.id != id);
                self.error = Some(why.into());
            }
        }
        cx.notify();
    }

    /// `paths`, dropped or chosen, are attached.
    pub(in crate::chat) fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.attach(paths.iter().cloned().map(Source::File).collect(), cx);
    }

    /// Takes the keys' way of pasting: pictures and files are attached rather than typed.
    pub(super) fn takes_paste(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(&ClipboardItem, &mut Window, &mut App) -> bool + 'static {
        let composer = cx.weak_entity();
        move |pasted, _, cx| {
            let sources = attachments::sources_of(pasted);
            !sources.is_empty()
                && composer.update(cx, |composer, cx| composer.attach(sources, cx)).is_ok()
        }
    }

    /// Asks the person which files to attach.
    pub(super) fn choose_files(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                this.update(cx, |this, cx| this.attach_paths(&paths, cx)).ok();
            }
        })
        .detach();
    }

    /// Everything attached, for a message that's going, which it now goes with.
    pub(super) fn take_attached(&mut self) -> Vec<Attachment> {
        std::mem::take(&mut self.attached).into_iter().filter_map(|held| held.kept).collect()
    }

    /// A message didn't go, so what it attached comes back.
    pub(super) fn give_back_attached(&mut self, attachments: Vec<Attachment>) {
        for kept in attachments {
            let id = self.next_tile();
            self.attached.push(Attached {
                id,
                kept: Some(kept),
                _keeping: Task::ready(()),
            });
        }
    }

    /// Takes a tile away, and what it kept with it.
    pub(super) fn detach(&mut self, id: usize, cx: &mut Context<Self>) {
        let (gone, left) = std::mem::take(&mut self.attached).into_iter().partition(|held| held.id == id);
        self.attached = left;
        self.forget(gone, cx);
        cx.notify();
    }

    /// The files of `attached`, which no message will name.
    pub(super) fn forget(&self, attached: Vec<Attached>, cx: &App) {
        let Some(folder) = self.attachments.clone() else {
            return;
        };
        let files: Vec<String> =
            attached.into_iter().filter_map(|held| held.kept.map(|kept| kept.file)).collect();
        if files.is_empty() {
            return;
        }
        cx.background_spawn(async move {
            for file in files {
                if let Err(error) = std::fs::remove_file(folder.join(&file)) {
                    log::warn!("an attachment no message names, {file}: {error}");
                }
            }
        })
        .detach();
    }

    /// The tiles, if anything's attached.
    pub(super) fn tiles(&self, p: Palette, cx: &Context<Self>) -> Option<Div> {
        (!self.attached.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(self.attached.iter().map(|attached| self.tile(attached, p, cx)))
        })
    }

    fn tile(&self, attached: &Attached, p: Palette, cx: &Context<Self>) -> Tile {
        let id = attached.id;
        let keyed = ("chat-attached", id);
        let tile = match &attached.kept {
            Some(kept) => attachments::tile(keyed, kept, self.attachments.as_deref(), TILE, p),
            None => attachments::bare_tile(keyed, p)
                .status(AttachmentStatus::Processing)
                .axis(Axis::Vertical)
                .size(px(TILE))
                .media(AttachmentMedia::new()),
        };
        tile.on_remove(cx.listener(move |this, _: &ClickEvent, _, cx| this.detach(id, cx)))
    }

    /// `surface`, which takes files let go over it. It's the surface rather than what holds it, as
    /// what's on the glass blocks the pointer from what's behind.
    pub(super) fn takes_drops(surface: Div, p: Palette, cx: &Context<Self>) -> Div {
        surface
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(p.selected))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                this.attach_paths(dropped.paths(), cx);
            }))
    }
}
