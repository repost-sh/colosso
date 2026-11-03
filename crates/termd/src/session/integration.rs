//! Ghostty's shell integration, injected the way Ghostty injects it (its
//! `src/shell-integration/README.md`), so the shell marks its prompts and commands (OSC 133)
//! and reports its folder (OSC 7), which the status follows (`docs/terminals.md` 5.7): zsh
//! through `ZDOTDIR`, fish through `XDG_DATA_DIRS`. bash isn't injected: Ghostty's script
//! needs bash 4.4, and macOS's is 3.2. `KOYO_SHELL_INTEGRATION=off` in a terminal's
//! environment leaves its shell alone.

use std::path::Path;

/// The variables that load the integration into `program`, a shell's path or name, the
/// scripts being under `resources` (Ghostty's `GHOSTTY_RESOURCES_DIR`). `get` reads what
/// the shell's environment holds otherwise, so the person's own `ZDOTDIR` and
/// `XDG_DATA_DIRS` are kept.
pub fn variables(
    program: &str,
    resources: &Path,
    get: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    if get("KOYO_SHELL_INTEGRATION").as_deref() == Some("off") {
        return Vec::new();
    }
    let scripts = resources.join("shell-integration");
    let name = (Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .trim_start_matches('-');
    match name {
        "zsh" => {
            // zsh reads Ghostty's .zshenv first, which puts the person's ZDOTDIR back and
            // reads their own files from it.
            let zdotdir = scripts.join("zsh").to_string_lossy().into_owned();
            let mut variables = vec![("ZDOTDIR".to_owned(), zdotdir)];
            if let Some(own) = get("ZDOTDIR") {
                variables.push(("GHOSTTY_ZSH_ZDOTDIR".to_owned(), own));
            }
            variables
        }
        "fish" => {
            // fish's vendor_conf.d loads the integration, which then takes its folder back
            // out of XDG_DATA_DIRS.
            let dir = scripts.to_string_lossy().into_owned();
            let rest = (get("XDG_DATA_DIRS").filter(|dirs| !dirs.is_empty()))
                .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
            vec![
                ("XDG_DATA_DIRS".to_owned(), format!("{dir}:{rest}")),
                ("GHOSTTY_SHELL_INTEGRATION_XDG_DIR".to_owned(), dir),
            ]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn injected(program: &str, env: &[(&str, &str)]) -> BTreeMap<String, String> {
        let env: BTreeMap<String, String> = env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        variables(program, Path::new("/res"), |key| env.get(key).cloned())
            .into_iter()
            .collect()
    }

    #[test]
    fn zsh_starts_from_ghosttys_folder_and_keeps_the_persons_own() {
        let plain = injected("/bin/zsh", &[]);
        assert_eq!(
            plain.get("ZDOTDIR").map(String::as_str),
            Some("/res/shell-integration/zsh")
        );
        assert!(
            !plain.contains_key("GHOSTTY_ZSH_ZDOTDIR"),
            "unset stays unset: zsh reads HOME"
        );

        let own = injected("-zsh", &[("ZDOTDIR", "/me/.config/zsh")]);
        assert_eq!(
            own.get("GHOSTTY_ZSH_ZDOTDIR").map(String::as_str),
            Some("/me/.config/zsh")
        );
    }

    #[test]
    fn fish_finds_the_scripts_first_among_its_data_folders() {
        let plain = injected("/opt/homebrew/bin/fish", &[]);
        assert_eq!(
            plain.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/res/shell-integration:/usr/local/share:/usr/share")
        );
        let own = injected("fish", &[("XDG_DATA_DIRS", "/a:/b")]);
        assert_eq!(
            own.get("XDG_DATA_DIRS").map(String::as_str),
            Some("/res/shell-integration:/a:/b")
        );
        assert_eq!(
            own.get("GHOSTTY_SHELL_INTEGRATION_XDG_DIR")
                .map(String::as_str),
            Some("/res/shell-integration")
        );
    }

    #[test]
    fn other_programs_and_a_terminal_that_says_off_are_left_alone() {
        assert!(injected("/bin/bash", &[]).is_empty());
        assert!(injected("/usr/bin/vim", &[]).is_empty());
        assert!(injected("/bin/zsh", &[("KOYO_SHELL_INTEGRATION", "off")]).is_empty());
    }
}
