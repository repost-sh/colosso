//! TypeScript's own language server: the native `tsc` of TypeScript 7's npm package for this
//! platform, which is the language server too (`tsc --lsp`), with no Node to run it. The
//! project's own when it has TypeScript 7 or newer in `node_modules`, so what the editor says
//! agrees with what the project builds with; else the pinned release, from the npm registry.
//! A project on an older TypeScript is better served by vtsls, VS Code's service, which runs
//! the project's own TypeScript; [`project_is_older`] says when.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::ServerSpec;
use super::release::{Archive, Release};

pub static TYPESCRIPT: ServerSpec = ServerSpec {
    name: "typescript",
    // A `tsc` on PATH is as likely TypeScript 5's or 6's, a script with no `--lsp`.
    on_path: false,
    project: project_binary,
    release: Release {
        version: "7.0.2",
        // The platform package `typescript` depends on: the binary and its library files.
        url: "https://registry.npmjs.org/@typescript/typescript-{target}/-/typescript-{target}-{version}.tgz",
        // Digests computed from the downloads.
        assets: &[
            (
                ("aarch64", "macos"),
                "darwin-arm64",
                "902e2fe1cf0799198ef902c6b8c310a450fef629a6baba41d45641ef75c04ebd",
            ),
            (
                ("x86_64", "macos"),
                "darwin-x64",
                "eba158cb54050f723d5ff781438f33de5640054440bb4f2bd170cfe9bc2eb551",
            ),
            (
                ("aarch64", "linux"),
                "linux-arm64",
                "c83d931ac9dd7549cde6e71246aa9d6a9812843023df3e277fe3b5dcf41dd0ea",
            ),
            (
                ("x86_64", "linux"),
                "linux-x64",
                "7ecad6f67377e831856367ab062ef394f21506a611405bf8ac0ff039348637d3",
            ),
        ],
        archive: Archive::Tarball,
        binary: "package/lib/tsc",
    },
    args: &["--lsp", "--stdio"],
    settings,
    initialization_options: None,
};

/// The TypeScript the project at `root` has in `node_modules`: the package's real folder
/// (pnpm keeps it in a store and links it) and its major version.
fn project_typescript(root: &Path) -> Option<(PathBuf, u64)> {
    let package = fs::canonicalize(root.join("node_modules/typescript")).ok()?;
    let manifest = fs::read_to_string(package.join("package.json")).ok()?;
    let manifest: Value = serde_json::from_str(&manifest).ok()?;
    let version = manifest.get("version")?.as_str()?;
    let major = version.split('.').next()?.parse().ok()?;
    Some((package, major))
}

/// The project's own `tsc`, when its TypeScript is 7 or newer: in the platform package
/// `typescript` depends on, beside it.
fn project_binary(root: &Path) -> Option<PathBuf> {
    let (package, major) = project_typescript(root)?;
    let target = TYPESCRIPT.release.target()?;
    let tsc = (package.parent()?).join(format!("@typescript/typescript-{target}/lib/tsc"));
    (major >= 7 && tsc.is_file()).then_some(tsc)
}

/// Whether the project at `root` is on a TypeScript older than its own native server, 5 or
/// 6, which vtsls, running that TypeScript, serves as the project's build reads it.
pub fn project_is_older(root: &Path) -> bool {
    project_typescript(root).is_some_and(|(_, major)| major < 7)
}

/// The server's settings, the same for both languages: every inlay hint on. The names are
/// VS Code's settings', which TypeScript's own server and vtsls both read.
pub(super) fn settings() -> Value {
    json!({
        "typescript": { "inlayHints": inlay_hints() },
        "javascript": { "inlayHints": inlay_hints() },
    })
}

/// Every kind of inlay hint TypeScript offers, parameter names where the argument is a
/// literal (VS Code's "literals"), which is where a name tells the most.
fn inlay_hints() -> Value {
    json!({
        "parameterNames": { "enabled": "literals" },
        "parameterTypes": { "enabled": true },
        "variableTypes": { "enabled": true },
        "propertyDeclarationTypes": { "enabled": true },
        "functionLikeReturnTypes": { "enabled": true },
        "enumMemberValues": { "enabled": true },
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::scratch;
    use super::*;

    /// A project at `root` with TypeScript `version` in its `node_modules`, the package in
    /// `store` and linked from `node_modules/typescript` as pnpm has it, or there itself.
    fn with_typescript(root: &Path, version: &str, store: Option<&str>) -> PathBuf {
        let modules = root.join("node_modules");
        let package = match store {
            Some(store) => modules.join(store).join("node_modules/typescript"),
            None => modules.join("typescript"),
        };
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), format!(r#"{{"version":"{version}"}}"#)).unwrap();
        if store.is_some() {
            std::os::unix::fs::symlink(&package, modules.join("typescript")).unwrap();
        }
        package
    }

    /// The platform package beside `typescript`, with a `tsc` that runs.
    fn with_platform_package(typescript: &Path) -> PathBuf {
        let target = TYPESCRIPT.release.target().expect("a platform this test runs on");
        let lib = typescript.parent().unwrap().join(format!("@typescript/typescript-{target}/lib"));
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("tsc"), "#!/bin/sh\nexit 0\n").unwrap();
        lib.join("tsc")
    }

    #[test]
    fn the_project_s_typescript_is_known_by_its_real_folder_and_major_version() {
        let root = scratch("typescript-major");
        assert_eq!(project_typescript(&root), None);
        assert!(!project_is_older(&root));
        let package = with_typescript(&root, "5.9.3", None);
        assert_eq!(project_typescript(&root), Some((fs::canonicalize(package).unwrap(), 5)));
        assert!(project_is_older(&root));
        // Linked as pnpm links it: the package where it really is.
        let root = scratch("typescript-pnpm");
        let package = with_typescript(&root, "7.0.2", Some(".pnpm/typescript@7.0.2"));
        assert_eq!(project_typescript(&root), Some((fs::canonicalize(package).unwrap(), 7)));
        assert!(!project_is_older(&root));
        // Unreadable: no TypeScript known.
        fs::write(root.join("node_modules/typescript/package.json"), "not json").unwrap();
        assert_eq!(project_typescript(&root), None);
    }

    #[test]
    fn the_project_s_own_tsc_is_the_platform_package_s_beside_its_typescript_7() {
        let root = scratch("typescript-own");
        let package = with_typescript(&root, "7.0.2", Some(".pnpm/typescript@7.0.2"));
        assert_eq!(project_binary(&root), None);
        let tsc = with_platform_package(&package);
        assert_eq!(project_binary(&root), Some(fs::canonicalize(&tsc).unwrap()));
        // TypeScript 5's, even with a platform package beside it, isn't the project's server.
        let root = scratch("typescript-own-older");
        let package = with_typescript(&root, "5.9.3", None);
        with_platform_package(&package);
        assert_eq!(project_binary(&root), None);
    }
}
