//! What the activity panel draws: its tabs, each tab's tools, and its rows, after
//! agent-terminal's dashboard (`dashboard/src/components/activity-panel.tsx` and its
//! siblings): a row a line, expanded by a click; Follow, Jump to latest and Clear on the
//! activity; "Failed only" on the commands.

use koyo_term_control::{ActivityEntry, ActorKind, CommandRecord, ScreenshotEntry};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, ScrollEvent, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, img, px,
};

use super::super::header::words;
use super::{ActivityPanel, PanelTab};
use crate::theme::{self, Palette};

impl Render for ActivityPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let rows = match self.tab {
            PanelTab::Activity => self.activity_rows(p, cx),
            PanelTab::Commands => self.command_rows(p, cx),
            PanelTab::Screenshots => self.screenshot_rows(p),
            PanelTab::Output => self.output_rows(p),
            PanelTab::Info => self.info_rows(p),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_xs()
            .text_color(p.text)
            .child(self.tabs(p, cx))
            .child(self.tools(p, cx))
            .child(
                div()
                    .id("terminal-panel-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        // Scrolling back through the rows stops following the newest.
                        if event.delta.pixel_delta(px(16.)).y > px(0.) && this.following {
                            this.following = false;
                            cx.notify();
                        }
                    }))
                    .children(rows),
            )
    }
}

impl ActivityPanel {
    fn tabs(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(28.))
            .flex_shrink_0()
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(p.hairline)
            .children(PanelTab::ALL.into_iter().map(|tab| {
                let on = tab == self.tab;
                tool(("terminal-panel-tab", tab as usize), tab.label(), on, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.show(tab, cx)))
            }))
    }

    fn tools(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let bar = div()
            .h(px(26.))
            .flex_shrink_0()
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .text_color(p.muted)
            .border_b_1()
            .border_color(p.hairline);
        match self.tab {
            PanelTab::Activity => bar
                .map(|bar| {
                    if self.following {
                        bar.child(div().px_1p5().child("Following"))
                    } else {
                        bar.child(
                            tool("terminal-panel-latest", "Jump to latest", false, p).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.following = true;
                                    this.scroll.scroll_to_bottom();
                                    cx.notify();
                                }),
                            ),
                        )
                    }
                })
                .child(div().flex_1())
                .child(
                    tool("terminal-panel-clear", "Clear", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.cleared = this.activity.end();
                            this.expanded.clear();
                            cx.notify();
                        },
                    )),
                ),
            PanelTab::Commands => bar.child(div().flex_1()).child(
                tool("terminal-panel-failed", "Failed only", self.failed_only, p).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.failed_only = !this.failed_only;
                        cx.notify();
                    }),
                ),
            ),
            PanelTab::Output => {
                let label = if self.output.mark.is_some() {
                    "Mark again"
                } else {
                    "Set mark"
                };
                bar.child(div().px_1p5().child("The output from the mark on"))
                    .child(div().flex_1())
                    .child(
                        tool("terminal-panel-mark", label, false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.set_mark(cx))),
                    )
            }
            PanelTab::Screenshots | PanelTab::Info => bar.h(px(0.)).border_b_0(),
        }
    }

    fn activity_rows(&self, p: Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let shown: Vec<_> = self.activity.numbered(self.cleared).collect();
        if shown.is_empty() {
            let said = if self.cleared > 0 {
                "Activity cleared"
            } else {
                "Nothing yet"
            };
            return vec![empty(said, p)];
        }
        (shown.into_iter())
            .map(|(index, entry)| {
                let open = self.expanded.contains(&(PanelTab::Activity, index));
                activity_row(entry, index, open, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle(PanelTab::Activity, index, cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn command_rows(&self, p: Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let rows: Vec<AnyElement> = self
            .commands
            .numbered(0)
            .filter(|(_, record)| !self.failed_only || record.exit_code.is_some_and(|c| c != 0))
            .map(|(index, record)| {
                let open = self.expanded.contains(&(PanelTab::Commands, index));
                command_row(record, index, open, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle(PanelTab::Commands, index, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            let said = if self.failed_only {
                "No command failed"
            } else {
                "No commands yet: the shell reports them through its integration"
            };
            return vec![empty(said, p)];
        }
        rows
    }

    fn screenshot_rows(&self, p: Palette) -> Vec<AnyElement> {
        if self.screenshots.rows().is_empty() {
            return vec![empty("No screenshots yet", p)];
        }
        let folder = self.client.dir().terminal(&self.handle);
        (self.screenshots.rows().iter().rev())
            .map(|shot| screenshot_row(shot, folder.join(&shot.path), p).into_any_element())
            .collect()
    }

    fn output_rows(&self, p: Palette) -> Vec<AnyElement> {
        if self.output.mark.is_none() {
            return vec![empty("Set a mark to follow the output from there", p)];
        }
        vec![
            mono(self.output.text.clone(), p)
                .px_2()
                .py_1()
                .into_any_element(),
        ]
    }

    fn info_rows(&self, p: Palette) -> Vec<AnyElement> {
        let Some(record) = &self.info else {
            return vec![empty("Reading…", p)];
        };
        let holder = (record.control.holder.as_ref()).map_or_else(
            || "no one".to_owned(),
            |holder| holder.actor().label.clone(),
        );
        let command = if record.command.is_empty() {
            "the login shell".to_owned()
        } else {
            record.command.join(" ")
        };
        let pairs = [
            ("Handle", record.handle.to_string()),
            ("Name", record.name.clone()),
            (
                "Worktree",
                (record.worktree.as_ref())
                    .map_or_else(String::new, |w| w.path.display().to_string()),
            ),
            ("Folder", record.cwd.display().to_string()),
            ("Command", command),
            (
                "Process",
                record
                    .pid
                    .map_or_else(|| "none".to_owned(), |pid| pid.to_string()),
            ),
            (
                "Size",
                format!("{} × {}", record.size.cols, record.size.rows),
            ),
            (
                "Status",
                format!("{} · {}", words(record.status), record.status_reason),
            ),
            (
                "Last exit",
                record
                    .last_exit
                    .map_or_else(String::new, |code| code.to_string()),
            ),
            ("TERM", koyo_term_engine::TERM.to_owned()),
            ("Controller", holder),
            ("Watching", record.control.observers.len().to_string()),
            ("Attached clients", record.attached_clients.to_string()),
            ("Opened by", record.created_by.label.clone()),
        ];
        pairs
            .into_iter()
            .map(|(key, value)| {
                div()
                    .px_2()
                    .py_0p5()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .text_color(p.muted)
                            .child(key),
                    )
                    .child(div().min_w_0().child(value))
                    .into_any_element()
            })
            .collect()
    }

    fn toggle(&mut self, tab: PanelTab, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&(tab, index)) {
            self.expanded.insert((tab, index));
        }
        cx.notify();
    }
}

fn activity_row(entry: &ActivityEntry, index: usize, open: bool, p: Palette) -> Stateful<Div> {
    let prefix = match entry.actor.kind {
        ActorKind::Human => "human:",
        ActorKind::Agent => "agent:",
        ActorKind::System => "koyo:",
    };
    let tone = if entry.exit != 0 {
        p.danger
    } else if entry.control.is_some() {
        p.accent
    } else {
        p.text
    };
    let is_agent = entry.actor.kind == ActorKind::Agent;
    // A refusal's code ends its action, so the words wrap across the row.
    let code = (entry.error)
        .and_then(|error| serde_json::to_value(error).ok())
        .and_then(|code| code.as_str().map(str::to_owned));
    let action = match code {
        Some(code) => format!("{} · {code}", entry.action),
        None => entry.action.clone(),
    };
    row(("terminal-activity", index), p)
        .child(
            line()
                .child(div().text_color(p.muted).child(local_time(&entry.time)))
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("{prefix} {}", entry.actor.label)),
                )
                // The action takes the room the row has, so a long one wraps across it.
                .child(div().flex_1().min_w_0().text_color(tone).child(action))
                .when(entry.duration_ms > 0, |line| {
                    line.child(duration(entry.duration_ms))
                }),
        )
        .when(open, |row| {
            // A person's rows show what they printed, never a command line.
            row.when(is_agent && !entry.command.is_empty(), |row| {
                row.child(mono(format!("$ {}", entry.command), p).text_color(p.muted))
            })
            .when_some(entry.output.clone(), |row, output| {
                row.child(mono(output, p))
            })
        })
}

fn command_row(record: &CommandRecord, index: usize, open: bool, p: Palette) -> Stateful<Div> {
    let (exit, tone) = match record.exit_code {
        Some(0) => ("0".to_owned(), p.added),
        Some(code) => (code.to_string(), p.danger),
        None => ("?".to_owned(), p.muted),
    };
    let by = record
        .by
        .as_ref()
        .map(|by| by.label.clone())
        .unwrap_or_default();
    row(("terminal-command", index), p)
        .child(
            line()
                .child(div().text_color(p.muted).child(format!("#{}", record.id)))
                .child(
                    mono(record.command.clone(), p)
                        .min_w_0()
                        .flex_1()
                        .truncate(),
                )
                .child(div().text_color(p.muted).child(by))
                .child(duration(record.duration_ms))
                .child(div().w(px(24.)).text_color(tone).child(exit)),
        )
        .when(open, |row| {
            let output = if record.output.is_empty() {
                "(no output)".to_owned()
            } else {
                record.output.clone()
            };
            row.child(mono(output, p))
        })
}

fn screenshot_row(shot: &ScreenshotEntry, path: std::path::PathBuf, p: Palette) -> Div {
    div()
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
        .border_b_1()
        .border_color(p.hairline.opacity(0.5))
        .child(
            line()
                .child(div().text_color(p.muted).child(local_time(&shot.time)))
                .child(div().child(shot.by.label.clone()))
                .child(
                    div()
                        .text_color(p.muted)
                        .child(format!("{} × {}", shot.cols, shot.rows)),
                ),
        )
        .child(img(path).max_w(px(240.)).rounded(px(4.)))
}

fn row(id: (&'static str, usize), p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
        .cursor_pointer()
        .border_b_1()
        .border_color(p.hairline.opacity(0.5))
        .hover(|style| style.bg(p.hover))
}

fn line() -> Div {
    div().flex().items_center().gap_2().min_w_0()
}

fn mono(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .font_family(theme::MONO)
        .text_color(p.text)
        .whitespace_normal()
        .child(text.into())
}

fn empty(said: &'static str, p: Palette) -> AnyElement {
    div()
        .p_3()
        .text_color(p.muted)
        .child(said)
        .into_any_element()
}

/// A small text button, `on` when what it switches is.
fn tool(
    id: impl Into<gpui_kit::ElementId>,
    label: &'static str,
    on: bool,
    p: Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_1p5()
        .py_0p5()
        .rounded(px(4.))
        .cursor_pointer()
        .text_color(if on { p.text } else { p.muted })
        .when(on, |tool| tool.bg(p.selected))
        .hover(|style| style.bg(p.hover))
        .child(label)
}

fn duration(ms: u64) -> Div {
    let text = if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.)
    };
    div().flex_shrink_0().child(text)
}

/// An RFC 3339 time as the clock here reads it.
fn local_time(time: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(time)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}
