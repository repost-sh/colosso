//! Installing a pinned release: fetched, checked against its digest, unpacked, and moved
//! into place by one rename, so a binary found in place is always whole.
//!
//! How it's fetched is the caller's: the app can bring its own HTTP client. [`Curl`] is the
//! one that needs nothing, as `curl` comes with macOS and nearly every Linux; the lockfile's
//! HTTP clients don't fit here, its `reqwest` needing a tokio runtime this crate doesn't run.
//! What's fetched is checked against a pinned SHA-256 either way.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::{Release, host_target, runs};
use crate::error::LspError;

/// Downloads a file over HTTPS.
pub trait Fetch {
    /// Writes what `url` holds to the new file `to`. Blocks.
    fn fetch(&self, url: &str, to: &Path) -> Result<(), String>;
}

/// Fetches with the system's `curl`, ignoring any `.curlrc`, over HTTPS only, retrying twice.
pub struct Curl;

impl Fetch for Curl {
    fn fetch(&self, url: &str, to: &Path) -> Result<(), String> {
        let mut curl = Command::new("curl");
        curl
            // First, so no `.curlrc` changes what's fetched.
            .arg("--disable")
            .args(["--fail", "--location", "--silent", "--show-error"])
            .args(["--proto", "=https", "--proto-redir", "=https"])
            .args([
                "--retry",
                "2",
                "--connect-timeout",
                "30",
                "--max-time",
                "900",
            ])
            .arg("--output")
            .arg(to)
            .arg(url);
        run(&mut curl, "curl")
    }
}

/// Runs `command` to its end, with nothing on its stdin; failed, what it said on stderr.
pub(super) fn run(command: &mut Command, program: &str) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("couldn't run {program}: {error}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(said.trim().to_owned());
    }
    Ok(())
}

pub(super) fn install(
    name: &str,
    release: &Release,
    data: &Path,
    fetch: &dyn Fetch,
) -> Result<PathBuf, LspError> {
    let folder = data.join("servers").join(name);
    let version = folder.join(release.version);
    let binary = version.join(name);
    if binary.is_file() {
        return Ok(binary);
    }
    let target = host_target()
        .ok_or_else(|| LspError::Install(format!("{name} has no download for this platform")))?;
    let sha256 = release
        .assets
        .iter()
        .find(|(asset, _)| *asset == target)
        .map(|(_, sha256)| *sha256)
        .ok_or_else(|| {
            LspError::Install(format!(
                "{name} {} has no download for {target}",
                release.version
            ))
        })?;
    let url = release
        .url
        .replace("{version}", release.version)
        .replace("{target}", target);

    fs::create_dir_all(&folder).map_err(io_failed("create", &folder))?;
    let staging = staging(&folder);
    log::info!("installing {name} {} from {url}", release.version);
    let installed = unpack(&staging, name, &url, sha256, fetch)
        .and_then(|()| place(&staging, &version, Path::new(name)));
    // Gone already after the rename; what a failure left behind otherwise.
    let _ = fs::remove_dir_all(&staging);
    installed?;
    Ok(binary)
}

/// Fetches, checks and unpacks the binary into `staging`, and makes sure it runs.
fn unpack(
    staging: &Path,
    name: &str,
    url: &str,
    sha256: &str,
    fetch: &dyn Fetch,
) -> Result<(), LspError> {
    fs::create_dir(staging).map_err(io_failed("create", staging))?;
    let archive = staging.join(format!("{name}.gz"));
    fetch
        .fetch(url, &archive)
        .map_err(|error| LspError::Install(format!("couldn't download {url}: {error}")))?;
    verify(&archive, sha256)?;
    let binary = staging.join(name);
    gunzip(&archive, &binary)?;
    fs::remove_file(&archive).map_err(io_failed("remove", &archive))?;
    make_executable(&binary)?;
    if !runs(&binary) {
        return Err(LspError::Install(format!(
            "the downloaded {name} doesn't run"
        )));
    }
    Ok(())
}

/// A folder of `folder`'s own for one install, named so two at once don't meet.
pub(super) fn staging(folder: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    folder.join(format!(".staging-{}-{nanos}", std::process::id()))
}

/// Moves the folder staged into place as `version` in one step; `proof`, a path under it,
/// is what says an install there is whole.
pub(super) fn place(staging: &Path, version: &Path, proof: &Path) -> Result<(), LspError> {
    match fs::rename(staging, version) {
        Ok(()) => Ok(()),
        // Another install got there first, and what it put there is the same.
        Err(_) if version.join(proof).is_file() => Ok(()),
        Err(error) => Err(LspError::Install(format!(
            "couldn't move {} into {}: {error}",
            proof.display(),
            version.display()
        ))),
    }
}

fn verify(archive: &Path, expected: &str) -> Result<(), LspError> {
    let mut file = File::open(archive).map_err(io_failed("open", archive))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(io_failed("read", archive))?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(LspError::Install(format!(
            "the download's SHA-256 is {actual}, not the pinned {expected}"
        )));
    }
    Ok(())
}

fn gunzip(archive: &Path, to: &Path) -> Result<(), LspError> {
    let file = File::open(archive).map_err(io_failed("open", archive))?;
    let mut decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut out = BufWriter::new(File::create_new(to).map_err(io_failed("create", to))?);
    io::copy(&mut decoder, &mut out).map_err(io_failed("unpack", archive))?;
    out.flush().map_err(io_failed("write", to))
}

#[cfg(unix)]
fn make_executable(binary: &Path) -> Result<(), LspError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(binary, fs::Permissions::from_mode(0o755))
        .map_err(io_failed("make executable", binary))
}

#[cfg(not(unix))]
fn make_executable(_binary: &Path) -> Result<(), LspError> {
    Ok(())
}

pub(super) fn io_failed(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> LspError {
    let path = path.display().to_string();
    move |error| LspError::Install(format!("couldn't {action} {path}: {error}"))
}
