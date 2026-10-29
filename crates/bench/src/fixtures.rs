//! The files the editor is measured on, each a shape that stresses a different part of it.
//! They're generated once and kept: a later run measures the same bytes, and a changed file
//! shows in the hash the results record.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::generate::{self, Corpus};
use crate::repo;

/// Where the bench keeps what it makes, relative to the checkout; both are gitignored.
pub const FIXTURES_DIR: &str = ".superpowers/fixtures";
pub const RESULTS_DIR: &str = ".superpowers/bench";

/// How a fixture is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// A file, of this text.
    Text(Text),
    /// A git repository of about `tracked` files in nested folders, and `ignored` more in
    /// `node_modules/` and `target/`: what the scan scenarios read.
    Repo { tracked: usize, ignored: usize },
    /// A tree of folders held in memory, made afresh by each run of the rows scenarios.
    Tree,
}

/// How a fixture's text is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// This repository's Rust, repeated to `lines` lines.
    Rust { lines: usize },
    /// The same, with emoji, CJK and joined sequences sprinkled in.
    Unicode { lines: usize },
    /// Short prose around fenced code blocks, each block an injected grammar.
    Markdown { lines: usize },
    /// One line of minified JSON, at most `bytes` long.
    Json { bytes: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Fixture {
    pub name: &'static str,
    file: &'static str,
    /// The highlighter's name for the language.
    pub language: &'static str,
    source: Source,
    /// One seed per fixture, so adding a fixture leaves the others as they were.
    seed: u64,
    /// The whole file is one line: scrolling through it is sideways, and only opening,
    /// scrolling and typing are measured on it.
    pub single_line: bool,
    /// Whether the search scenarios run on it.
    pub searched: bool,
}

/// Every fixture, in the order the results list them. The two under 256 KB are where the
/// highlighter parses on the main thread at every keystroke and scans the whole tree for
/// injections; every larger file is parsed in the background. Then a repository, for the
/// scan scenarios only, and a tree held in memory, for the rows scenarios only.
pub const FIXTURES: [Fixture; 10] = [
    fixture(
        "rust-4k",
        "rust-4k.rs",
        "rust",
        Source::Text(Text::Rust { lines: 4_000 }),
        4,
        true,
    ),
    fixture(
        "rust-10k",
        "rust-10k.rs",
        "rust",
        Source::Text(Text::Rust { lines: 10_000 }),
        10,
        true,
    ),
    fixture(
        "rust-50k",
        "rust-50k.rs",
        "rust",
        Source::Text(Text::Rust { lines: 50_000 }),
        50,
        true,
    ),
    fixture(
        "rust-200k",
        "rust-200k.rs",
        "rust",
        Source::Text(Text::Rust { lines: 200_000 }),
        200,
        true,
    ),
    fixture(
        "markdown-5k",
        "markdown-5k.md",
        "markdown",
        Source::Text(Text::Markdown { lines: 5_000 }),
        5,
        true,
    ),
    fixture(
        "markdown-20k",
        "markdown-20k.md",
        "markdown",
        Source::Text(Text::Markdown { lines: 20_000 }),
        20,
        false,
    ),
    fixture(
        "unicode-50k",
        "unicode-50k.rs",
        "rust",
        Source::Text(Text::Unicode { lines: 50_000 }),
        51,
        false,
    ),
    // A mebibyte on one line: the editor's long-line path.
    Fixture {
        single_line: true,
        ..fixture(
            "json-1mb",
            "minified-1mb.json",
            "json",
            Source::Text(Text::Json { bytes: 1 << 20 }),
            1,
            false,
        )
    },
    fixture(
        "repo-300k",
        "repo-300k",
        "-",
        Source::Repo {
            tracked: 150_000,
            ignored: 150_000,
        },
        300,
        false,
    ),
    fixture("tree-500k", "-", "-", Source::Tree, 500, false),
];

const fn fixture(
    name: &'static str,
    file: &'static str,
    language: &'static str,
    source: Source,
    seed: u64,
    searched: bool,
) -> Fixture {
    Fixture {
        name,
        file,
        language,
        source,
        seed,
        single_line: false,
        searched,
    }
}

impl Fixture {
    pub fn path(&self) -> PathBuf {
        in_checkout(FIXTURES_DIR).join(self.file)
    }

    /// Whether it's the repository the scan scenarios read, rather than a file to edit.
    pub fn is_repo(&self) -> bool {
        matches!(self.source, Source::Repo { .. })
    }

    /// Whether it's the tree the rows scenarios work, which is never on disk.
    pub fn is_tree(&self) -> bool {
        self.source == Source::Tree
    }

    pub fn read(&self) -> Result<String> {
        let path = self.path();
        fs::read_to_string(&path).with_context(|| {
            format!(
                "reading {} (run `colosso-bench fixtures` first)",
                path.display()
            )
        })
    }

    /// The file's text.
    fn generate(&self, text: Text, corpus: &Corpus) -> String {
        match text {
            Text::Rust { lines } => generate::rust(corpus, lines, self.seed),
            Text::Unicode { lines } => generate::unicode(corpus, lines, self.seed),
            Text::Markdown { lines } => generate::markdown(corpus, lines, self.seed),
            Text::Json { bytes } => generate::json(bytes, self.seed),
        }
    }

    fn describe(&self, text: &str) -> FixtureInfo {
        FixtureInfo {
            name: self.name,
            file: self.path().display().to_string(),
            language: self.language,
            bytes: text.len(),
            lines: text.lines().count(),
            longest_line_bytes: text.lines().map(str::len).max().unwrap_or(0),
            files: 1,
            hash: format!("{:016x}", fnv1a(text.as_bytes())),
        }
    }

    fn describe_tree(&self) -> FixtureInfo {
        FixtureInfo {
            name: self.name,
            file: "(in memory)".to_owned(),
            language: self.language,
            bytes: 0,
            lines: 0,
            longest_line_bytes: 0,
            files: crate::rows::TREE_FILES,
            hash: "-".to_owned(),
        }
    }

    /// Makes the repository unless it's there, and describes it.
    fn ensure_repo(&self, tracked: usize, ignored: usize) -> Result<FixtureInfo> {
        let made = repo::ensure(&self.path(), tracked, ignored, self.seed)?;
        Ok(FixtureInfo {
            name: self.name,
            file: self.path().display().to_string(),
            language: self.language,
            bytes: 0,
            lines: 0,
            longest_line_bytes: 0,
            files: made.tracked + made.ignored,
            hash: made.hash,
        })
    }
}

/// What a results file says about a fixture, so two runs can tell whether they measured the
/// same bytes.
#[derive(Clone, Debug, Serialize)]
pub struct FixtureInfo {
    pub name: &'static str,
    pub file: String,
    pub language: &'static str,
    pub bytes: usize,
    pub lines: usize,
    pub longest_line_bytes: usize,
    /// One for a file; every file in a repository, the ignored ones included.
    pub files: usize,
    /// FNV-1a of the contents, or of a repository's paths.
    pub hash: String,
}

/// The checkout this crate is in.
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or(manifest)
        .to_path_buf()
}

pub fn in_checkout(relative: &str) -> PathBuf {
    workspace_root().join(relative)
}

/// Writes every fixture that isn't there yet, and describes them all.
pub fn ensure() -> Result<Vec<FixtureInfo>> {
    let dir = in_checkout(FIXTURES_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let missing = (FIXTURES.iter())
        .any(|fixture| matches!(fixture.source, Source::Text(_)) && !fixture.path().exists());
    let corpus = if missing {
        Some(Corpus::load(&workspace_root())?)
    } else {
        None
    };
    FIXTURES
        .iter()
        .map(|fixture| {
            let text = match fixture.source {
                Source::Repo { tracked, ignored } => return fixture.ensure_repo(tracked, ignored),
                Source::Tree => return Ok(fixture.describe_tree()),
                Source::Text(text) => text,
            };
            let path = fixture.path();
            let generated = match &corpus {
                Some(corpus) if !path.exists() => Some(fixture.generate(text, corpus)),
                _ => None,
            };
            let text = match generated {
                Some(text) => {
                    write_atomically(&path, &text)?;
                    text
                }
                None => fixture.read()?,
            };
            Ok(fixture.describe(&text))
        })
        .collect()
}

/// Through a temporary file, so a run cut short never leaves half a fixture to be measured.
pub fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let partial = path.with_extension("partial");
    fs::write(&partial, contents).with_context(|| format!("writing {}", partial.display()))?;
    fs::rename(&partial, path).with_context(|| format!("moving {}", path.display()))
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
