use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::failure::Failure;

/// A folder of the tool's own in the workspace's build output, gone with all that's in it when
/// it's dropped.
pub struct Scratch {
    folder: PathBuf,
    /// The folder's path in the checkout.
    name: String,
}

impl Scratch {
    pub fn new(root: &Path, purpose: &str) -> Result<Self, Failure> {
        let name = format!("koyo-rs/target/tmp/hygiene-{}-{purpose}", process::id());
        let folder = root.join(&name);
        fs::create_dir_all(&folder).map_err(|failure| Failure::new(&name, failure))?;
        Ok(Self { folder, name })
    }

    /// The path of `file` in the folder.
    pub fn path(&self, file: &str) -> PathBuf {
        self.folder.join(file)
    }

    /// The path in the checkout of `file` in the folder.
    pub fn name(&self, file: &str) -> String {
        format!("{}/{file}", self.name)
    }

    pub fn write(&self, file: &str, bytes: &[u8]) -> Result<(), Failure> {
        fs::write(self.path(file), bytes).map_err(|failure| Failure::new(self.name(file), failure))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}
