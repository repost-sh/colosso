use super::*;

#[test]
fn feeds_bytes_and_reads_them_back() {
    let mut engine = Engine::new(Size::cells(20, 4), 1 << 20).unwrap();
    engine.write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\nsecond");
    let frame = engine.frame().unwrap();
    assert_eq!(frame.cols, 20);
    assert_eq!(frame.row_count(), 4);
    assert_eq!(frame.text(), "Hello, world!\nsecond");
    let world = &frame.rows[0].cells[7];
    assert_eq!(world.grapheme, Grapheme::Char('w'));
    assert!(world.attrs.contains(Attrs::BOLD));
    assert!(world.fg.is_some());
    let cursor = frame.cursor.unwrap();
    assert_eq!((cursor.col, cursor.row), (6, 1));
    assert_eq!(frame.dirty, Dirty::Full);
}

#[test]
fn a_second_frame_reuses_clean_rows() {
    let mut engine = Engine::new(Size::cells(10, 3), 1 << 20).unwrap();
    engine.write(b"one\r\ntwo\r\n");
    let first = engine.frame().unwrap();
    engine.write(b"x");
    let second = engine.frame().unwrap();
    assert_eq!(second.dirty, Dirty::Partial);
    assert!(Arc::ptr_eq(&first.rows[0], &second.rows[0]));
    assert!(!Arc::ptr_eq(&first.rows[2], &second.rows[2]));
    assert_eq!(second.text(), "one\ntwo\nx");
    let third = engine.frame().unwrap();
    assert_eq!(third.dirty, Dirty::Clean);
}

#[test]
fn effects_come_out_in_order() {
    let mut engine = Engine::new(Size::cells(10, 3), 1 << 20).unwrap();
    engine.write(b"\x1b]2;a title\x07\x07\x1b[c");
    let effects = engine.take_effects();
    assert_eq!(effects[0], Effect::Title("a title".into()));
    assert_eq!(effects[1], Effect::Bell);
    assert!(matches!(&effects[2], Effect::WriteBack(bytes) if bytes.starts_with(b"\x1b[?")));
    assert_eq!(engine.title(), "a title");
    assert!(engine.take_effects().is_empty());
}

#[test]
fn snapshots_round_trip() {
    let mut engine = Engine::new(Size::cells(12, 3), 1 << 20).unwrap();
    engine.write(b"keep \x1b[31mthis\x1b[0m\r\n> ");
    let bytes = engine.snapshot().unwrap();
    let mut restored = Engine::restore(&bytes).unwrap();
    assert_eq!(restored.cols(), 12);
    assert_eq!(
        restored.screen_text(TextFormat::Plain).unwrap(),
        engine.screen_text(TextFormat::Plain).unwrap()
    );
    assert_eq!(restored.rows(), 3);
    let frame = restored.frame().unwrap();
    assert_eq!(frame.text(), "keep this\n>");
    assert_eq!(
        frame.rows[0].cells[5].fg,
        engine.frame().unwrap().rows[0].cells[5].fg
    );
}

#[test]
fn an_empty_screen_formats_to_nothing() {
    let engine = Engine::new(Size::cells(12, 3), 1 << 20).unwrap();
    assert_eq!(engine.screen_text(TextFormat::Plain).unwrap(), "");
    let restored = Engine::restore(&engine.snapshot().unwrap()).unwrap();
    assert_eq!(restored.screen_text(TextFormat::Plain).unwrap(), "");
}

#[test]
fn formats_the_screen_as_text() {
    let mut engine = Engine::new(Size::cells(12, 3), 1 << 20).unwrap();
    engine.write(b"a\r\nb\r\n");
    let text = engine.screen_text(TextFormat::Plain).unwrap();
    assert!(text.starts_with("a\nb"), "{text:?}");
}

#[test]
fn scrolls_the_viewport_into_history() {
    let mut engine = Engine::new(Size::cells(8, 2), 1 << 20).unwrap();
    engine.write(b"1\r\n2\r\n3\r\n4");
    assert_eq!(engine.frame().unwrap().text(), "3\n4");
    assert!(engine.scrollbar().at_bottom());
    engine.scroll(Scroll::Top);
    assert_eq!(engine.frame().unwrap().text(), "1\n2");
    assert!(!engine.scrollbar().at_bottom());
    engine.scroll(Scroll::Bottom);
    assert_eq!(engine.frame().unwrap().text(), "3\n4");
}

#[test]
fn programs_are_told_the_hosts_theme() {
    let mut engine = Engine::new(Size::cells(10, 3), 1 << 20).unwrap();
    engine.set_default_colors(Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 }).unwrap();
    assert_eq!(engine.set_color_scheme(false).unwrap(), None);
    engine.write(b"\x1b]11;?\x07\x1b[?996n");
    let replies: Vec<Vec<u8>> = (engine.take_effects().into_iter())
        .filter_map(|effect| match effect {
            Effect::WriteBack(bytes) => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(replies, [b"\x1b]11;rgb:ffff/ffff/ffff\x07".to_vec(), b"\x1b[?997;2n".to_vec()]);
    // A change is reported only to a program that asked to hear of changes.
    assert_eq!(engine.set_color_scheme(true).unwrap(), None);
    engine.write(b"\x1b[?2031h");
    assert_eq!(engine.set_color_scheme(true).unwrap(), None);
    assert_eq!(engine.set_color_scheme(false).unwrap(), Some(b"\x1b[?997;2n".to_vec()));
}

#[test]
fn a_link_a_program_prints_covers_its_cells() {
    let mut engine = Engine::new(Size::cells(30, 3), 1 << 20).unwrap();
    engine.write(b"see \x1b]8;;file:///abs/a%20b.png\x1b\\a.png\x1b]8;;\x1b\\ or ");
    engine.write(b"\x1b]8;;https://example.com/\x1b\\\xe5\xad\x97x\x1b]8;;\x1b\\\r\nplain");
    let frame = engine.frame().unwrap();
    let links = &frame.rows[0].links;
    assert_eq!(links.len(), 2, "{links:?}");
    assert_eq!(links[0].cols, 4..9);
    assert_eq!(&*links[0].uri, "file:///abs/a%20b.png");
    // The wide character's tail is the link's too.
    assert_eq!(links[1].cols, 13..16);
    assert_eq!(&*links[1].uri, "https://example.com/");
    assert!(frame.rows[1].links.is_empty());
}
