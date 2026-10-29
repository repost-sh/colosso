//! What the fixtures are made of. Every generator is seeded, so a fixture is the same file on
//! every machine; the Rust ones are this repository's own sources, so the editor is measured
//! on code shaped like the code it will hold.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// `SplitMix64`: a few lines of arithmetic, which is all a fixture needs from randomness.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`, or 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// This repository's Rust sources, whole files in path order.
pub struct Corpus {
    files: Vec<Vec<String>>,
}

impl Corpus {
    /// The app's and the git crate's sources. The bench's own are left out, so changing the
    /// bench doesn't change what it measures.
    pub fn load(root: &Path) -> Result<Self> {
        let mut paths = Vec::new();
        for dir in ["crates/colosso/src", "crates/git/src"] {
            collect_rust(&root.join(dir), &mut paths)?;
        }
        paths.sort();
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            files.push(text.lines().map(str::to_owned).collect());
        }
        anyhow::ensure!(
            !files.is_empty(),
            "no Rust sources under {}",
            root.display()
        );
        Ok(Self { files })
    }

    /// `count` consecutive lines from somewhere in the corpus.
    fn excerpt(&self, rng: &mut Rng, count: usize) -> &[String] {
        let file = &self.files[rng.below(self.files.len())];
        let start = rng.below(file.len().saturating_sub(count));
        &file[start..(start + count).min(file.len())]
    }
}

fn collect_rust(dir: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust(&path, into)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
    Ok(())
}

/// `lines` lines of Rust: the corpus over and over, its files in a new order each pass, so
/// the file is real code all the way down and not one block repeated. The last file is cut
/// off where the count runs out.
pub fn rust(corpus: &Corpus, lines: usize, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(lines);
    let mut order: Vec<usize> = (0..corpus.files.len()).collect();
    while out.len() < lines {
        rng.shuffle(&mut order);
        for &file in &order {
            let room = lines - out.len();
            out.extend(corpus.files[file].iter().take(room).map(String::as_str));
            if out.len() == lines {
                break;
            }
        }
    }
    join_lines(out)
}

const NON_ASCII: [&str; 10] = [
    "日本語のコメント、幅の広い文字",
    "中文注释：测试宽字符的排版",
    "한국어 주석과 공백",
    "emoji 🚀✨🦀🔥",
    "Ünïcödé çàfé naïve façade",
    "混合 mixed テキスト 🎉 text",
    "👩‍💻 joined 👨‍👩‍👧‍👦 sequences",
    "Ελληνικά και κυριλλица",
    "全角ＡＢＣ１２３と半角",
    "✓ ✗ → ← ⚠ ★ ♥",
];

/// Rust like [`rust`], with emoji, CJK and other wide or combined characters sprinkled into
/// about one line in five, as comments and string literals.
pub fn unicode(corpus: &Corpus, lines: usize, seed: u64) -> String {
    let rust = rust(corpus, lines, seed);
    let mut rng = Rng::new(seed ^ 0x55);
    let mut out = String::with_capacity(rust.len() + rust.len() / 4);
    for line in rust.lines() {
        out.push_str(line);
        if rng.chance(20) {
            let text = rng.pick(&NON_ASCII);
            if line.trim().is_empty() {
                let _ = write!(out, "let label = \"{text}\";");
            } else {
                let _ = write!(out, " // {text}");
            }
        }
        out.push('\n');
    }
    out
}

const WORDS: [&str; 16] = [
    "alpha",
    "branch",
    "commit",
    "delta",
    "editor",
    "frame",
    "glyph",
    "highlight",
    "index",
    "journal",
    "kernel",
    "layout",
    "merge",
    "notify",
    "offset",
    "paint",
];

/// One line of minified JSON at most `max_bytes` long: an array of records with nested
/// objects, arrays, numbers, strings, booleans and nulls.
pub fn json(max_bytes: usize, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut out = String::from("[");
    let mut id = 0usize;
    loop {
        let record = json_record(&mut rng, id);
        // The comma before it, and the closing bracket after the last one.
        let separator = usize::from(id > 0);
        if out.len() + separator + record.len() + 1 > max_bytes {
            break;
        }
        if id > 0 {
            out.push(',');
        }
        out.push_str(&record);
        id += 1;
    }
    out.push(']');
    out
}

fn json_record(rng: &mut Rng, id: usize) -> String {
    let name = format!("{}-{}", rng.pick(&WORDS), rng.pick(&WORDS));
    let tags: Vec<String> = (0..=rng.below(4))
        .map(|_| format!("\"{}\"", rng.pick(&WORDS)))
        .collect();
    let weights: Vec<String> = (0..rng.below(6))
        .map(|_| rng.below(1000).to_string())
        .collect();
    let score = rng.below(100_000) as f64 / 1000.0;
    let owner = if rng.chance(30) {
        "null".to_owned()
    } else {
        format!("\"{}\"", rng.pick(&WORDS))
    };
    format!(
        "{{\"id\":{id},\"name\":\"{name}\",\"active\":{},\"score\":{score},\"tags\":[{}],\
         \"meta\":{{\"owner\":{owner},\"created\":\"2026-{:02}-{:02}T{:02}:{:02}:00Z\",\"weights\":[{}]}}}}",
        rng.chance(50),
        tags.join(","),
        1 + rng.below(12),
        1 + rng.below(28),
        rng.below(24),
        rng.below(60),
        weights.join(","),
    )
}

/// `lines` lines of Markdown that is mostly fenced code: headings, short paragraphs with
/// inline code, emphasis and links, lists, and code blocks in Rust (from the corpus) and JSON,
/// each block an injection the highlighter parses with a grammar of its own.
pub fn markdown(corpus: &Corpus, lines: usize, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut out: Vec<String> = Vec::with_capacity(lines + 64);
    let mut section = 0;
    while out.len() < lines {
        section += 1;
        out.push(format!("## {} {section}", capitalized(rng.pick(&WORDS))));
        out.push(String::new());
        for _ in 0..=rng.below(3) {
            out.push(paragraph_line(&mut rng));
        }
        out.push(String::new());
        if rng.chance(40) {
            for _ in 0..2 + rng.below(4) {
                out.push(format!("- {}", paragraph_line(&mut rng)));
            }
            out.push(String::new());
        }
        for _ in 0..=rng.below(2) {
            if rng.chance(75) {
                out.push("```rust".to_owned());
                let count = 5 + rng.below(36);
                out.extend(corpus.excerpt(&mut rng, count).iter().cloned());
            } else {
                out.push("```json".to_owned());
                out.extend(pretty_json(&mut rng));
            }
            out.push("```".to_owned());
            out.push(String::new());
        }
    }
    out.truncate(lines);
    join_lines(out.iter().map(String::as_str))
}

fn paragraph_line(rng: &mut Rng) -> String {
    let a = rng.pick(&WORDS);
    let b = rng.pick(&WORDS);
    let c = rng.pick(&WORDS);
    format!(
        "The `{a}_{b}` step keeps **{c}** in view; see [{a}](https://example.com/{b}) and \
         _{b}_ before calling `{c}()`."
    )
}

fn pretty_json(rng: &mut Rng) -> Vec<String> {
    let mut lines = vec!["{".to_owned()];
    let count = 2 + rng.below(8);
    for i in 0..count {
        let comma = if i + 1 < count { "," } else { "" };
        lines.push(format!(
            "  \"{}\": {{ \"value\": {}, \"label\": \"{}\" }}{comma}",
            rng.pick(&WORDS),
            rng.below(10_000),
            rng.pick(&WORDS),
        ));
    }
    lines.push("}".to_owned());
    lines
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn join_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}
