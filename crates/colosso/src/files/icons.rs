//! The icon beside each of the file tree's rows: a folder open or closed, a link, or a
//! file by what its name says it holds.

use colosso_project::{Entry, Kind};
use gpui_kit::assets::IconName;

pub fn icon(entry: &Entry, expanded: bool) -> IconName {
    match entry.kind {
        Kind::Dir if expanded => IconName::FolderOpen,
        Kind::Dir => IconName::Folder,
        Kind::Symlink { to_dir: true } => IconName::FolderSymlink,
        Kind::Symlink { to_dir: false } => IconName::FileSymlink,
        Kind::File => file(&entry.name),
    }
}

fn file(name: &str) -> IconName {
    match name {
        "Cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "Gemfile.lock"
        | "poetry.lock" | "bun.lockb" | "flake.lock" => return IconName::FileLock,
        "Dockerfile" | "Containerfile" => return IconName::Container,
        "Makefile" | "makefile" | "GNUmakefile" | "Justfile" | "justfile" => {
            return IconName::FileTerminal;
        }
        "LICENSE" | "LICENCE" | "COPYING" => return IconName::FileBadge,
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".editorconfig" | ".env" => {
            return IconName::FileCog;
        }
        _ => {}
    }
    // Lowercased on the stack: this runs for every row drawn. None of the extensions known
    // is longer than the buffer.
    let mut buffer = [0; 8];
    let extension = (name.rsplit_once('.'))
        .and_then(|(_, extension)| buffer.get_mut(..extension.len()).map(|to| (to, extension)))
        .and_then(|(to, extension)| {
            to.copy_from_slice(extension.as_bytes());
            to.make_ascii_lowercase();
            std::str::from_utf8(to).ok()
        });
    match extension {
        Some(
            "rs" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "c"
            | "h" | "cc" | "cpp" | "cxx" | "hpp" | "java" | "kt" | "kts" | "swift" | "rb" | "zig"
            | "lua" | "php" | "cs" | "scala" | "ex" | "exs" | "hs" | "ml" | "dart" | "vue"
            | "svelte" | "astro" | "html" | "htm" | "css" | "scss" | "sass" | "less" | "graphql"
            | "gql" | "proto",
        ) => IconName::FileCode,
        Some("json" | "jsonc" | "json5") => IconName::FileBraces,
        Some("toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "env" | "plist" | "xml") => {
            IconName::FileCog
        }
        Some("md" | "mdx" | "markdown" | "txt" | "rst" | "adoc") => IconName::FileText,
        Some("sh" | "bash" | "zsh" | "fish" | "ps1" | "bat") => IconName::FileTerminal,
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "icns" | "bmp" | "tiff"
            | "avif" | "heic",
        ) => IconName::FileImage,
        Some("zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst") => {
            IconName::FileArchive
        }
        Some("mp3" | "wav" | "flac" | "ogg" | "m4a") => IconName::FileMusic,
        Some("mp4" | "mov" | "mkv" | "webm" | "avi") => IconName::FileVideoCamera,
        Some("csv" | "tsv" | "xlsx" | "xls") => IconName::FileSpreadsheet,
        Some("lock") => IconName::FileLock,
        Some("diff" | "patch") => IconName::FileDiff,
        Some("key" | "pem" | "crt" | "cer" | "p12") => IconName::FileKey,
        Some("sql" | "db" | "sqlite") => IconName::Database,
        Some("ttf" | "otf" | "woff" | "woff2") => IconName::FileType,
        _ => IconName::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: Kind) -> Entry {
        Entry {
            name: name.into(),
            kind,
            ignored: false,
        }
    }

    #[test]
    fn files_are_told_apart_by_name_and_folders_by_whether_they_are_open() {
        let file = |name| icon(&entry(name, Kind::File), false);
        assert_eq!(file("lib.rs"), IconName::FileCode);
        assert_eq!(file("Cargo.lock"), IconName::FileLock);
        assert_eq!(file("README.md"), IconName::FileText);
        assert_eq!(file("logo.PNG"), IconName::FileImage);
        assert_eq!(file("Makefile"), IconName::FileTerminal);
        assert_eq!(file("noextension"), IconName::File);
        assert_eq!(icon(&entry("src", Kind::Dir), true), IconName::FolderOpen);
        assert_eq!(icon(&entry("src", Kind::Dir), false), IconName::Folder);
    }
}
