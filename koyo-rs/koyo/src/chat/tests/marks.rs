//! The rows that say what became of a run (`chat/marks.rs`), drawn: what a notice or a failure
//! says wraps at the column's width, however long it is.

use gpui_kit::{TestAppContext, VisualTestContext};
use koyo_agent::protocol::model::{Failure, FailureKind, ItemBody, Notice, NoticeLevel, RunStatus};
use koyo_agent::timeline::RowKind;

use super::drawing::{settled_with, shell_with_chat};

/// A warning that says `text`.
fn warning(text: &str) -> ItemBody {
    ItemBody::Notice(Notice {
        level: NoticeLevel::Warning,
        text: text.into(),
    })
}

/// Text straight in a row is as wide as it's long: a notice of a title and one long line stayed
/// two lines high and ran out of the column, to the window's edge.
#[gpui_kit::test]
fn a_long_notice_and_a_long_failure_wrap_at_the_columns_width(cx: &mut TestAppContext) {
    // Far wider than the widest column, on one line.
    let long =
        "Claude's earlier thinking was written while you were signed in elsewhere. ".repeat(8);
    let notices = vec![
        ("short", warning("Its title\nA line.")),
        ("long", warning(&format!("Its title\n{long}"))),
    ];
    let mut shown = settled_with("0001", notices);
    shown.runs[0].status = RunStatus::Failed;
    shown.runs[0].failure = Some(Failure {
        kind: FailureKind::Api,
        message: long,
        retry_at: None,
    });
    let (window, _shell, view, _thread) = shell_with_chat("chat-marks-wrap", shown, cx);

    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let kinds = timeline.read_with(cx, |timeline, _| {
        let kinds = timeline.rows.iter().map(|row| row.kind.clone());
        kinds.collect::<Vec<_>>()
    });
    let rows = |is: fn(&RowKind) -> bool| {
        let found = kinds.iter().enumerate().filter(|(_, kind)| is(kind));
        found.map(|(at, _)| at).collect::<Vec<_>>()
    };
    let height = |row: usize, cx: &mut TestAppContext| {
        let drawn = VisualTestContext::from_window(window, cx)
            .debug_bounds(format!("chat-row-{row}").leak())
            .unwrap_or_else(|| panic!("row {row} is drawn"));
        drawn.size.height
    };
    let [short, wrapped] = rows(|kind| matches!(kind, RowKind::Notice { .. }))[..] else {
        panic!("a row for each notice: {kinds:?}");
    };
    let [failure] = rows(|kind| matches!(kind, RowKind::Failure { .. }))[..] else {
        panic!("a row for the failure: {kinds:?}");
    };
    let two_lines = height(short, cx);
    assert!(
        height(wrapped, cx) > two_lines * 1.5,
        "the long notice takes the lines it needs: {:?} against {two_lines:?}",
        height(wrapped, cx)
    );
    assert!(
        height(failure, cx) > two_lines,
        "and so does the failure: {:?} against {two_lines:?}",
        height(failure, cx)
    );
}
