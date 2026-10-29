//! Servers that are npm packages run by Node: vtsls for TypeScript and JavaScript, and
//! basedpyright for Python. Node is the machine's, found on PATH; each package is installed
//! by npm into the app's data folder at a pinned version, staged and moved into place as a
//! downloaded server is, and run as `node script --stdio`, so neither the script's shebang
//! nor the PATH it would need matter.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::install::{io_failed, place, run, staging};
use super::first_that_runs;
use crate::client::Launch;
use crate::error::LspError;

/// One server that's an npm package.
pub struct NodeServer {
    /// Its name, for the log and the status bar, and its folder under `servers`.
    pub name: &'static str,
    /// The npm package, and the version installed.
    pub package: &'static str,
    pub version: &'static str,
    /// The script in the package that is the server, which Node runs.
    pub script: &'static str,
    /// The settings its `workspace/configuration` requests are answered from, by section.
    pub settings: fn() -> Value,
}

/// Runs npm to install a package.
pub trait Npm {
    /// Installs `spec`, a `package@version`, under `prefix`. Blocks.
    fn install(&self, prefix: &Path, spec: &str) -> Result<(), String>;
}

/// The npm beside Node, which being a Node script itself runs with Node's folder first on
/// its PATH; its cache is kept under the prefix while it runs, so it touches nothing of the
/// user's.
pub struct NpmCli {
    pub node: PathBuf,
}

impl Npm for NpmCli {
    fn install(&self, prefix: &Path, spec: &str) -> Result<(), String> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let folders = (self.node.parent().map(Path::to_path_buf).into_iter())
            .chain(std::env::split_paths(&path));
        let path = std::env::join_paths(folders).map_err(|error| error.to_string())?;
        let cache = prefix.join(".npm-cache");
        let mut npm = Command::new("npm");
        npm.arg("install")
            .args(["--no-audit", "--no-fund", "--loglevel", "error"])
            .arg("--prefix")
            .arg(prefix)
            .arg("--cache")
            .arg(&cache)
            .arg(spec)
            .env("PATH", path)
            .current_dir(prefix);
        let installed = run(&mut npm, "npm");
        let _ = fs::remove_dir_all(&cache);
        installed
    }
}

impl NodeServer {
    /// The package's script, installed by `npm` into `data`'s `servers/<name>/<version>`
    /// folder first unless it's there. Blocks, for as long as an install takes.
    pub fn script(&self, data: &Path, npm: &dyn Npm) -> Result<PathBuf, LspError> {
        let folder = data.join("servers").join(self.name);
        let version = folder.join(self.version);
        let script = Path::new("node_modules")
            .join(self.package)
            .join(self.script);
        if version.join(&script).is_file() {
            return Ok(version.join(script));
        }
        fs::create_dir_all(&folder).map_err(io_failed("create", &folder))?;
        let staging = staging(&folder);
        let spec = format!("{}@{}", self.package, self.version);
        log::info!("installing {spec} into {}", version.display());
        let installed = fs::create_dir(&staging)
            .map_err(io_failed("create", &staging))
            .and_then(|()| {
                npm.install(&staging, &spec).map_err(|error| {
                    LspError::Install(format!("npm couldn't install {spec}: {error}"))
                })
            })
            .and_then(|()| {
                if staging.join(&script).is_file() {
                    Ok(())
                } else {
                    Err(LspError::Install(format!("{spec} has no {}", self.script)))
                }
            })
            .and_then(|()| place(&staging, &version, &script));
        // Gone already after the rename; what a failure left behind otherwise.
        let _ = fs::remove_dir_all(&staging);
        installed?;
        Ok(version.join(script))
    }

    /// How to start the server: `node` running `script` for the workspace at `root`.
    pub fn launch(&self, node: PathBuf, script: &Path, root: &Path) -> Launch {
        Launch {
            name: self.name.to_owned(),
            program: node,
            args: vec![script.to_string_lossy().into_owned(), "--stdio".to_owned()],
            env: Vec::new(),
            cwd: root.to_owned(),
            settings: (self.settings)(),
            initialization_options: None,
            log: None,
        }
    }
}

/// Node on PATH, or in the folders Homebrew and the nodejs.org installer put it in, which
/// an app the Finder starts has none of on its PATH.
pub fn node() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from);
    first_that_runs(std::env::split_paths(&path).chain(usual), "node")
}

pub static VTSLS: NodeServer = NodeServer {
    name: "vtsls",
    package: "@vtsls/language-server",
    version: "0.3.0",
    script: "bin/vtsls.js",
    settings: vtsls_settings,
};

fn vtsls_settings() -> Value {
    json!({
        // The project's own TypeScript when it has one in `node_modules`, so what the
        // editor says agrees with what the project builds with.
        "vtsls": { "autoUseWorkspaceTsdk": true },
        // TypeScript's inlay hints are all off unless asked for.
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

pub static BASEDPYRIGHT: NodeServer = NodeServer {
    name: "basedpyright",
    package: "basedpyright",
    version: "1.40.2",
    script: "langserver.index.js",
    settings: basedpyright_settings,
};

fn basedpyright_settings() -> Value {
    json!({
        // Pyright's own strictness; basedpyright's `recommended` adds checks that flood a
        // project that wasn't written for them.
        "basedpyright": { "analysis": { "typeCheckingMode": "standard" } },
        "python": {},
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A folder of this crate's `target` for one test.
    fn scratch(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/node-tests")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// An npm that writes vtsls's script where the package would have it, and counts.
    #[derive(Default)]
    struct Fake {
        installs: Cell<usize>,
    }

    impl Npm for Fake {
        fn install(&self, prefix: &Path, spec: &str) -> Result<(), String> {
            assert_eq!(spec, "@vtsls/language-server@0.3.0");
            self.installs.set(self.installs.get() + 1);
            let package = prefix.join("node_modules").join(VTSLS.package);
            fs::create_dir_all(package.join("bin")).unwrap();
            fs::write(package.join(VTSLS.script), "// the server\n").unwrap();
            Ok(())
        }
    }

    /// An npm that installs nothing and says nothing of it.
    struct Nothing;

    impl Npm for Nothing {
        fn install(&self, _: &Path, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_package_is_installed_once_into_a_folder_of_its_version() {
        let data = scratch("install");
        let npm = Fake::default();
        let script = VTSLS.script(&data, &npm).unwrap();
        let expected = "servers/vtsls/0.3.0/node_modules/@vtsls/language-server/bin/vtsls.js";
        assert_eq!(script, data.join(expected));
        assert!(script.is_file());
        // Nothing of the install is left beside it.
        let left: Vec<_> = fs::read_dir(data.join("servers/vtsls")).unwrap().collect();
        assert_eq!(left.len(), 1);
        // There already: not installed again.
        assert_eq!(VTSLS.script(&data, &npm).unwrap(), script);
        assert_eq!(npm.installs.get(), 1);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn an_install_that_leaves_no_script_fails_and_leaves_nothing() {
        let data = scratch("no-script");
        let failed = VTSLS.script(&data, &Nothing);
        assert!(matches!(failed, Err(LspError::Install(why)) if why.contains("has no bin/vtsls.js")));
        assert!(fs::read_dir(data.join("servers/vtsls")).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn node_is_the_first_on_the_path_that_runs() {
        let folder = scratch("node");
        assert_eq!(first_that_runs([folder.clone()], "node"), None);
        let node = folder.join("node");
        fs::write(&node, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let folders = [PathBuf::from("/nowhere"), folder.clone()];
        assert_eq!(first_that_runs(folders, "node"), Some(node));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_launch_runs_the_script_with_node() {
        let node = PathBuf::from("/usr/local/bin/node");
        let launch = VTSLS.launch(node.clone(), Path::new("/data/vtsls.js"), Path::new("/work"));
        assert_eq!(launch.program, node);
        assert_eq!(launch.args, ["/data/vtsls.js", "--stdio"]);
        assert_eq!(launch.cwd, PathBuf::from("/work"));
        assert!(launch.settings.get("vtsls").is_some());
    }
}
