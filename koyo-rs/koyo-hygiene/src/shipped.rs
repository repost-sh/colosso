mod attributes;
mod code;

use std::fs;
use std::path::{Path, PathBuf};

use crate::failure::Failure;
use crate::pool::map_in_order;

use code::{Body, read};
pub use code::{Code, Declared, Kind, Token};

/// The root of the koyo crate's shipped build, the app.
pub const KOYO: &str = "koyo-rs/koyo/src/main.rs";

/// The folder of the koyo crate's source.
pub const KOYO_FILES: &str = "koyo-rs/koyo/src";

/// The root of the browser host's, the library the app runs the engine with on its main
/// thread.
pub const BROWSER_HOST: &str = "koyo-rs/koyo-browser-host/src/lib.rs";

/// A file of a crate's module tree, by its path in the checkout.
pub struct File {
    pub path: PathBuf,
    /// The module the file is, `[]` for the crate root.
    pub module: Vec<String>,
    /// Where `mod x;` in the file looks for `x.rs` and `x/mod.rs`.
    owns: PathBuf,
}

/// Each file of the crate at `crate_root` (its root's path in the checkout) the shipped build
/// compiles, read from the checkout at `root` and handed to `read` with its text and its code,
/// the shipped build's alone: the module tree walked from the crate root as rustc does, a level
/// of files at a time, each level's files read in parallel. What `read` makes of each comes
/// back in that order, the root's first.
pub fn walk<T: Send>(
    root: &Path,
    crate_root: &str,
    read: impl Fn(&File, &str, &Code<'_>) -> T + Sync,
) -> Result<Vec<T>, Failure> {
    let crate_root = PathBuf::from(crate_root);
    let owns = crate_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut level = vec![File {
        path: crate_root,
        module: Vec::new(),
        owns,
    }];
    let mut made = Vec::new();
    while !level.is_empty() {
        let outcomes = map_in_order(&level, |file| read_file(root, file, &read))
            .map_err(|failure| Failure::new("threads", failure))?;
        let mut next = Vec::new();
        for outcome in outcomes {
            let (files, one) = outcome?;
            made.push(one);
            next.extend(files);
        }
        level = next;
    }
    Ok(made)
}

fn read_file<T>(
    root: &Path,
    file: &File,
    read_code: impl Fn(&File, &str, &Code<'_>) -> T,
) -> Result<(Vec<File>, T), Failure> {
    let display = file.path.display().to_string();
    let text = fs::read_to_string(root.join(&file.path))
        .map_err(|failure| Failure::new(&display, failure))?;
    let code = read(&text).map_err(|failure| Failure::new(&display, failure))?;
    let files = code
        .modules
        .iter()
        .filter_map(|declared| match &declared.body {
            Body::File { path } => Some(locate(root, file, declared, path.as_deref())),
            Body::Inline => None,
        })
        .collect::<Result<_, _>>()?;
    Ok((files, read_code(file, &text, &code)))
}

fn locate(
    root: &Path,
    file: &File,
    declared: &Declared,
    path: Option<&str>,
) -> Result<File, Failure> {
    let module = [
        file.module.as_slice(),
        &declared.within,
        std::slice::from_ref(&declared.name),
    ]
    .concat();
    let inline = declared
        .within
        .iter()
        .fold(file.owns.clone(), |folder, name| folder.join(name));
    if let Some(path) = path {
        let base = if declared.within.is_empty() {
            file.path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            inline
        };
        let path = base.join(path);
        let owns = path.parent().map(Path::to_path_buf).unwrap_or_default();
        return Ok(File { path, module, owns });
    }
    let owns = inline.join(&declared.name);
    let candidates = [
        inline.join(format!("{}.rs", declared.name)),
        owns.join("mod.rs"),
    ];
    match candidates
        .iter()
        .find(|candidate| root.join(candidate).is_file())
    {
        Some(path) => Ok(File {
            path: path.clone(),
            module,
            owns,
        }),
        None => Err(Failure::new(
            format!("{}:{}", file.path.display(), declared.line),
            format!(
                "mod {} has no file: neither {} nor {} exists",
                declared.name,
                candidates[0].display(),
                candidates[1].display()
            ),
        )),
    }
}
