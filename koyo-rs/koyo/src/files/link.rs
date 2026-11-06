//! Links that name a file, as a chat writes them: `[mod.rs](src/viewer/mod.rs)`, with the line
//! after it as `#L42` or `:42`, relative to the chat's folder or absolute, or a `file://` URL.
//! Clicked, the file opens in a tab, as a click in the tree opens it. What a link names is the
//! terminal's to say, as it says it of the links a program prints. In a chat, such a link is
//! drawn as the tree draws the file: its icon, and its name.

use std::path::PathBuf;

use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    markdown_ast,
};
use gpui_kit::{
    Action, App, ClickEvent, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Window, div,
};
use koyo_term::{LinkTarget, link_target};

use super::{At, file_icon, file_label, file_name};
use crate::theme;

/// What a link to a file is called among the nodes of a markdown document.
const FILE_LINK: &str = "file-link";

/// Opens the file at `path`, absolute, at `at`: what a link to a file asks of the shell.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = files, no_json)]
pub struct OpenLinked {
    pub path: PathBuf,
    pub at: Option<At>,
}

/// What clicking a link in text drawn for a chat, or for a markdown file, in `folder` does: a file it names opens in a
/// tab, a page in the browser. A right click does nothing, as it does to any link.
pub fn opens_links(
    folder: PathBuf,
) -> impl Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync + 'static {
    let folders = [folder];
    move |link, event, window, cx| {
        if event.is_right_click() {
            return;
        }
        match link_target(link, &folders) {
            Some(LinkTarget::File { path, line, .. }) => {
                let at = line.map(At::printed_line);
                window.dispatch_action(OpenLinked { path, at }.boxed_clone(), cx);
            }
            Some(LinkTarget::Url(url)) => cx.open_url(&url),
            // Another scheme's, an email's say, is the system's to open.
            None if link.contains("://") || link.starts_with("mailto:") => cx.open_url(link),
            None => log::info!("{link} names no file in {}", folders[0].display()),
        }
    }
}

/// Draws the links in markdown that name a file in `folder`, or anywhere absolute, as the file
/// tree draws the file. A link to a page, or to a file that isn't there, stays a link.
pub struct FileLinks {
    folders: [PathBuf; 1],
}

impl FileLinks {
    pub fn new(folder: PathBuf) -> Self {
        FileLinks { folders: [folder] }
    }
}

/// What a link to a file shows: the file's icon, by its own name, beside what the link says.
struct Linked {
    name: SharedString,
}

impl MarkdownPlugin for FileLinks {
    fn name(&self) -> &str {
        FILE_LINK
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else {
            return None;
        };
        let Some(LinkTarget::File { path, .. }) = link_target(&link.url, &self.folders) else {
            return None;
        };
        let name = SharedString::from(file_name(&path));
        let said = node.to_string();
        let said = if said.trim().is_empty() {
            name.to_string()
        } else {
            said
        };
        Some(MarkdownNode::new(FILE_LINK, Linked { name }).text(said))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let linked = node.data::<Linked>()?;
        let p = theme::palette(cx);
        let size = f32::from(context.font_size());
        let named = linked.name.clone();
        // In the tree's colours, not a link's, which the text around it would give it.
        let name = div()
            .text_color(p.text)
            .text_decoration_none()
            .child(node.as_text().to_owned());
        let label = file_label(file_icon(&named), size, name, p)
            .debug_selector(move || format!("file-link-{named}"));
        Some(InlineElement::new(label.into_any_element()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use koyo_term::{LinkTarget, link_target};

    use crate::files::At;
    use crate::test_support::scratch;

    /// The file `target` names in `folder`, and the place it names in it; `None` for a page,
    /// or a path to nothing.
    fn linked(target: &str, folder: &Path) -> Option<(PathBuf, Option<At>)> {
        match link_target(target, &[folder.to_path_buf()])? {
            LinkTarget::File { path, line, .. } => Some((path, line.map(At::printed_line))),
            LinkTarget::Url(_) => None,
        }
    }

    #[test]
    fn a_link_to_a_file_names_it_and_its_line() {
        let folder = scratch("chat-links");
        std::fs::create_dir_all(folder.join("src/viewer")).unwrap();
        std::fs::write(folder.join("src/viewer/mod.rs"), "").unwrap();
        std::fs::write(folder.join("a picture.png"), "").unwrap();
        let file = folder.join("src/viewer/mod.rs");
        let link = |link: &str| linked(link, &folder);

        assert_eq!(link("src/viewer/mod.rs"), Some((file.clone(), None)));
        assert_eq!(
            link("./src/viewer/mod.rs#L42"),
            Some((file.clone(), Some(At::Line(41))))
        );
        assert_eq!(
            link("src/viewer/mod.rs#L42-L50"),
            Some((file.clone(), Some(At::Line(41))))
        );
        assert_eq!(
            link("src/viewer/mod.rs:42"),
            Some((file.clone(), Some(At::Line(41))))
        );
        assert_eq!(
            link("src/viewer/mod.rs:42:7"),
            Some((file.clone(), Some(At::Line(41))))
        );
        let absolute = file.display().to_string();
        assert_eq!(link(&absolute), Some((file.clone(), None)));
        assert_eq!(
            link(&format!("file://{absolute}")),
            Some((file.clone(), None))
        );
        assert_eq!(
            link("a%20picture.png"),
            Some((folder.join("a picture.png"), None))
        );
        assert_eq!(link("src/../src/viewer/mod.rs"), Some((file, None)));
    }

    #[test]
    fn a_link_to_a_page_or_to_nothing_names_no_file() {
        let folder = scratch("chat-links-none");
        let link = |link: &str| linked(link, &folder);
        assert_eq!(link("https://example.com/a.rs"), None);
        assert_eq!(link("mailto:someone@example.com"), None);
        assert_eq!(link("#heading"), None);
        assert_eq!(link("missing.rs"), None);
        assert_eq!(link("missing.rs:12"), None);
        assert_eq!(link(""), None);
        assert_eq!(linked("src", Path::new("/")), None);
    }
}
