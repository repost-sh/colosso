//! The commands a person keeps for a project, like its dev server or its tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectScript {
    pub name: String,
    pub command: String,
}

/// The scripts of each project, by its main checkout.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectScripts(BTreeMap<PathBuf, Vec<ProjectScript>>);

impl ProjectScripts {
    pub fn of(&self, project: &Path) -> &[ProjectScript] {
        self.0.get(project).map_or(&[], Vec::as_slice)
    }

    pub fn add(&mut self, project: PathBuf, script: ProjectScript) {
        self.0.entry(project).or_default().push(script);
    }

    /// Takes away the project's script at `at`, and the project's entry with its last one.
    pub fn remove(&mut self, project: &Path, at: usize) {
        let Some(scripts) = self.0.get_mut(project) else {
            return;
        };
        if at < scripts.len() {
            scripts.remove(at);
        }
        if scripts.is_empty() {
            self.0.remove(project);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_kept_by_project_and_go_with_the_last_of_them() {
        let (one, two) = (Path::new("/one"), Path::new("/two"));
        let script = |name: &str| ProjectScript {
            name: name.to_owned(),
            command: "true".to_owned(),
        };
        let mut scripts = ProjectScripts::default();
        scripts.add(one.to_owned(), script("a"));
        scripts.add(one.to_owned(), script("b"));
        assert_eq!(scripts.of(one).len(), 2);
        assert!(scripts.of(two).is_empty());
        scripts.remove(one, 0);
        assert_eq!(scripts.of(one), [script("b")]);
        scripts.remove(one, 5);
        assert_eq!(scripts.of(one), [script("b")]);
        scripts.remove(one, 0);
        assert_eq!(scripts, ProjectScripts::default());
    }
}
