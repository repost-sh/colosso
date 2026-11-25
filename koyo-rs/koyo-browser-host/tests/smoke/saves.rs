//! What a page saves (`docs/browser.md` 16): a download asks where it goes and comes in there,
//! one canceled before it has a place saves nothing, one whose page closes goes on to its end,
//! its profile's data cleared or not, and the host shutting down ends it; the page saves as a
//! PDF, prints in Chrome's print preview, and its context menu keeps Chrome's Print.

use koyo_browser_ipc::{AppRequest, DownloadAct, DownloadState, HostEvent, MenuEntry, Print, Viewport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::agents::cdp;
use crate::support::{Session, close, evaluate, launch, open, until};

/// `/` links to the downloads and counts the times it's printed; `/file` is an attachment;
/// `/slow` one whose rest comes once it's let; `/cut` one the server drops halfway, whose rest
/// it has for who asks; `/late` one it answers once it's let, `download` in its link.
const PAGE: &str = "<title>saves</title><body style='margin:0'>\
    <p style='height:200px'>a page to save</p>\
    <a id=file href='/file'>file</a> <a id=slow href='/slow'>slow</a> <a id=cut href='/cut'>cut</a>\
    <a id=late href='/late' download>late</a>\
    <script>window.printed = 0; addEventListener('beforeprint', () => printed += 1);</script></body>";
const FILE: &[u8] = b"what the server sent\n";
const PART: &[u8] = &[7; 64 * 1024];

/// [`PAGE`] and its downloads, served at a local address.
struct Server {
    address: String,
    /// Each sent lets the rest of a `/slow` come, or a `/late` be answered.
    rest: Sender<()>,
    /// Told of each `/late` asked for.
    late: Receiver<()>,
}

fn serve() -> Server {
    let (rest, let_rest) = channel();
    let (tell, late) = channel();
    let let_rest = Arc::new(Mutex::new(let_rest));
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let address = format!("http://{}/", listener.local_addr().expect("an address"));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let let_rest = Arc::clone(&let_rest);
            let tell = tell.clone();
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
                        let head = attachment("slow.bin", PART.len() * 2);
                        (stream.write_all(head.as_bytes()))
                            .and_then(|()| stream.write_all(PART))
                            .and_then(|()| stream.flush())
                            .and_then(|()| {
                                let _ = let_rest.lock().expect("the lock").recv();
                                stream.write_all(PART)
                            })
                    }
                    "/late" => {
                        let _ = tell.send(());
                        let _ = let_rest.lock().expect("the lock").recv();
                        stream
                            .write_all(attachment("late.txt", FILE.len()).as_bytes())
                            .and_then(|()| stream.write_all(FILE))
                    }
                    "/cut" if line.to_lowercase().contains("range: bytes=") => write!(
                        stream,
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\n\
                         ETag: \"cut\"\r\nContent-Range: bytes {half}-{last}/{whole}\r\n\
                         Content-Length: {half}\r\nConnection: close\r\n\r\n",
                        half = PART.len(),
                        last = PART.len() * 2 - 1,
                        whole = PART.len() * 2,
                    )
                    .and_then(|()| stream.write_all(PART)),
                    "/cut" => {
                        let head = attachment("cut.bin", PART.len() * 2);
                        let head = head.replace("\r\n\r\n", "\r\nAccept-Ranges: bytes\r\nETag: \"cut\"\r\n\r\n");
                        stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(PART))
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
    Server { address, rest, late }
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
    asked(session, pane)
}

/// Waits for a download of `pane`'s to ask where it goes: its number, and the name it was given.
fn asked(session: &mut Session, pane: u64) -> (u32, String) {
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

/// Waits for the download `id` to stand as `state` says, telling it how it went.
fn until_state(session: &mut Session, id: u32, state: impl Fn(&DownloadState) -> bool) -> koyo_browser_ipc::Download {
    let mut seen = None;
    session.wait("the download to get there", |event| match event {
        HostEvent::Download { download } if download.id == id && state(&download.state) => {
            seen = Some(download.clone());
            true
        }
        _ => false,
    });
    seen.expect("seen")
}

/// Waits for the download `id` to end, done, with `content` at `path`.
fn comes_in(session: &mut Session, id: u32, path: &Path, content: &[u8]) {
    let ended = until_state(session, id, |state| *state != DownloadState::Going);
    assert_eq!(ended.state, DownloadState::Done);
    assert_eq!(std::fs::read(path).expect("the file"), content);
}

/// The download `id` goes to `path`.
fn save(session: &mut Session, id: u32, path: &Path) {
    act(session, id, DownloadAct::SaveTo { path: path.to_string_lossy().into_owned() });
}

fn act(session: &mut Session, download: u32, act: DownloadAct) {
    let request = AppRequest::Download { download, act };
    session.host.send(&request).expect("the host hears it");
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_comes_in_where_the_app_says_and_one_canceled_saves_nothing() {
    let launch = launch("downloads");
    let mut session = Session::start(&launch);
    let server = serve();
    let saved = folder(&launch);
    open(&mut session, &server.address, "saves");
    let (id, name) = download(&mut session, 1, "file");
    assert_eq!(name, "report.txt", "the name the server gave it");
    let path = saved.join("report (1).txt");
    save(&mut session, id, &path);
    let done = until_state(&mut session, id, |state| *state != DownloadState::Going);
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
    let opened = AppRequest::Open { pane: 2, url: server.address, viewport, profile: "smoke".into() };
    session.host.send(&opened).expect("open");
    session.wait("the second page", |event| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title == "saves" && !state.loading)
    });
    let (id, _) = download(&mut session, 2, "file");
    act(&mut session, id, DownloadAct::Cancel);
    let canceled = until_state(&mut session, id, |state| *state != DownloadState::Going);
    assert_eq!(canceled.state, DownloadState::Canceled);
    let left: Vec<PathBuf> = (std::fs::read_dir(&saved).expect("the folder").flatten()).map(|entry| entry.path()).collect();
    assert_eq!(left, [path], "nothing more was saved");
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_the_server_drops_goes_on_where_it_was_going_without_asking_again() {
    let launch = launch("download-resumes");
    let mut session = Session::start(&launch);
    let server = serve();
    let path = folder(&launch).join("cut.bin");
    open(&mut session, &server.address, "saves");
    let (id, _) = download(&mut session, 1, "cut");
    save(&mut session, id, &path);
    let mut ended = None;
    session.wait("the download to end", |event| match event {
        HostEvent::DownloadAsked { .. } => panic!("the download asked again where it goes"),
        HostEvent::Download { download } if download.id == id && download.state != DownloadState::Going => {
            ended = Some(download.state.clone());
            true
        }
        _ => false,
    });
    assert_eq!(ended, Some(DownloadState::Done));
    assert_eq!(std::fs::read(&path).expect("the file").len(), PART.len() * 2);
    close(session, &launch);
}

/// A page whose download `slow.bin` comes in, its rest still to come.
struct Slow {
    launch: koyo_browser_ipc::client::Launch,
    session: Session,
    server: Server,
    path: PathBuf,
    id: u32,
}

fn slow(name: &str) -> Slow {
    let launch = launch(name);
    let mut session = Session::start(&launch);
    let server = serve();
    let path = folder(&launch).join("slow.bin");
    open(&mut session, &server.address, "saves");
    let (id, _) = download(&mut session, 1, "slow");
    save(&mut session, id, &path);
    until_state(&mut session, id, |state| *state == DownloadState::Going);
    Slow { launch, session, server, path, id }
}

impl Slow {
    /// Waits for its page to close, the download still coming in: whether the page closed
    /// without the app letting it go.
    fn page_closes(&mut self) -> bool {
        let mut tab_closed = false;
        let id = self.id;
        self.session.wait("the page to close", |event| match event {
            HostEvent::TabClosed { pane: 1 } => {
                tab_closed = true;
                false
            }
            HostEvent::Download { download } if download.id == id => {
                assert_eq!(download.state, DownloadState::Going, "the download ended before its page closed");
                false
            }
            HostEvent::Closed { pane: 1 } => true,
            _ => false,
        });
        tab_closed
    }

    /// The rest of it comes, and it's done.
    fn comes_in_whole(mut self) {
        self.server.rest.send(()).expect("the server");
        comes_in(&mut self.session, self.id, &self.path, &PART.repeat(2));
        close(self.session, &self.launch);
    }
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_closed_while_its_download_comes_in_goes_and_the_download_still_ends() {
    let mut slow = slow("download-outlives-close");
    slow.session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    assert!(!slow.page_closes(), "the app let it go");
    slow.comes_in_whole();
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_that_closes_itself_while_its_download_comes_in_leaves_it_to_finish() {
    let mut slow = slow("download-outlives-self-close");
    cdp(&mut slow.session, 950, "Runtime.evaluate", serde_json::json!({ "expression": "window.close(); true" }));
    assert!(slow.page_closes(), "the app is told its tab goes");
    slow.comes_in_whole();
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_outlives_its_profiles_data_and_is_not_taken_for_one_of_the_page_opened_again() {
    let mut slow = slow("download-outlives-clearing");
    slow.session.host.send(&AppRequest::ClearData).expect("clear");
    slow.page_closes();
    open(&mut slow.session, &slow.server.address, "saves");
    let (again, _) = download(&mut slow.session, 1, "file");
    assert_ne!(again, slow.id, "a download of its own, though its profile's first too");
    let path = slow.path.with_file_name("again.txt");
    save(&mut slow.session, again, &path);
    comes_in(&mut slow.session, again, &path, FILE);
    slow.comes_in_whole();
}

/// A page that asked for a download and went by `goes` before the server answered: its download
/// still asks where it goes, and is saved there.
fn answered_once_its_page_went(name: &str, goes: &AppRequest) {
    let launch = launch(name);
    let mut session = Session::start(&launch);
    let server = serve();
    open(&mut session, &server.address, "saves");
    let click = "document.getElementById('late').click(); true";
    cdp(&mut session, 960, "Runtime.evaluate", serde_json::json!({ "expression": click, "userGesture": true }));
    server.late.recv_timeout(Duration::from_secs(10)).expect("the server is asked");
    session.host.send(goes).expect("the page goes");
    session.wait("the page to close", |event| matches!(event, HostEvent::Closed { pane: 1 }));
    server.rest.send(()).expect("the server");
    is_saved(session, &launch, FILE);
}

/// Waits for pane 1's download to ask where it goes, and to end, saved there, as `content`.
fn is_saved(mut session: Session, launch: &koyo_browser_ipc::client::Launch, content: &[u8]) {
    let path = folder(launch).join("saved");
    let (id, _) = asked(&mut session, 1);
    save(&mut session, id, &path);
    comes_in(&mut session, id, &path, content);
    close(session, launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_asked_for_by_a_page_that_closed_before_the_server_answered_is_saved() {
    answered_once_its_page_went("download-answered-late", &AppRequest::Close { pane: 1 });
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_asked_for_is_saved_though_the_data_was_cleared_before_the_server_answered() {
    answered_once_its_page_went("download-answered-cleared", &AppRequest::ClearData);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_download_of_what_the_page_made_is_saved_though_the_page_closes_at_once() {
    let launch = launch("download-made");
    let mut session = Session::start(&launch);
    let server = serve();
    open(&mut session, &server.address, "saves");
    let made = "const made = document.createElement('a'); made.download = 'made.txt'; \
        made.href = URL.createObjectURL(new Blob(['made'])); made.click(); window.close(); true";
    cdp(&mut session, 960, "Runtime.evaluate", serde_json::json!({ "expression": made, "userGesture": true }));
    is_saved(session, &launch, b"made");
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn the_host_shutting_down_ends_a_download_whose_page_has_closed() {
    let mut slow = slow("download-shutdown");
    slow.session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    slow.page_closes();
    slow.session.host.send(&AppRequest::Shutdown).expect("shutdown");
    let ended = until_state(&mut slow.session, slow.id, |state| *state != DownloadState::Going);
    assert_eq!(ended.state, DownloadState::Canceled);
    slow.session.host.shutdown(Duration::from_secs(10));
    assert!(!slow.path.exists(), "what had come of it goes");
    let _ = std::fs::remove_dir_all(slow.launch.profiles.parent().expect("scratch"));
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn the_page_saves_as_a_pdf_and_prints_in_chromes_print_preview() {
    let launch = launch("print");
    let mut session = Session::start(&launch);
    let server = serve();
    let pdf = folder(&launch).join("saves.pdf");
    open(&mut session, &server.address, "saves");
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
