//! TypeScript's own language server: the native `tsc` of TypeScript 7's npm package for this
//! platform, which is the language server too (`tsc --lsp`), with no Node to run it. The
//! project's own when TypeScript 7 or newer is installed in the checkout, so what the editor
//! says agrees with what the project builds with; else the pinned release, from the npm
//! registry. A checkout with a project on an older TypeScript anywhere in it is better served
//! by vtsls, VS Code's service, running that TypeScript: TypeScript 7 dropped `baseUrl`,
//! `moduleResolution: node` and more, so its server can't even follow such a project's
//! imports. [`engine`] chooses, from every TypeScript installed in the checkout.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::release::{Archive, Release};
use super::{ServerSpec, runs};

pub static TYPESCRIPT: ServerSpec = ServerSpec {
    name: "typescript",
    // A `tsc` on PATH is as likely TypeScript 5's or 6's, a script with no `--lsp`.
    on_path: false,
    // The project's own is found by [`engine`], among every TypeScript installed in the
    // checkout.
    project: |_| None,
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

/// Which server serves the TypeScript of a checkout.
#[derive(Debug, PartialEq, Eq)]
pub enum Engine {
    /// vtsls, running the TypeScript at this `typescript` package folder: a project still on
    /// 5 or 6 is in the checkout, the newest such being this one. Any such project decides
    /// for the whole checkout, since TypeScript 7's server can't read it at all, while
    /// TypeScript 5 or 6 reads a TypeScript 7 project near enough as 7 does.
    Vtsls { typescript: PathBuf },
    /// TypeScript's own server, the project's own `tsc` when one runs: the newest TypeScript
    /// 7 or newer installed, in the platform package `typescript` depends on, beside it.
    Native { tsc: Option<PathBuf> },
}

/// The engine for the checkout at `root`, from every TypeScript installed in it.
pub fn engine(root: &Path) -> Engine {
    let installed = installed(root);
    if let Some(older) = installed.iter().find(|typescript| typescript.version.0 < 7) {
        return Engine::Vtsls { typescript: older.package.clone() };
    }
    let tsc = TYPESCRIPT.release.target().and_then(|target| {
        let platform = format!("@typescript/typescript-{target}/lib/tsc");
        (installed.iter().filter(|typescript| typescript.version.0 >= 7))
            .find_map(|typescript| {
                let tsc = typescript.package.parent()?.join(&platform);
                runs(&tsc).then_some(tsc)
            })
    });
    Engine::Native { tsc }
}

/// How deep below the root TypeScript projects are looked for: a monorepo keeps its packages
/// a folder or two down (`packages/*`, `apps/*`), an example a third.
const DEPTH: usize = 3;

/// How many folders are looked into at most, the shallower first: a root with more within
/// reach is no checkout of a few packages but a home folder or the like, and the time is
/// better spent starting the server.
const FOLDERS: usize = 1000;

/// Whether a folder of the name may hold a project of the checkout's own: dependencies,
/// builds and hidden folders don't.
fn looked_into(name: &str) -> bool {
    !name.starts_with('.') && !["node_modules", "target", "dist", "build"].contains(&name)
}

/// A TypeScript installed in the checkout.
struct Installed {
    /// The package's real folder: pnpm keeps it in a store and links it from `node_modules`.
    package: PathBuf,
    version: (u64, u64, u64),
}

/// Every TypeScript installed in the checkout at `root`, newest first: each project's
/// `node_modules/typescript`, the root's and those within [`DEPTH`] folders of it.
fn installed(root: &Path) -> Vec<Installed> {
    let mut found = Vec::new();
    let mut folders = VecDeque::from([(root.to_path_buf(), 0)]);
    let mut looked = 0;
    while let Some((folder, depth)) = folders.pop_front() {
        found.extend(installed_in(&folder));
        looked += 1;
        if depth == DEPTH || looked == FOLDERS {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if is_folder && looked_into(&entry.file_name().to_string_lossy()) {
                folders.push_back((entry.path(), depth + 1));
            }
        }
    }
    found.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| a.package.cmp(&b.package)));
    found
}

/// The TypeScript the project at `folder` has in its `node_modules`.
fn installed_in(folder: &Path) -> Option<Installed> {
    let package = fs::canonicalize(folder.join("node_modules/typescript")).ok()?;
    let manifest = fs::read_to_string(package.join("package.json")).ok()?;
    let manifest: Value = serde_json::from_str(&manifest).ok()?;
    let version = version(manifest.get("version")?.as_str()?)?;
    Some(Installed { package, version })
}

/// The three numbers of a version like `5.9.3`, or of `7.0.0-beta`'s before its dash.
fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut numbers = text.split('-').next()?.split('.').map(str::parse);
    Some((numbers.next()?.ok()?, numbers.next()?.ok()?, numbers.next()?.ok()?))
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
    use super::super::install::make_executable;
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
        make_executable(&lib.join("tsc")).unwrap();
        lib.join("tsc")
    }

    #[test]
    fn every_typescript_installed_within_three_folders_is_found_and_the_newest_older_one_runs() {
        let root = scratch("typescript-installed");
        assert!(installed(&root).is_empty());
        assert_eq!(engine(&root), Engine::Native { tsc: None });
        let at_root = with_typescript(&root, "5.9.3", None);
        let in_api = with_typescript(&root.join("packages/api"), "4.9.5", None);
        // Linked as pnpm links it: the package where it really is.
        let store = Some(".pnpm/typescript@7.0.2");
        let in_web = with_typescript(&root.join("packages/web"), "7.0.2", store);
        let deep = with_typescript(&root.join("examples/typescript/web"), "6.0.0", None);
        // Four folders down, under a dependency, under a build, hidden: not looked into.
        with_typescript(&root.join("a/b/c/d"), "7.1.0", None);
        with_typescript(&root.join("node_modules/dep"), "7.1.0", None);
        with_typescript(&root.join("dist/app"), "7.1.0", None);
        with_typescript(&root.join(".cache/app"), "7.1.0", None);
        let real = |package: &Path| fs::canonicalize(package).unwrap();
        let found: Vec<_> = (installed(&root).iter())
            .map(|typescript| (typescript.package.clone(), typescript.version))
            .collect();
        let expected = [
            (real(&in_web), (7, 0, 2)),
            (real(&deep), (6, 0, 0)),
            (real(&at_root), (5, 9, 3)),
            (real(&in_api), (4, 9, 5)),
        ];
        assert_eq!(found, expected);
        // The newest older than 7 is the one vtsls runs, wherever it is.
        assert_eq!(engine(&root), Engine::Vtsls { typescript: real(&deep) });
        // Unreadable: not known.
        fs::write(at_root.join("package.json"), "not json").unwrap();
        assert_eq!(installed(&root).len(), 3);
    }

    #[test]
    fn a_version_is_its_three_numbers_before_any_dash() {
        assert_eq!(version("5.9.3"), Some((5, 9, 3)));
        assert_eq!(version("7.0.0-beta"), Some((7, 0, 0)));
        assert_eq!(version("7.0"), None);
        assert_eq!(version("seven"), None);
    }

    #[test]
    fn the_project_s_own_tsc_is_the_platform_package_s_beside_its_typescript_7_when_it_runs() {
        let root = scratch("typescript-own");
        let store = Some(".pnpm/typescript@7.0.2");
        let package = with_typescript(&root.join("packages/web"), "7.0.2", store);
        assert_eq!(engine(&root), Engine::Native { tsc: None });
        let tsc = with_platform_package(&package);
        assert_eq!(engine(&root), Engine::Native { tsc: Some(fs::canonicalize(&tsc).unwrap()) });
        // One that doesn't run is no server.
        fs::write(&tsc, "#!/bin/sh\nexit 1\n").unwrap();
        assert_eq!(engine(&root), Engine::Native { tsc: None });
        // TypeScript 5's, even with a platform package beside it, isn't the project's server.
        let root = scratch("typescript-own-older");
        let package = with_typescript(&root, "5.9.3", None);
        with_platform_package(&package);
        let typescript = fs::canonicalize(&package).unwrap();
        assert_eq!(engine(&root), Engine::Vtsls { typescript });
    }
}
