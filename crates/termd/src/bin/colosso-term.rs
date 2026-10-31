//! The command line for the terminal daemon, for testing it without the app:
//!
//! ```text
//! colosso-term list [--worktree <selector>]
//! colosso-term open <name> [--worktree <selector>] [--cwd <path>] [--respawn] [--no-attach] [-- <program> <args>...]
//! colosso-term attach <handle>        Ctrl-] detaches; the terminal lives on
//! colosso-term close <handle>
//! colosso-term shutdown
//! ```
//!
//! The daemon is started when none runs, from `COLOSSO_TERMD` or beside this binary.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use colosso_term_control::{
    AttachMode, Auth, ClientInfo, ClientKind, Connection, DaemonCommand, EventKind, Handle, Hello,
    IfExists, Message, OpenRequest, PROTOCOL, RequestBody, ResponseBody, Selector, Size, TermDir,
};
use colosso_term_engine::{Engine, TextFormat};
use crossbeam_channel::Receiver;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "colosso-term: {error}");
            ExitCode::FAILURE
        }
    }
}

type Failure = Box<dyn std::error::Error>;

fn run(args: &[String]) -> Result<(), Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Err("usage: colosso-term <list|open|attach|close|shutdown> ...".into());
    };
    let dir = TermDir::from_env().ok_or("no data folder: set COLOSSO_DATA_DIR")?;
    let (inbox, received) = colosso_term_control::client::inbox();
    let (connection, _welcome) = Connection::connect_or_start(
        &dir,
        &DaemonCommand::binary(daemon_binary()?),
        &hello(),
        inbox,
    )?;
    match command.as_str() {
        "list" => list(&connection, rest),
        "open" => open(&connection, &received, rest),
        "attach" => {
            let handle = rest.first().ok_or("usage: colosso-term attach <handle>")?;
            attach(&connection, &received, &Handle(handle.clone()))
        }
        "close" => {
            let handle = rest.first().ok_or("usage: colosso-term close <handle>")?;
            connection.request(RequestBody::Close {
                handle: Handle(handle.clone()),
                force: true,
            })?;
            Ok(())
        }
        "shutdown" => {
            connection.request(RequestBody::Shutdown)?;
            Ok(())
        }
        other => Err(format!("unknown command {other}").into()),
    }
}

fn hello() -> Hello {
    Hello {
        protocol: PROTOCOL,
        client: ClientInfo {
            kind: ClientKind::Cli,
            name: "colosso-term".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            session: None,
        },
        auth: Auth::None,
        // The terminal this runs in answers the program's queries itself.
        capabilities: vec!["answers_queries".into()],
        cwd: std::env::current_dir().ok(),
    }
}

fn daemon_binary() -> Result<PathBuf, Failure> {
    if let Some(path) = std::env::var_os("COLOSSO_TERMD") {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe()?;
    let beside = exe.with_file_name("colosso-termd");
    if beside.exists() {
        Ok(beside)
    } else {
        Err("no colosso-termd beside this binary; set COLOSSO_TERMD".into())
    }
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|i| args.get(i + 1))
}

fn list(connection: &Connection, args: &[String]) -> Result<(), Failure> {
    let worktree = option(args, "--worktree").map(|text| Selector::parse(text));
    let ResponseBody::Terminals { terminals } = connection.request(RequestBody::ListTerminals {
        worktree,
        status: None,
    })?
    else {
        return Err("unexpected answer".into());
    };
    let mut out = std::io::stdout().lock();
    for summary in terminals {
        let record = summary.record;
        let place = record.worktree.as_ref().map_or_else(
            || record.cwd.display().to_string(),
            colosso_term_control::WorktreeRef::display_name,
        );
        writeln!(
            out,
            "{}  {:<16} {:<20} {:?} {}  {}",
            record.handle,
            record.name,
            place,
            record.status,
            record
                .pid
                .map_or_else(|| "-".to_owned(), |pid| pid.to_string()),
            summary.preview
        )?;
    }
    Ok(())
}

fn open(
    connection: &Arc<Connection>,
    received: &Receiver<Message>,
    args: &[String],
) -> Result<(), Failure> {
    let name = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .ok_or("usage: colosso-term open <name> [options] [-- program args]")?;
    let (options, command) = match args.iter().position(|arg| arg == "--") {
        Some(dash) => (&args[1..dash], args[dash + 1..].to_vec()),
        None => (&args[1..], Vec::new()),
    };
    let request = OpenRequest {
        worktree: option(options, "--worktree").map(|text| Selector::parse(text)),
        name: name.clone(),
        command,
        cwd: option(options, "--cwd").map(PathBuf::from),
        env: BTreeMap::new(),
        size: Some(window_size().unwrap_or_default()),
        labels: BTreeSet::new(),
        if_exists: IfExists::Attach,
        respawn: options.iter().any(|arg| arg == "--respawn"),
    };
    let ResponseBody::Opened { terminal, existed } =
        connection.request(RequestBody::Open(request))?
    else {
        return Err("unexpected answer".into());
    };
    let _ = writeln!(
        std::io::stderr(),
        "{} {} ({})",
        if existed { "found" } else { "opened" },
        terminal.handle,
        terminal.name
    );
    if options.iter().any(|arg| arg == "--no-attach") {
        return Ok(());
    }
    attach(connection, received, &terminal.handle)
}

/// Mirrors the terminal here, raw, until Ctrl-] or the program exits.
fn attach(
    connection: &Arc<Connection>,
    received: &Receiver<Message>,
    handle: &Handle,
) -> Result<(), Failure> {
    let size = window_size();
    let ResponseBody::Attached { index, .. } = connection.request(RequestBody::Attach {
        handle: handle.clone(),
        size,
        mode: AttachMode::Control,
        since_offset: None,
    })?
    else {
        return Err("unexpected answer".into());
    };
    let _raw = RawMode::enable()?;
    let (detach_tx, detach_rx) = crossbeam_channel::bounded::<()>(1);
    let stdin_connection = Arc::clone(connection);
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 1024];
        loop {
            let n = match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if buf[..n].contains(&0x1d) {
                break;
            }
            if stdin_connection.input(index, &buf[..n]).is_err() {
                break;
            }
        }
        let _ = detach_tx.send(());
    });
    let mut stdout = std::io::stdout().lock();
    let mut snapshot: Vec<u8> = Vec::new();
    let mut last_size = size;
    loop {
        crossbeam_channel::select! {
            recv(received) -> message => match message {
                Ok(Message::Output(output)) if output.index == index => {
                    stdout.write_all(&output.bytes)?;
                    stdout.flush()?;
                }
                Ok(Message::Snapshot(chunk)) if chunk.index == index => {
                    snapshot.extend_from_slice(&chunk.bytes);
                    if chunk.chunk + 1 == chunk.of {
                        let text = Engine::restore(&snapshot).and_then(|engine| engine.screen_text(TextFormat::Vt));
                        snapshot.clear();
                        match text {
                            Ok(text) => {
                                write!(stdout, "\x1b[2J\x1b[H{}", text.replace('\n', "\r\n"))?;
                                stdout.flush()?;
                            }
                            Err(error) => log::warn!("couldn't restore the snapshot: {error}"),
                        }
                    }
                }
                Ok(Message::Event(event)) if event.handle == *handle => match event.kind {
                    EventKind::Exited { exit } => {
                        write!(stdout, "\r\n[exited {:?}]\r\n", exit.code)?;
                        break;
                    }
                    EventKind::Closed => {
                        write!(stdout, "\r\n[closed]\r\n")?;
                        break;
                    }
                    EventKind::Refused { error } => write!(stdout, "\r\n[refused: {}]\r\n", error.message)?,
                    _ => {}
                },
                Ok(_) => {}
                Err(_) => break,
            },
            recv(detach_rx) -> _ => break,
            default(Duration::from_millis(250)) => {
                let now = window_size();
                if now != last_size && let Some(size) = now {
                    last_size = now;
                    let _ = connection.request(RequestBody::Resize { handle: handle.clone(), size });
                }
            }
        }
    }
    let _ = connection.request(RequestBody::Detach {
        handle: handle.clone(),
    });
    Ok(())
}

fn window_size() -> Option<Size> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCGWINSZ fills the winsize struct given and touches nothing else.
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &raw mut ws) };
    (result == 0 && ws.ws_col > 0 && ws.ws_row > 0).then(|| Size {
        cols: ws.ws_col,
        rows: ws.ws_row,
        cell_width_px: u32::from(ws.ws_xpixel) / u32::from(ws.ws_col),
        cell_height_px: u32::from(ws.ws_ypixel) / u32::from(ws.ws_row),
    })
}

/// Stdin in raw mode, put back on drop.
struct RawMode {
    saved: libc::termios,
}

impl RawMode {
    fn enable() -> std::io::Result<RawMode> {
        // SAFETY: a zeroed termios is a valid value for tcgetattr to fill.
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: tcgetattr and tcsetattr read and write the termios given, nothing else.
        unsafe {
            if libc::tcgetattr(libc::STDIN_FILENO, &raw mut saved) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut raw = saved;
            libc::cfmakeraw(&raw mut raw);
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const raw) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(RawMode { saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restores the termios read in `enable`.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const self.saved) };
    }
}
