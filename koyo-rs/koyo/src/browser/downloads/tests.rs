//! The downloads list against a host of the test's own: where a download goes, what the list
//! says of it as the host tells it, canceling, and a page saved as a PDF.

use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;
use koyo_browser_ipc::{self as ipc, AppRequest, DownloadAct, DownloadState, HostEvent, Print};

use super::*;
use crate::browser::host::fake::FakeHost;

mod top_bar;

const PAGE: PaneId = 7;

/// A host of the test's own, and the list's folder, a scratch folder with `report.pdf` in it.
fn started(name: &str, cx: &mut TestAppContext) -> (FakeHost, PathBuf) {
    cx.update(crate::test_support::init);
    let host = FakeHost::serve(name, cx);
    let folder = crate::test_support::scratch(&format!("{name}-downloads"));
    std::fs::write(folder.join("report.pdf"), "an earlier one").unwrap();
    let downloads = cx.update(Downloads::global);
    downloads.update(cx, |downloads, _| downloads.folder = Some(folder.clone()));
    (host, folder)
}

fn asked(id: u32, at: At, cx: &mut TestAppContext) {
    let downloads = cx.update(Downloads::global);
    downloads.update(cx, |downloads, cx| downloads.asked(id, "report.pdf", at, cx));
    cx.run_until_parked();
}

/// What the app had the host do with downloads.
fn acts(host: &FakeHost, cx: &mut TestAppContext) -> Vec<(u32, DownloadAct)> {
    (host.asked(cx).into_iter())
        .filter_map(|request| match request {
            AppRequest::Download { download, act } => Some((download, act)),
            _ => None,
        })
        .collect()
}

fn save_to(path: &Path) -> DownloadAct {
    DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() }
}

fn stages(cx: &mut TestAppContext) -> Vec<(String, Stage)> {
    cx.update(|cx| {
        (Downloads::listed(cx).iter())
            .map(|entry| (entry.name.clone(), entry.stage.clone()))
            .collect()
    })
}

fn going(id: u32, path: &Path, received: u64, state: DownloadState) -> HostEvent {
    HostEvent::Download {
        download: ipc::Download {
            id,
            url: "https://example.com/report".into(),
            path: path.to_string_lossy().into_owned(),
            received,
            total: Some(2048),
            state,
        },
    }
}

#[gpui_kit::test]
fn a_download_goes_to_the_downloads_folder_under_a_name_nothing_there_has(cx: &mut TestAppContext) {
    let (host, folder) = started("download-free-name", cx);
    asked(1, At::Person, cx);
    asked(2, At::Nobody, cx);
    assert_eq!(
        acts(&host, cx),
        [
            (1, save_to(&folder.join("report (1).pdf"))),
            (2, save_to(&folder.join("report (2).pdf"))),
        ],
        "the file there is kept, and the second doesn't take the first's place"
    );
    assert_eq!(
        stages(cx),
        [("report (1).pdf".into(), Stage::Going), ("report (2).pdf".into(), Stage::Going)]
    );
}

#[gpui_kit::test]
fn the_list_says_what_the_host_says_until_it_is_cleared(cx: &mut TestAppContext) {
    let (host, folder) = started("download-follows", cx);
    asked(1, At::Person, cx);
    let path = folder.join("report (1).pdf");
    host.says(going(1, &path, 512, DownloadState::Going), cx);
    cx.update(|cx| {
        let entry = &Downloads::listed(cx)[0];
        assert_eq!((entry.received, entry.total, &entry.stage), (512, Some(2048), &Stage::Going));
        assert_eq!(panel::status(entry), "512 bytes of 2.0 KB");
    });
    host.says(going(1, &path, 2048, DownloadState::Done), cx);
    let reason = "The network went away.".to_owned();
    asked(2, At::Person, cx);
    let second = folder.join("report (2).pdf");
    host.says(going(2, &second, 10, DownloadState::Failed { reason: reason.clone() }), cx);
    assert_eq!(
        stages(cx),
        [("report (1).pdf".into(), Stage::Done), ("report (2).pdf".into(), Stage::Failed(reason))]
    );
    let downloads = cx.update(Downloads::global);
    downloads.update(cx, Downloads::clear);
    assert_eq!(stages(cx), [], "both ended");
}

#[gpui_kit::test]
fn a_canceled_download_stays_canceled_though_the_host_heard_late(cx: &mut TestAppContext) {
    let (host, folder) = started("download-cancel", cx);
    asked(1, At::Person, cx);
    let placed = acts(&host, cx);
    assert_eq!(placed.len(), 1, "{placed:?}");
    let downloads = cx.update(Downloads::global);
    let key = Key::Engine(1);
    downloads.update(cx, |downloads, cx| downloads.cancel(key, cx));
    cx.run_until_parked();
    assert_eq!(acts(&host, cx), [(1, DownloadAct::Cancel)]);
    host.says(going(1, &folder.join("report (1).pdf"), 64, DownloadState::Going), cx);
    assert_eq!(acts(&host, cx), [], "the host was told");
    assert_eq!(stages(cx), [("report (1).pdf".into(), Stage::Canceled)]);
}

#[gpui_kit::test]
fn a_page_an_agent_drives_never_asks_where_its_download_goes(cx: &mut TestAppContext) {
    let (host, folder) = started("download-ask", cx);
    cx.update(|cx| crate::settings::update(cx, |settings| settings.downloads_ask = true));
    asked(1, At::Agent("claude-code:7f3a".into()), cx);
    asked(2, At::Person, cx);
    assert_eq!(
        acts(&host, cx),
        [(1, save_to(&folder.join("report (1).pdf")))],
        "the person's waits for the save panel"
    );
    cx.update(|cx| {
        let listed = Downloads::listed(cx);
        assert_eq!(listed[0].by.as_deref(), Some("claude-code:7f3a"));
        assert!(panel::status(&listed[0]).ends_with(" · claude-code:7f3a"));
        assert_eq!(listed[1].stage, Stage::Placing);
    });
}

#[gpui_kit::test]
fn a_page_saved_as_a_pdf_is_listed_until_it_is_written(cx: &mut TestAppContext) {
    let (host, folder) = started("download-pdf", cx);
    let downloads = cx.update(Downloads::global);
    let path = folder.join("Example");
    downloads.update(cx, |downloads, cx| downloads.saving_pdf(PAGE, path, cx));
    let written = folder.join("Example.pdf");
    let printed: Vec<AppRequest> = (host.asked(cx).into_iter())
        .filter(|request| matches!(request, AppRequest::Print { .. }))
        .collect();
    let pdf = Print::Pdf { path: written.to_string_lossy().into_owned() };
    assert_eq!(printed, [AppRequest::Print { pane: PAGE, print: pdf }]);
    assert_eq!(stages(cx), [("Example.pdf".into(), Stage::Going)]);
    let said = |saved| HostEvent::PdfSaved {
        pane: PAGE,
        path: written.to_string_lossy().into_owned(),
        saved,
    };
    host.says(said(true), cx);
    assert_eq!(stages(cx), [("Example.pdf".into(), Stage::Done)]);
    downloads.update(cx, |downloads, cx| downloads.saving_pdf(PAGE, folder.join("Again.pdf"), cx));
    host.says(said(false), cx);
    assert_eq!(stages(cx)[1].1, Stage::Going, "another file's failure isn't this one's");
}

#[gpui_kit::test]
fn a_pdf_named_with_no_extension_never_takes_the_place_of_a_file_the_panel_didnt_ask_about(
    cx: &mut TestAppContext,
) {
    let (host, folder) = started("download-pdf-named", cx);
    let downloads = cx.update(Downloads::global);
    let named = folder.join("report");
    downloads.update(cx, |downloads, cx| downloads.saving_pdf(PAGE, named, cx));
    let printed: Vec<AppRequest> = (host.asked(cx).into_iter())
        .filter(|request| matches!(request, AppRequest::Print { .. }))
        .collect();
    let free = folder.join("report (1).pdf").to_string_lossy().into_owned();
    let pdf = Print::Pdf { path: free };
    assert_eq!(printed, [AppRequest::Print { pane: PAGE, print: pdf }], "report.pdf is there already");
    assert_eq!(stages(cx), [("report (1).pdf".into(), Stage::Going)]);
}

#[test]
fn names_are_numbered_before_their_extension() {
    assert_eq!(numbered("report.pdf", 0), "report.pdf");
    assert_eq!(numbered("report.pdf", 2), "report (2).pdf");
    assert_eq!(numbered("archive.tar.gz", 1), "archive (1).tar.gz");
    assert_eq!(numbered("README", 1), "README (1)");
    assert_eq!(numbered(".env", 1), ".env (1)");
    assert_eq!(file_name("../../etc/passwd"), "passwd");
    assert_eq!(file_name("  "), "download");
    assert_eq!(pdf_name("A/B: notes"), "A-B- notes.pdf");
    assert_eq!(pdf_name(""), "Page.pdf");
}

#[test]
fn sizes_read_as_the_finder_says_them() {
    assert_eq!(size(512), "512 bytes");
    assert_eq!(size(2048), "2.0 KB");
    assert_eq!(size(4_500_000), "4.5 MB");
    assert_eq!(size(999_999), "1.0 MB");
    assert_eq!(size(1_100_000_000), "1.1 GB");
}
