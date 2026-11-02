//! Stopping another build's daemon to start this one's (`docs/terminals.md` 5.1).

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use colosso_term_control::client::stop_daemon;
use colosso_term_control::{ClientKind, Hello, TermDir};

/// A daemon of an old build that never stops when asked: it holds the lock with its pid,
/// listens on the socket, hangs up on every hello it can't read, and ignores SIGTERM.
const DEAF_DAEMON: &str = r#"
use Fcntl qw(:flock);
use IO::Socket::UNIX;
my ($lock, $socket) = @ARGV;
$SIG{TERM} = "IGNORE";
open(my $held, "+>", $lock) or die "lock: $!";
flock($held, LOCK_EX) or die "flock: $!";
syswrite($held, "$$");
unlink $socket;
my $server = IO::Socket::UNIX->new(Type => SOCK_STREAM(), Local => $socket, Listen => 8)
    or die "listen: $!";
print "ready\n";
STDOUT->flush();
while (my $client = $server->accept()) { close($client); }
"#;

#[test]
fn a_daemon_that_ignores_sigterm_is_killed_so_this_builds_can_start() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stop-deaf");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("scratch");
    let dir = TermDir::at(&root);
    let mut deaf = Command::new("perl")
        .arg("-e")
        .arg(DEAF_DAEMON)
        .arg(dir.lock())
        .arg(dir.socket())
        .stdout(Stdio::piped())
        .spawn()
        .expect("perl");
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(deaf.stdout.take().expect("stdout")),
        &mut ready,
    )
    .expect("ready");
    assert!(UnixStream::connect(dir.socket()).is_ok(), "it listens");

    let start = Instant::now();
    let stopped = stop_daemon(&dir, &Hello::new(ClientKind::App, "test", "0"));
    let took = start.elapsed();
    let alive = deaf.try_wait().expect("its state").is_none();
    if alive {
        // Not stopped: it would outlive the test.
        let _ = deaf.kill();
    }
    let status = deaf.wait().expect("it ends");
    assert!(
        !alive,
        "still running after stop_daemon answered {stopped:?}"
    );

    assert!(stopped.is_ok(), "stopped: {stopped:?}");
    assert!(
        UnixStream::connect(dir.socket()).is_err(),
        "nothing listens"
    );
    assert!(!status.success(), "killed, not ended by itself: {status:?}");
    assert!(took < Duration::from_secs(12), "within the waits: {took:?}");
    let _ = std::fs::remove_dir_all(&root);
}
