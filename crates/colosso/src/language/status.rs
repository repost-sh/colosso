//! The selected checkout's language servers, in the status bar along the window's bottom:
//! each with what it's doing, and a menu to restart or stop it, turn it off or on, and show
//! its log. A view of its own, so a server reporting progress redraws only the servers.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, Bounds, Context, Entity, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};

use super::Languages;
use super::registry::with_registry;
use super::server::{Server, State};
use super::Kind;
use crate::modal::spinner;
use crate::shell::icon;
use crate::settings::{Scope, SetLanguageServer};
use crate::theme::{self, Palette};

/// Starts the server of a kind in the checkout at a root again, whatever it's doing.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = language, no_json)]
pub struct RestartServer(pub Kind, pub PathBuf);

/// Stops the server of a kind in the checkout at a root, until it's started again.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = language, no_json)]
pub struct StopServer(pub Kind, pub PathBuf);

/// Opens the log of the server of a kind in the checkout at a root.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = language, no_json)]
pub struct ShowServerLog(pub Kind, pub PathBuf);

/// What the menus' items do, from any window.
pub fn register(cx: &mut App) {
    cx.on_action(|RestartServer(kind, root): &RestartServer, cx| {
        with_registry(cx, |registry, cx| registry.restart(*kind, root, cx));
    });
    cx.on_action(|StopServer(kind, root): &StopServer, cx| {
        with_registry(cx, |registry, cx| registry.stop(*kind, root, cx));
    });
    cx.on_action(|ShowServerLog(kind, root): &ShowServerLog, cx| {
        with_registry(cx, |registry, cx| {
            let server = registry.server(*kind, root);
            let log = server.and_then(|server| server.read(cx).log().path().map(Path::to_path_buf));
            match log {
                // Console follows the file as the server adds to it.
                Some(log) if log.exists() => cx.open_with_system(&log),
                _ => log::warn!("{} has no log for {} yet", kind.name(), root.display()),
            }
        });
    });
}

/// The selected checkout's servers, as the status bar shows them.
pub struct ServersStatus {
    /// The selected checkout.
    root: Option<PathBuf>,
    servers: Vec<Entity<Server>>,
    /// The registry installed, as installing one replaces any there was.
    _installed: Subscription,
    _registry: Option<Subscription>,
    _servers: Vec<Subscription>,
}

impl ServersStatus {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut status = ServersStatus {
            root: None,
            servers: Vec::new(),
            _installed: cx.observe_global::<Languages>(Self::installed),
            _registry: None,
            _servers: Vec::new(),
        };
        status.installed(cx);
        status
    }

    /// Follows the registry installed now.
    fn installed(&mut self, cx: &mut Context<Self>) {
        let registry = cx.try_global::<Languages>().cloned();
        self._registry = registry.map(|Languages(registry)| {
            cx.observe(&registry, |this: &mut Self, _, cx| this.list(cx))
        });
        self.list(cx);
    }

    /// Shows the servers of the checkout at `root`, the selected one.
    pub fn show(&mut self, root: Option<&Path>, cx: &mut Context<Self>) {
        if self.root.as_deref() != root {
            self.root = root.map(Path::to_path_buf);
            self.list(cx);
        }
    }

    /// Lists the selected checkout's servers again, and follows each.
    fn list(&mut self, cx: &mut Context<Self>) {
        let registry = cx.try_global::<Languages>().cloned();
        let servers = match (registry, &self.root) {
            (Some(Languages(registry)), Some(root)) => registry.read(cx).servers_at(root),
            _ => Vec::new(),
        };
        if servers != self.servers {
            self._servers = (servers.iter())
                .map(|server| cx.observe(server, |_, _, cx| cx.notify()))
                .collect();
            self.servers = servers;
        }
        cx.notify();
    }
}

impl Render for ServersStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let registry = cx.try_global::<Languages>().cloned();
        let items: Vec<AnyElement> = (self.servers.iter().enumerate())
            .map(|(index, server)| {
                let server = server.read(cx);
                let off = (registry.as_ref())
                    .is_some_and(|Languages(registry)| {
                        registry.read(cx).is_off(server.kind(), server.root())
                    });
                let project = (registry.as_ref()).and_then(|Languages(registry)| {
                    registry.read(cx).project_of(server.root()).map(Path::to_path_buf)
                });
                item(index, Shown::of(server, off), project, p)
            })
            .collect();
        div().flex().items_center().gap_1().children(items)
    }
}

/// What the bar shows of a server.
struct Shown {
    kind: Kind,
    root: PathBuf,
    state: State,
    off: bool,
    restarting: bool,
    health: Option<SharedString>,
}

impl Shown {
    fn of(server: &Server, off: bool) -> Shown {
        Shown {
            kind: server.kind(),
            root: server.root().to_owned(),
            state: server.state().clone(),
            off,
            restarting: server.is_restarting(),
            health: server.health().cloned(),
        }
    }

    /// Whether it's running, or about to be.
    fn is_live(&self) -> bool {
        self.is_busy() || matches!(self.state, State::Ready)
    }

    fn is_busy(&self) -> bool {
        self.restarting || matches!(self.state, State::Starting | State::Indexing { .. })
    }

    /// The few words beside its name, if any: none once it's ready and well.
    fn words(&self) -> Option<SharedString> {
        if self.off {
            return Some("off".into());
        }
        let words: SharedString = match &self.state {
            State::Stopped => "stopped".into(),
            State::Starting => "starting".into(),
            State::Indexing { done, total, title } if *total > 0 => {
                format!("{title} {done}/{total}").into()
            }
            State::Indexing { title, .. } => format!("{title}…").into(),
            State::Ready => return self.health.clone(),
            State::Failed(failure) => failure.words.unwrap_or("couldn't start").into(),
            State::Crashed(_) if self.restarting => "restarting".into(),
            State::Crashed(_) => "crashed".into(),
        };
        Some(words)
    }

    /// The whole story, for its tooltip.
    fn told(&self) -> SharedString {
        let name = self.kind.name();
        let told = if self.off {
            format!("{name} is turned off for this project")
        } else {
            match &self.state {
                State::Stopped => format!("{name} is stopped"),
                State::Starting => format!("{name} is starting"),
                State::Indexing { title, .. } => format!("{name}: {title}"),
                State::Ready => match &self.health {
                    Some(health) => format!("{name}: {health}"),
                    None => format!("{name} is ready"),
                },
                State::Failed(failure) => format!("{name} couldn't start: {}", failure.why),
                State::Crashed(why) => format!("{name} crashed: {why}"),
            }
        };
        told.into()
    }

    /// The icon before the language's name, and its own colour where it isn't the text's:
    /// braces, as VS Code has them, while it's well; what's wrong otherwise.
    fn icon(&self, p: Palette) -> (IconName, Option<Hsla>) {
        if self.off {
            return (IconName::CircleOff, None);
        }
        match &self.state {
            State::Ready if self.health.is_some() => (IconName::TriangleAlert, Some(p.modified)),
            State::Failed(_) | State::Crashed(_) if !self.restarting => {
                (IconName::CircleX, Some(p.danger))
            }
            // Busy, which a spinner shows instead.
            State::Starting | State::Indexing { .. } | State::Crashed(_) => {
                (IconName::Braces, Some(p.modified))
            }
            State::Ready | State::Stopped | State::Failed(_) => (IconName::Braces, None),
        }
    }

    /// The menu its item opens, the project the checkout is in, by its main checkout, offered
    /// to turn it off or on in.
    fn menu(&self, project: Option<PathBuf>) -> NativeMenu {
        let (kind, root, name) = (self.kind, self.root.clone(), self.kind.name());
        let restart = Box::new(RestartServer(kind, root.clone()));
        let mut menu = NativeMenu::new();
        if !self.off {
            menu = if self.is_live() {
                menu.menu(format!("Restart {name}"), restart)
                    .menu("Stop", Box::new(StopServer(kind, root.clone())))
            } else {
                menu.menu(format!("Start {name}"), restart)
            };
            menu = menu.separator();
        }
        let set = |scope: Scope, on: bool| -> Box<dyn Action> {
            Box::new(SetLanguageServer { name, scope, on })
        };
        menu = match (self.off, project) {
            (false, Some(project)) => menu
                .menu("Turn Off for This Project", set(Scope::Project(project), false))
                .menu("Turn Off Everywhere", set(Scope::Everywhere, false)),
            (false, None) => menu.menu("Turn Off Everywhere", set(Scope::Everywhere, false)),
            (true, Some(project)) => menu.menu("Turn On", set(Scope::Project(project), true)),
            (true, None) => menu.menu("Turn On", set(Scope::Everywhere, true)),
        };
        menu.separator()
            .menu("Show Log", Box::new(ShowServerLog(kind, root)))
    }
}

/// A server's item: an icon for how it's doing, its language and a few words; a click opens
/// its menu.
fn item(index: usize, shown: Shown, project: Option<PathBuf>, p: Palette) -> AnyElement {
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let measured = bounds.clone();
    let told = shown.told();
    let (language, words) = (shown.kind.language(), shown.words());
    let (glyph, colour) = shown.icon(p);
    let mark = if shown.is_busy() {
        div().relative().size(px(13.)).child(spinner(colour.unwrap_or(p.muted)))
    } else {
        div().child(icon(glyph, 13.).when_some(colour, Styled::text_color))
    };
    div()
        .id(("language-server", index))
        .h_full()
        .px_1p5()
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|item| item.bg(p.hover).text_color(p.text))
        .on_prepaint(move |bounds, _, _| measured.set(bounds))
        .tooltip(move |window, cx| Tooltip::new(told.clone()).build(window, cx))
        .on_click(move |_, window, cx| {
            shown.menu(project.clone()).show(bounds.get().origin, window, cx);
        })
        .child(mark.flex_none())
        .child(language)
        .when_some(words, |item, words| {
            item.child(div().text_color(p.muted).truncate().child(words))
        })
        .into_any_element()
}
