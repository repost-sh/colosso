use koyo_term_engine::{Engine, Size, Width};

use super::*;

/// The files of a made-up disk, where relative paths are looked for in `/work`.
const FILES: [&str; 8] = [
    "/work/src/main.rs",
    "/work/src/foo.rs",
    "/work/src/a.rs",
    "/work/docs/diagram.svg",
    "/work/app/(auth)/page.tsx",
    "/abs/path/shot.png",
    "/home/me/x/y.pdf",
    "/work/README",
];

/// The frame a terminal `cols` wide shows after `bytes`.
fn frame(cols: u16, bytes: &str) -> Frame {
    let mut engine = Engine::new(Size::cells(cols, 4), 1 << 20).unwrap();
    engine.write(bytes.as_bytes());
    engine.frame().unwrap()
}

/// The link in `frame` at `at`, on the made-up disk.
fn link_at(frame: &Frame, at: GridPoint) -> Option<Found> {
    let mut is_file = |path: &Path| FILES.iter().any(|file| Path::new(file) == path);
    let roots = [PathBuf::from("/work")];
    find(frame, at, &roots, Some(Path::new("/home/me")), &mut is_file)
}

/// The cell the first character of `pointed` is shown in, on the screen's first line.
fn cell_of(frame: &Frame, pointed: &str) -> GridPoint {
    let line = Line::of(frame, 0);
    let text: String = line.text.iter().collect();
    let byte = text.find(pointed).unwrap();
    let (row, col, _) = line.cells[text[..byte].chars().count()];
    GridPoint { col, row }
}

/// The link on a line of `text` at the first character of `pointed`, and the text it
/// underlines.
fn link(text: &str, pointed: &str) -> Option<(LinkTarget, String)> {
    let frame = frame(80, text);
    let found = link_at(&frame, cell_of(&frame, pointed))?;
    let underlined = (found.cells.iter())
        .flat_map(|(row, cols)| {
            let row = &frame.rows[usize::from(*row)];
            cols.clone().map(move |col| &row.cells[usize::from(col)])
        })
        .filter(|cell| cell.width != Width::SpacerTail)
        .map(|cell| cell.grapheme.text().into_owned())
        .collect();
    Some((found.target, underlined))
}

fn file(path: &str, line: Option<u32>, column: Option<u32>) -> LinkTarget {
    LinkTarget::File {
        path: path.into(),
        line,
        column,
    }
}

#[test]
fn a_path_in_a_tool_header_is_found() {
    let main = file("/work/src/main.rs", None, None);
    let found = Some((main, "src/main.rs".to_owned()));
    assert_eq!(link("⏺ Read(src/main.rs)", "main"), found);
    assert_eq!(link("⏺ Read(src/main.rs)", "src"), found);
    assert_eq!(link("⏺ Read(src/main.rs)", "Read"), None);
    assert_eq!(
        link("  ⎿  Update(src/a.rs) with 2 additions", "a.rs"),
        Some((file("/work/src/a.rs", None, None), "src/a.rs".to_owned()))
    );
}

#[test]
fn a_line_and_column_after_a_path_are_kept() {
    assert_eq!(
        link("error at src/foo.rs:42:7", "foo"),
        Some((file("/work/src/foo.rs", Some(42), Some(7)), "src/foo.rs:42:7".to_owned()))
    );
    assert_eq!(
        link("src/foo.rs:42", "42"),
        Some((file("/work/src/foo.rs", Some(42), None), "src/foo.rs:42".to_owned()))
    );
}

#[test]
fn a_full_stop_or_colon_after_a_path_is_left_out() {
    let foo = Some((file("/work/src/foo.rs", None, None), "src/foo.rs".to_owned()));
    assert_eq!(link("It's in src/foo.rs.", "foo"), foo);
    assert_eq!(link("In src/foo.rs: a warning", "foo"), foo);
    assert_eq!(
        link("src/foo.rs:42: unused import", "src"),
        Some((file("/work/src/foo.rs", Some(42), None), "src/foo.rs:42".to_owned()))
    );
}

#[test]
fn absolute_home_and_dotted_paths_are_found() {
    assert_eq!(
        link("Saved /abs/path/shot.png", "shot"),
        Some((file("/abs/path/shot.png", None, None), "/abs/path/shot.png".to_owned()))
    );
    assert_eq!(
        link("open ~/x/y.pdf", "y.pdf"),
        Some((file("/home/me/x/y.pdf", None, None), "~/x/y.pdf".to_owned()))
    );
    assert_eq!(
        link("see ./docs/diagram.svg", "docs"),
        Some((file("/work/docs/diagram.svg", None, None), "./docs/diagram.svg".to_owned()))
    );
}

#[test]
fn a_path_in_a_box_is_found() {
    let a = Some((file("/work/src/a.rs", None, None), "src/a.rs".to_owned()));
    assert_eq!(link("│ src/a.rs │", "a.rs"), a);
    assert_eq!(link("╭─│src/a.rs│─╮", "src"), a);
    assert_eq!(link("• `src/a.rs`", "src"), a);
}

#[test]
fn a_route_group_in_a_path_is_kept() {
    let page = Some((
        file("/work/app/(auth)/page.tsx", None, None),
        "app/(auth)/page.tsx".to_owned(),
    ));
    assert_eq!(link("Read(app/(auth)/page.tsx)", "page"), page);
    assert_eq!(link("Read(app/(auth)/page.tsx)", "auth"), page);
    assert_eq!(link("app/(auth)/page.tsx", "app"), page);
}

#[test]
fn a_url_leaves_out_what_a_sentence_puts_after_it() {
    let url = |url: &str| Some((LinkTarget::Url(url.to_owned()), url.to_owned()));
    assert_eq!(
        link("(see https://example.com/a).", "example"),
        url("https://example.com/a")
    );
    assert_eq!(
        link("docs: https://en.wikipedia.org/wiki/Rust_(language)).", "wiki"),
        url("https://en.wikipedia.org/wiki/Rust_(language)")
    );
    assert_eq!(link("at http://localhost:3000/,", "http"), url("http://localhost:3000/"));
    assert_eq!(link("just https:// alone", "https"), None);
}

#[test]
fn a_link_wrapped_onto_the_next_row_is_one_link() {
    // Ten columns: `see src/ma` and `in.rs now`.
    let frame = frame(10, "see src/main.rs now");
    assert!(frame.rows[0].wrapped);
    let found = link_at(&frame, GridPoint { col: 1, row: 1 }).unwrap();
    assert_eq!(found.target, file("/work/src/main.rs", None, None));
    assert_eq!(found.cells, [(0, 4..10), (1, 0..5)]);
    let from_above = link_at(&frame, GridPoint { col: 5, row: 0 }).unwrap();
    assert_eq!(from_above, found);
}

#[test]
fn wide_characters_before_a_path_take_their_two_columns() {
    let frame = frame(20, "字字 src/a.rs");
    let found = link_at(&frame, GridPoint { col: 6, row: 0 }).unwrap();
    assert_eq!(found.target, file("/work/src/a.rs", None, None));
    assert_eq!(found.cells, [(0, 5..13)]);
    // The tail of a wide character is pointed at as the character.
    assert_eq!(link_at(&frame, GridPoint { col: 1, row: 0 }), None);
}

#[test]
fn a_word_that_isnt_a_file_is_no_link() {
    assert_eq!(link("hello src/missing.rs world", "missing"), None);
    assert_eq!(link("hello world", "world"), None);
    assert_eq!(link("hello world", " "), None);
    assert_eq!(
        link("read README first", "README"),
        Some((file("/work/README", None, None), "README".to_owned()))
    );
}

#[test]
fn a_folder_is_no_link() {
    // The crate's own folder, on the disk.
    let crate_folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut links = Links {
        roots: vec![crate_folder.clone()],
        ..Links::default()
    };
    let frame = frame(80, "src/ src src/links.rs");
    assert_eq!(links.find(&frame, GridPoint { col: 1, row: 0 }), None);
    assert_eq!(links.find(&frame, GridPoint { col: 6, row: 0 }), None);
    let found = links.find(&frame, GridPoint { col: 10, row: 0 }).unwrap();
    assert_eq!(found.target, file(&crate_folder.join("src/links.rs").to_string_lossy(), None, None));
}

#[test]
fn paths_known_to_be_files_are_asked_of_the_disk_once_in_a_while() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/test-links")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("made.txt");
    std::fs::write(&path, "made").unwrap();
    let mut files = Files::default();
    assert!(files.is_file(&path));
    std::fs::remove_file(&path).unwrap();
    assert!(files.is_file(&path), "known from a moment ago");
    files.since = Instant::now().checked_sub(FORGET * 2);
    assert!(!files.is_file(&path), "asked again once it's old");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_link_the_program_made_opens_what_it_names() {
    let frame = frame(
        60,
        "\x1b]8;;file:///abs/path/shot.png#L3\x1b\\the shot\x1b]8;;\x1b\\ and \
         \x1b]8;;https://example.com/\x1b\\a site\x1b]8;;\x1b\\ and \
         \x1b]8;;mailto:me@example.com\x1b\\src/a.rs\x1b]8;;\x1b\\",
    );
    let shot = link_at(&frame, cell_of(&frame, "shot")).unwrap();
    assert_eq!(shot.target, file("/abs/path/shot.png", Some(3), None));
    assert_eq!(shot.cells, [(0, 0..8)]);
    let site = link_at(&frame, cell_of(&frame, "a site")).unwrap();
    assert_eq!(site.target, LinkTarget::Url("https://example.com/".into()));
    // A link of another kind is looked through, to the text it shows.
    let text = link_at(&frame, cell_of(&frame, "src")).unwrap();
    assert_eq!(text.target, file("/work/src/a.rs", None, None));
}

#[test]
fn a_relative_path_is_looked_for_in_the_shells_folder_first() {
    let frame = frame(60, "\x1b]7;file://host/abs/path\x1b\\shot.png and README");
    assert_eq!(frame.pwd, "file://host/abs/path");
    let shot = link_at(&frame, cell_of(&frame, "shot")).unwrap();
    assert_eq!(shot.target, file("/abs/path/shot.png", None, None));
    // Not in the shell's folder: in the checkout.
    let readme = link_at(&frame, cell_of(&frame, "README")).unwrap();
    assert_eq!(readme.target, file("/work/README", None, None));
}

#[test]
fn a_file_uri_is_decoded() {
    assert_eq!(
        file_uri("file://host/a%20b/c%C3%A9.rs#L42"),
        Some(("/a b/cé.rs".to_owned(), Some(42)))
    );
    assert_eq!(file_uri("file:///a.png"), Some(("/a.png".to_owned(), None)));
    assert_eq!(file_uri("file:///100%"), Some(("/100%".to_owned(), None)));
    assert_eq!(file_uri("https://example.com/"), None);
}

#[test]
fn the_shells_folder_is_read_from_either_uri_its_integration_sends() {
    assert_eq!(shell_folder("file://host/a%20b"), Some(PathBuf::from("/a b")));
    assert_eq!(shell_folder("kitty-shell-cwd://host/a b"), Some(PathBuf::from("/a b")));
    assert_eq!(shell_folder(""), None);
}

#[test]
fn a_path_is_tidied_as_a_tab_names_it() {
    let tidy = |path: &str| tidied(Path::new(path));
    assert_eq!(tidy("/a/./b/../c.png"), PathBuf::from("/a/c.png"));
    assert_eq!(tidy("a/../../b"), PathBuf::from("../b"));
    assert_eq!(tidy("../../b"), PathBuf::from("../../b"));
}
