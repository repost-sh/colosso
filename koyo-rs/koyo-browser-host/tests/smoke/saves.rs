//! What a page saves (`docs/browser.md` 16): a download asks where it goes and comes in there,
//! one canceled before it has a place saves nothing, a page the app closes while one comes in
//! stays open until it ends, and one that closes itself takes it with it, which the app is
//! told; the page saves as a PDF, prints in Chrome's print preview, and its
//! context menu keeps Chrome's Print.

use koyo_browser_ipc::{AppRequest, DownloadAct, DownloadState, HostEvent, MenuEntry, Print, Viewport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::agents::cdp;
use crate::support::{Session, close, evaluate, launch, open, until};

/// `/` links to the downloads and counts the times it's printed; `/file` is an attachment;
/// `/slow` one that comes a part at a time.
const PAGE: &str = "<title>saves</title><body style='margin:0'>\
    <p style='height:200px'>a page to save</p>\
    <a id=file href='/file'>file</a> <a id=slow href='/slow'>slow</a>\
    <script>window.printed = 0; addEventListener('beforeprint', () => printed += 1);</script></body>";
const FILE: &[u8] = b"what the server sent\n";
const PART: &[u8] = &[7; 4096];
const PARTS: usize = 12;
const PART_EVERY: Duration = Duration::from_millis(250);

/// Serves [`PAGE`] and its downloads at a local address.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let address = format!("http://{}/", listener.local_addr().expect("an address"));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while !request.windows(4).any(|end| end == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(read) if read > 0 => request.extend_from_slice(&chunk[..read]),
                        _ => return,
                    }
                }
                let line = String::from_utf8_lossy(&request);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let attachment = |name: &str, length: usize| {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                         Content-Disposition: attachment; filename=\"{name}\"\r\n\
                         Content-Length: {length}\r\nConnection: close\r\n\r\n"
                    )
                };
                let _ = match path.as_str() {
                    "/file" => stream
                        .write_all(attachment("report.txt", FILE.len()).as_bytes())
                        .and_then(|()| stream.write_all(FILE)),
                    "/slow" => {
                        let head = attachment("slow.bin", PART.len() * PARTS);
                        let mut sent = stream.write_all(head.as_bytes());
                        for _ in 0..PARTS {
                            sent = sent.and_then(|()| stream.write_all(PART)).and_then(|()| stream.flush());
                            std::thread::park_timeout(PART_EVERY);
                        }
                        sent
                    }
                    _ => write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                        PAGE.len()
                    ),
                };
            });
        }
    });
    address
}

/// A folder of the test's own, by its whole path, which CEF writes files to.
fn folder(launch: &koyo_browser_ipc::client::Launch) -> PathBuf {
    let folder = launch.profiles.parent().expect("scratch").join("saved");
    std::fs::create_dir_all(&folder).expect("a folder");
    std::fs::canonicalize(folder).expect("its whole path")
}

/// Clicks the link `id` of `pane`'s page, and waits for the download it starts to ask where
/// it goes: its number, and the name it was given.
fn download(session: &mut Session, pane: u64, id: &str) -> (u32, String) {
    let click = format!("document.getElementById('{id}').click(); true");
    let params = serde_json::json!({ "expression": click });
    let caller = koyo_browser_ipc::CdpCaller::Agent;
    let clicked = AppRequest::Cdp { pane, call: 900 + pane, caller, session: None, method: "Runtime.evaluate".into(), params };
    session.host.send(&clicked).expect("the click");
    let mut asked = None;
    session.wait("the download to ask where it goes", |event| match event {
        HostEvent::DownloadAsked { pane: of, download, name, .. } if *of == pane => {
            asked = Some((*download, name.clone()));
            true
        }
        _ => false,
    });
    asked.expect("asked")
}

/// Waits for `pane`'s download `id` to stand as `state` says, telling it how it went.
fn until_state(session: &mut Session, pane: u64, id: u32, state: impl Fn(&DownloadState) -> bool) -> koyo_browser_ipc::Download {
    let mut seen = None;
    session.wait("the download to get there", |event| match event {
        HostEvent::Download { pane: of, download } if *of == pane && download.id == id && state(&download.state) => {
            seen = Some(download.clone());
            true
        }
        _ => false,
    });
    seen.expect("seen")
}

fn act(session: &mut Session, pane: u64, download: u32, act: DownloadAct) {
    let request = AppRequest::Download { pane, download, act };
    session.host.send(&request).expect("the host hears it");
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_comes_in_where_the_app_says_and_one_canceled_saves_nothing() {
    let launch = launch("downloads");
    let mut session = Session::start(&launch);
    let address = serve();
    let saved = folder(&launch);
    open(&mut session, &address, "saves");
    let (id, name) = download(&mut session, 1, "file");
    assert_eq!(name, "report.txt", "the name the server gave it");
    let path = saved.join("report (1).txt");
    act(&mut session, 1, id, DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() });
    let done = until_state(&mut session, 1, id, |state| *state != DownloadState::Going);
    assert_eq!(done.state, DownloadState::Done);
    assert_eq!((done.received, done.path.as_str()), (FILE.len() as u64, path.to_str().expect("utf-8")));
    assert_eq!(std::fs::read(&path).expect("the file"), FILE);
    let quarantine = std::process::Command::new("xattr")
        .args(["-p", "com.apple.quarantine"])
        .arg(&path)
        .output()
        .expect("xattr");
    assert!(quarantine.status.success(), "quarantined for Gatekeeper, as Chrome's downloads are");

    let viewport = Viewport { width: 400, height: 300, scale: 1. };
    let opened = AppRequest::Open { pane: 2, url: address, viewport, profile: "smoke".into() };
    session.host.send(&opened).expect("open");
    session.wait("the second page", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title == "saves" && !state.loading)
    });
    let (id, _) = download(&mut session, 2, "file");
    act(&mut session, 2, id, DownloadAct::Cancel);
    let canceled = until_state(&mut session, 2, id, |state| *state != DownloadState::Going);
    assert_eq!(canceled.state, DownloadState::Canceled);
    let left: Vec<PathBuf> = (std::fs::read_dir(&saved).expect("the folder").flatten()).map(|entry| entry.path()).collect();
    assert_eq!(left, [path], "nothing more was saved");
    close(session, &launch);
}

/// Waits for `pane`'s page to close and its download `id` to end after it, as Chrome's outlive
/// their tab: whether its tab was said to close unasked, and how the download ended.
fn closes_then_ends(session: &mut Session, pane: u64, id: u32) -> (bool, DownloadState) {
    let (mut tab_closed, mut closed) = (false, false);
    session.wait("the page to close", |event| match event {
        HostEvent::TabClosed { pane: of } if *of == pane => {
            tab_closed = true;
            false
        }
        HostEvent::Download { pane: of, download } if *of == pane && download.id == id => {
            assert_eq!(download.state, DownloadState::Going, "the download ended before its page closed");
            false
        }
        HostEvent::Closed { pane: of } if *of == pane => {
            closed = true;
            true
        }
        _ => false,
    });
    assert!(closed);
    let ended = until_state(session, pane, id, |state| *state != DownloadState::Going);
    (tab_closed, ended.state)
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_closed_while_its_download_comes_in_goes_and_the_download_still_ends() {
    let launch = launch("download-outlives-close");
    let mut session = Session::start(&launch);
    let address = serve();
    let path = folder(&launch).join("slow.bin");
    open(&mut session, &address, "saves");
    let (id, _) = download(&mut session, 1, "slow");
    act(&mut session, 1, id, DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() });
    until_state(&mut session, 1, id, |state| *state == DownloadState::Going);
    session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    let (_, ended) = closes_then_ends(&mut session, 1, id);
    assert_eq!(ended, DownloadState::Done);
    assert_eq!(std::fs::read(&path).expect("the file").len(), PART.len() * PARTS);
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_that_closes_itself_while_its_download_comes_in_leaves_it_to_finish() {
    let launch = launch("download-outlives-self-close");
    let mut session = Session::start(&launch);
    let address = serve();
    let path = folder(&launch).join("slow.bin");
    open(&mut session, &address, "saves");
    let (id, _) = download(&mut session, 1, "slow");
    act(&mut session, 1, id, DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() });
    until_state(&mut session, 1, id, |state| *state == DownloadState::Going);
    let params = serde_json::json!({ "expression": "window.close(); true" });
    let caller = koyo_browser_ipc::CdpCaller::Agent;
    let closing = AppRequest::Cdp { pane: 1, call: 950, caller, session: None, method: "Runtime.evaluate".into(), params };
    session.host.send(&closing).expect("the page closes itself");
    let (tab_closed, ended) = closes_then_ends(&mut session, 1, id);
    assert!(tab_closed, "the app is told its tab goes");
    assert_eq!(ended, DownloadState::Done);
    assert_eq!(std::fs::read(&path).expect("the file").len(), PART.len() * PARTS);
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn the_page_saves_as_a_pdf_and_prints_in_chromes_print_preview() {
    let launch = launch("print");
    let mut session = Session::start(&launch);
    let address = serve();
    let pdf = folder(&launch).join("saves.pdf");
    open(&mut session, &address, "saves");
    let print = Print::Pdf { path: pdf.to_string_lossy().into_owned() };
    session.host.send(&AppRequest::Print { pane: 1, print }).expect("print");
    let mut saved = None;
    session.wait("the PDF", |event| match event {
        HostEvent::PdfSaved { pane: 1, path, saved: ok } => {
            saved = Some((path.clone(), *ok));
            true
        }
        _ => false,
    });
    assert_eq!(saved, Some((pdf.to_string_lossy().into_owned(), true)));
    let written = std::fs::read(&pdf).expect("the PDF");
    assert!(written.starts_with(b"%PDF-"), "{:?}", &written[..written.len().min(16)]);

    let press = |kind: &str| {
        serde_json::json!({ "type": kind, "x": 20, "y": 20, "button": "right", "clickCount": 1 })
    };
    cdp(&mut session, 801, "Input.dispatchMouseEvent", press("mousePressed"));
    cdp(&mut session, 802, "Input.dispatchMouseEvent", press("mouseReleased"));
    let mut menu = None;
    session.wait("the page's menu", |event| match event {
        HostEvent::ContextMenu { pane: 1, menu: id, entries, .. } => {
            menu = Some((*id, entries.clone()));
            true
        }
        _ => false,
    });
    let (id, entries) = menu.expect("a menu");
    let labels: Vec<&str> = (entries.iter())
        .filter_map(|entry| match entry {
            MenuEntry::Command { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert!(labels.iter().any(|label| label.starts_with("Print")), "Chrome's Print row: {labels:?}");
    session.host.send(&AppRequest::MenuCommand { pane: 1, menu: id, command: None }).expect("closed");

    let before = evaluate(&mut session, "printed");
    session.host.send(&AppRequest::Print { pane: 1, print: Print::Preview }).expect("print");
    let printed = until(&mut session, "JSON.stringify(printed)", "the page to be printed", Duration::from_secs(20), |printed| {
        printed.as_u64() > before.as_u64()
    });
    assert!(printed.as_u64() > before.as_u64());
    let quiet = Instant::now() + Duration::from_millis(500);
    assert!(!session.pump(quiet, |event| matches!(event, HostEvent::Gone { .. })), "the page stays up");
    close(session, &launch);
}
