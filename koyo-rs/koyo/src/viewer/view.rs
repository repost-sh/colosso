//! A viewer: the file it shows, what its thread has drawn of it so far, and how it's looked
//! at. A file written again is read again, what was shown staying until the new one's drawn.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context, FocusHandle, Focusable, RenderImage, SharedString, Task};

use super::Kind;
use super::bitmap::release;
use super::canvas::{Camera, fit_zoom};
use super::lane::{Job, Lane, Opened, Out};
use super::picture::Probe;
use super::tiles::{Level, TileKey};
use super::{pdf, picture, svg};

pub struct Viewer {
    pub(super) path: PathBuf,
    pub(super) kind: Kind,
    pub(super) focus: FocusHandle,
    pub(super) state: State,
    /// Each page's size in points, upright: a picture's one, at its density.
    pub(super) pages: Arc<[(f32, f32)]>,
    /// What a picture's header said.
    pub(super) probe: Option<Probe>,
    pub(super) camera: Camera,
    /// The pane's size in logical pixels, and the screen's pixels to one, as last laid out.
    pub(super) viewport: (f32, f32),
    pub(super) scale: f32,
    /// The picture as decoded, and the longest side it was asked for at.
    pub(super) picture: Option<Arc<RenderImage>>,
    pub(super) asked_longest: Option<u32>,
    /// The frame of a moving picture shown, and what moves it on.
    pub(super) frame: usize,
    pub(super) animating: Option<Task<()>>,
    /// The tiles drawn, of every scale still worth showing, and the scale they're drawn at
    /// now: the zoom's, once it's settled.
    pub(super) tiles: HashMap<TileKey, Arc<RenderImage>>,
    pub(super) level: Level,
    /// The tiles asked for at `level` and not yet drawn.
    pub(super) asked: HashSet<TileKey>,
    /// Tiles of the file as it was before it was written again, shown under the new ones
    /// until those are all drawn.
    pub(super) stale: Vec<(TileKey, Arc<RenderImage>)>,
    /// Sets the scale tiles are drawn at once a zoom stops.
    pub(super) settling: Task<()>,
    /// Where a drag that moves the pages started, while it's under way.
    pub(super) drag: Option<(f32, f32)>,
    /// The file's size and when it was written, as it was read.
    pub(super) stamp: Option<(u64, Option<SystemTime>)>,
    lane: Option<Lane>,
    _reading: Task<()>,
    _rereading: Task<()>,
}

/// How far a viewer has got with its file.
pub enum State {
    Opening,
    Open,
    Failed(SharedString),
}

impl Focusable for Viewer {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Viewer {
    /// A viewer of the file at `path`, shown as `kind`, which it starts reading at once.
    pub fn new(path: PathBuf, kind: Kind, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Viewer, cx| this.forget_all(cx))
            .detach();
        let mut viewer = Viewer {
            path,
            kind,
            focus: cx.focus_handle(),
            state: State::Opening,
            pages: Arc::default(),
            probe: None,
            camera: Camera::default(),
            viewport: (0., 0.),
            scale: 1.,
            picture: None,
            asked_longest: None,
            frame: 0,
            animating: None,
            tiles: HashMap::new(),
            level: Level::of(1.),
            asked: HashSet::new(),
            stale: Vec::new(),
            settling: Task::ready(()),
            drag: None,
            stamp: None,
            lane: None,
            _reading: Task::ready(()),
            _rereading: Task::ready(()),
        };
        viewer.read(cx);
        viewer
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether anything of the file is drawn yet: the picture, or a tile of its pages.
    pub fn is_drawn(&self) -> bool {
        self.picture.is_some() || !self.tiles.is_empty()
    }

    /// Starts reading the file afresh, on a thread of its own; the one before, if any, ends.
    fn read(&mut self, cx: &mut Context<Self>) {
        let (sender, mut received) = futures::channel::mpsc::unbounded();
        let path = self.path.clone();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let started = match self.kind {
            Kind::Picture => Lane::start(&name, move || picture::open(&path), sender),
            Kind::Svg => Lane::start(&name, move || svg::open(&path), sender),
            Kind::Pdf => Lane::start(&name, move || pdf::open(&path), sender),
        };
        match started {
            Ok(lane) => self.lane = Some(lane),
            Err(error) => {
                self.state = State::Failed(format!("Couldn't start reading it: {error}").into());
                cx.notify();
                return;
            }
        }
        self.stamp = stamp(&self.path);
        self._reading = cx.spawn(async move |this, cx| {
            while let Some(out) = received.next().await {
                if this.update(cx, |this, cx| this.take(out, cx)).is_err() {
                    return;
                }
            }
        });
    }

    /// The file may have been written: it's read again if its size or time changed.
    pub fn reread(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let was = self.stamp;
        self._rereading = cx.spawn(async move |this, cx| {
            let now = cx.background_spawn(async move { stamp(&path) }).await;
            if now != was {
                this.update(cx, Self::read).ok();
            }
        });
    }

    /// What the thread says.
    fn take(&mut self, out: Out, cx: &mut Context<Self>) {
        match out {
            Out::Opened(opened) => self.opened(opened, cx),
            Out::Failed(why) => {
                log::info!("{} can't be shown: {why}", self.path.display());
                self.state = State::Failed(format!("This file can't be shown: {why}.").into());
            }
            Out::Drawn(Job::Whole(longest), image) => {
                if self.asked_longest != Some(longest) {
                    return;
                }
                if let Some(old) = self.picture.replace(image) {
                    release(vec![old], cx);
                }
                self.frame = 0;
                self.animate(cx);
            }
            Out::Drawn(Job::Tile(key), image) => {
                if !self.asked.remove(&key) {
                    return;
                }
                if let Some(old) = self.tiles.insert(key, image) {
                    release(vec![old], cx);
                }
                self.evict(cx);
            }
            Out::Missed(job) => {
                log::warn!("{}: {job:?} couldn't be drawn", self.path.display());
                // Nothing will come for it: what's in sight is as drawn as it gets.
                if let Job::Tile(key) = job
                    && self.asked.remove(&key)
                {
                    self.evict(cx);
                }
            }
        }
        cx.notify();
    }

    /// The file was opened, or opened again once it was written: what was drawn of it before
    /// stays shown under what's drawn now, and the zoom stays unless it was fitting, or the
    /// pages changed size.
    fn opened(&mut self, opened: Opened, cx: &mut Context<Self>) {
        let pages = match opened {
            Opened::Picture(probe) => {
                let density = probe.density as f32;
                let pages = vec![(
                    probe.pixels.0 as f32 / density,
                    probe.pixels.1 as f32 / density,
                )];
                self.probe = Some(probe);
                self.asked_longest = None;
                pages
            }
            Opened::Pages(pages) => pages,
        };
        let resized = *pages != *self.pages;
        self.pages = pages.into();
        self.state = State::Open;
        self.stale.extend(self.tiles.drain());
        self.asked.clear();
        if resized || self.camera.fit {
            self.camera = Camera {
                fit: true,
                ..self.camera
            };
            self.refit();
        }
        self.level = self.zoom_level();
        self.plan(cx);
    }

    /// Fits the pages to the pane again, when the zoom follows it.
    pub(super) fn refit(&mut self) {
        if !self.camera.fit || self.pages.is_empty() || self.viewport.0 <= 0. {
            return;
        }
        self.camera.zoom = fit_zoom(&self.pages, self.viewport, self.kind.fit());
        let laid = super::canvas::layout(&self.pages, self.camera.zoom);
        self.camera.scroll = super::canvas::clamp(self.camera.scroll, laid.size, self.viewport);
    }

    /// Has the thread draw `jobs`, in place of what it hadn't got to.
    pub(super) fn ask(&self, jobs: impl IntoIterator<Item = Job>) {
        if let Some(lane) = &self.lane {
            lane.want(jobs);
        }
    }

    /// A moving picture moves on each frame's time, while it's shown.
    pub(super) fn animate(&mut self, cx: &mut Context<Self>) {
        let delays: Vec<Duration> = (self.probe.as_ref())
            .map(|probe| probe.delays.clone())
            .unwrap_or_default();
        let frames = self
            .picture
            .as_ref()
            .map_or(0, |picture| picture.frame_count());
        if delays.len() < 2 || frames != delays.len() {
            self.animating = None;
            return;
        }
        if self.animating.is_some() {
            return;
        }
        self.animating = Some(cx.spawn(async move |this, cx| {
            let mut frame = 0;
            loop {
                cx.background_executor().timer(delays[frame]).await;
                frame = (frame + 1) % delays.len();
                let shown = this.update(cx, |this, cx| {
                    this.frame = frame;
                    cx.notify();
                });
                if shown.is_err() {
                    return;
                }
            }
        }));
    }

    /// Takes everything drawn out of the GPU's atlas, as the viewer goes.
    fn forget_all(&mut self, cx: &mut gpui_kit::App) {
        let mut images: Vec<_> = self.tiles.drain().map(|(_, image)| image).collect();
        images.extend(self.stale.drain(..).map(|(_, image)| image));
        images.extend(self.picture.take());
        release(images, cx);
    }
}

/// The file's size and when it was last written, if it can be read.
fn stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
