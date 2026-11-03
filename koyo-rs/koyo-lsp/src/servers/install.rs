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

use flate2::bufread::GzDecoder;
use sha2::{Digest, Sha256};

use super::release::{Archive, Release};
use super::runs;
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
    let binary = version.join(release.binary);
    if binary.is_file() {
        return Ok(binary);
    }
    let (url, sha256) = release.download().ok_or_else(|| {
        LspError::Install(format!(
            "{name} {} has no download for this platform",
            release.version
        ))
    })?;

    fs::create_dir_all(&folder).map_err(io_failed("create", &folder))?;
    let staging = staging(&folder);
    // Beside the staging folder, so that it holds only what was unpacked.
    let download = staging.with_extension("download");
    log::info!("installing {name} {} from {url}", release.version);
    let installed = unpack(&staging, &download, name, &url, sha256, release, fetch)
        .and_then(|()| place(&staging, &version, Path::new(release.binary)));
    // Gone already after the rename; what a failure left behind otherwise.
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&download);
    installed?;
    Ok(binary)
}

/// Fetches `url` to `download`, checks it and unpacks it into `staging`, and makes sure the
/// release's binary in it runs.
fn unpack(
    staging: &Path,
    download: &Path,
    name: &str,
    url: &str,
    sha256: &str,
    release: &Release,
    fetch: &dyn Fetch,
) -> Result<(), LspError> {
    fs::create_dir(staging).map_err(io_failed("create", staging))?;
    fetch
        .fetch(url, download)
        .map_err(|error| LspError::Install(format!("couldn't download {url}: {error}")))?;
    verify(download, sha256)?;
    let binary = staging.join(release.binary);
    match release.archive {
        Archive::Gzip => gunzip(download, &binary)?,
        Archive::Tarball => untar(download, staging)?,
    }
    if !binary.is_file() {
        return Err(LspError::Install(format!(
            "the downloaded {name} has no {}",
            release.binary
        )));
    }
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

/// Unpacks the gzipped tar `archive` whole into `into`. An entry whose path would leave
/// `into` is refused by the unpacking itself.
fn untar(archive: &Path, into: &Path) -> Result<(), LspError> {
    tar::Archive::new(gunzipping(archive)?)
        .unpack(into)
        .map_err(io_failed("unpack", archive))
}

fn gunzip(archive: &Path, to: &Path) -> Result<(), LspError> {
    let mut decoder = gunzipping(archive)?;
    let mut out = BufWriter::new(File::create_new(to).map_err(io_failed("create", to))?);
    io::copy(&mut decoder, &mut out).map_err(io_failed("unpack", archive))?;
    out.flush().map_err(io_failed("write", to))
}

/// Reads the gzipped `archive`, unpacked as it's read.
fn gunzipping(archive: &Path) -> Result<GzDecoder<BufReader<File>>, LspError> {
    let file = File::open(archive).map_err(io_failed("open", archive))?;
    Ok(GzDecoder::new(BufReader::new(file)))
}

#[cfg(unix)]
pub(super) fn make_executable(binary: &Path) -> Result<(), LspError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(binary, fs::Permissions::from_mode(0o755))
        .map_err(io_failed("make executable", binary))
}

#[cfg(not(unix))]
pub(super) fn make_executable(_binary: &Path) -> Result<(), LspError> {
    Ok(())
}

pub(super) fn io_failed(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> LspError {
    let path = path.display().to_string();
    move |error| LspError::Install(format!("couldn't {action} {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::super::release::host;
    use super::super::tests::{Canned, scratch};
    use super::*;

    const SCRIPT: &str = "#!/bin/sh\nexit 0\n";

    /// A gzipped tar of `files`, none of them executable.
    fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gzip);
        for (path, text) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o644);
            tar.append_data(&mut header, path, text.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn gzip(text: &str) -> Vec<u8> {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(text.as_bytes()).unwrap();
        gzip.finish().unwrap()
    }

    /// A release whose download for this machine is `bytes`, with its binary at `binary`.
    fn release(bytes: &[u8], archive: Archive, binary: &'static str) -> Release {
        let sha256 = format!("{:x}", Sha256::digest(bytes)).leak();
        Release {
            version: "1.0",
            url: "https://example.test/{version}/{target}",
            assets: vec![(host(), "test", &*sha256)].leak(),
            archive,
            binary,
        }
    }

    #[test]
    fn a_tarball_is_unpacked_whole_and_its_binary_made_to_run() {
        let data = scratch("tarball");
        let bytes = tarball(&[
            ("package/lib/tsc", SCRIPT),
            ("package/lib/lib.d.ts", "declare var x: number;\n"),
            ("package/package.json", "{}\n"),
        ]);
        let fetch = Canned::new(&bytes);
        let release = release(&bytes, Archive::Tarball, "package/lib/tsc");
        let binary = install("ts", &release, &data, &fetch).unwrap();
        assert_eq!(binary, data.join("servers/ts/1.0/package/lib/tsc"));
        assert!(runs(&binary));
        assert!(data.join("servers/ts/1.0/package/lib/lib.d.ts").is_file());
        // Nothing of the install is left beside it: neither the staging folder nor the download.
        let left: Vec<_> = fs::read_dir(data.join("servers/ts")).unwrap().collect();
        assert_eq!(left.len(), 1);
        // There already: not fetched again.
        assert_eq!(install("ts", &release, &data, &fetch).unwrap(), binary);
        assert_eq!(fetch.fetches.get(), 1);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_gzipped_binary_is_unpacked_under_the_server_s_name() {
        let data = scratch("gzip");
        let bytes = gzip(SCRIPT);
        let release = release(&bytes, Archive::Gzip, "tool");
        let binary = install("tool", &release, &data, &Canned::new(&bytes)).unwrap();
        assert_eq!(binary, data.join("servers/tool/1.0/tool"));
        assert!(runs(&binary));
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_download_with_another_digest_is_refused_and_leaves_nothing() {
        let data = scratch("digest");
        let bytes = tarball(&[("package/lib/tsc", SCRIPT)]);
        let release = release(&bytes, Archive::Tarball, "package/lib/tsc");
        let failed = install("ts", &release, &data, &Canned::new(b"something else"));
        assert!(matches!(failed, Err(LspError::Install(why)) if why.contains("SHA-256")));
        assert!(fs::read_dir(data.join("servers/ts")).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_tarball_without_the_binary_is_refused() {
        let data = scratch("no-binary");
        let bytes = tarball(&[("package/README.md", "nothing here\n")]);
        let release = release(&bytes, Archive::Tarball, "package/lib/tsc");
        let failed = install("ts", &release, &data, &Canned::new(&bytes));
        assert!(matches!(failed, Err(LspError::Install(why)) if why.contains("has no package/lib/tsc")));
        assert!(fs::read_dir(data.join("servers/ts")).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_platform_without_a_download_is_said() {
        let data = scratch("platform");
        let release = Release {
            assets: &[],
            ..release(b"", Archive::Gzip, "tool")
        };
        let failed = install("tool", &release, &data, &Canned::new(b""));
        assert!(matches!(failed, Err(LspError::Install(why)) if why == "tool 1.0 has no download for this platform"));
        let _ = fs::remove_dir_all(&data);
    }
}
