//! HTTPS as the bucket serves it: [`Curl`] in the app, a folder of files in the tests.

use std::io::{self, Read as _, Write};
use std::process::{Command, Stdio};

use crate::blocking::Blocking;

pub(super) trait Fetch: Send + Sync {
    /// What `url` holds, at most `most` bytes. Blocks.
    fn read(&self, url: &str, most: u64, may: Blocking<'_>) -> Result<Vec<u8>, String>;

    /// Streams what `url` holds into `into`, at most `most` bytes. Blocks until it has all
    /// come, the server stalls, or `into` refuses a write.
    fn stream(
        &self,
        url: &str,
        most: u64,
        into: &mut dyn Write,
        may: Blocking<'_>,
    ) -> Result<(), String>;
}

/// The system's curl, ignoring any `.curlrc`, over HTTPS alone, redirects included. The feed
/// is read within half a minute. An image has no total cap, since 200 MB on a slow line
/// takes longer than any cap that suits the feed: it's given up once under 1 KiB a second
/// has come for a minute. What comes passes through Koyo, so it's counted and not polled,
/// and a Koyo that ends takes its curl with it, as curl has nowhere left to write.
pub(super) struct Curl;

const READ: [&str; 2] = ["--max-time", "30"];
const STREAM: [&str; 4] = ["--speed-limit", "1024", "--speed-time", "60"];

impl Fetch for Curl {
    fn read(&self, url: &str, most: u64, may: Blocking<'_>) -> Result<Vec<u8>, String> {
        let mut held = Vec::new();
        curl(&READ, url, most, &mut held, may)?;
        Ok(held)
    }

    fn stream(
        &self,
        url: &str,
        most: u64,
        into: &mut dyn Write,
        may: Blocking<'_>,
    ) -> Result<(), String> {
        curl(&STREAM, url, most, into, may)
    }
}

/// Runs curl on `url` under `policy`, its output copied into `into` up to `most` bytes: what
/// curl said if it failed, and why `into` refused if it did, which ends curl.
fn curl(
    policy: &[&str],
    url: &str,
    most: u64,
    into: &mut dyn Write,
    _: Blocking<'_>,
) -> Result<(), String> {
    let mut curl = Command::new("/usr/bin/curl")
        .arg("--disable")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--connect-timeout", "30"])
        .args(["--max-filesize", &most.to_string()])
        .args(policy)
        .args(["--output", "-", "--url", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("couldn't run curl: {error}"))?;
    let copied = match curl.stdout.take() {
        Some(mut came) => io::copy(&mut came, &mut AtMost { into, left: most }).map(drop),
        None => Err(io::Error::other("curl's output wasn't captured")),
    };
    if let Err(refused) = copied {
        let _ = curl.kill();
        let _ = curl.wait();
        return Err(refused.to_string());
    }
    let mut said = String::new();
    if let Some(mut errors) = curl.stderr.take() {
        let _ = errors.read_to_string(&mut said);
    }
    match curl.wait() {
        Ok(ended) if ended.success() => Ok(()),
        Ok(_) => Err(said.trim().to_owned()),
        Err(error) => Err(format!("curl couldn't be waited for: {error}")),
    }
}

/// A writer that takes `left` more bytes and refuses what comes after: a server can't send
/// more than was asked for, whatever it says of its size.
pub(super) struct AtMost<'a> {
    pub(super) into: &'a mut dyn Write,
    pub(super) left: u64,
}

impl Write for AtMost<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(left) = self.left.checked_sub(bytes.len() as u64) else {
            return Err(io::Error::other("more came than the release weighs"));
        };
        self.into.write_all(bytes)?;
        self.left = left;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.into.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::AtMost;

    #[test]
    fn a_writer_takes_what_was_asked_for_and_refuses_the_rest() {
        let mut held = Vec::new();
        let mut writer = AtMost {
            into: &mut held,
            left: 5,
        };
        writer.write_all(b"abc").unwrap();
        writer.write_all(b"de").unwrap();
        assert!(writer.write_all(b"f").is_err());
        assert_eq!(held, b"abcde");
    }
}
