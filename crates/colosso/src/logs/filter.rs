//! Which records are kept: a level for everything, and levels for the modules named.
//!
//! A filter reads like `RUST_LOG`: `debug` keeps everything at debug and above,
//! `colosso::changes=trace` turns one module up, and `info,gix=warn` both at once. A module's
//! level covers the modules inside it, and the longest name that matches wins.

use log::LevelFilter;

/// What's kept when nothing says otherwise.
const DEFAULT: LevelFilter = LevelFilter::Info;

/// Modules quieter than the rest unless a filter names them, for what they say every launch
/// that no one can act on.
const QUIET: [(&str, LevelFilter); 3] = [
    // macOS lists two of its system fonts twice; GPUI warns about both each launch.
    ("gpui_apple::text_system", LevelFilter::Error),
    // Advice on making elements accessible, said again as focus moves.
    ("gpui::window::a11y", LevelFilter::Warn),
    // An accessibility update sent, at debug, many times a second.
    ("gpui::window", LevelFilter::Info),
];

#[derive(Debug)]
pub(super) struct Filter {
    level: LevelFilter,
    /// Modules with levels of their own.
    modules: Vec<(String, LevelFilter)>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            level: DEFAULT,
            modules: (QUIET.iter())
                .map(|&(module, level)| (module.to_owned(), level))
                .collect(),
        }
    }
}

impl Filter {
    /// Reads `filter`, saying which part it couldn't.
    pub(super) fn parse(filter: &str) -> Result<Filter, String> {
        let mut parsed = Filter::default();
        let parts = filter
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty());
        for part in parts {
            match part.split_once('=') {
                Some((module, level)) => {
                    let level = level_named(level.trim()).ok_or_else(|| invalid(part))?;
                    parsed.modules.push((module.trim().to_owned(), level));
                }
                None => match level_named(part) {
                    Some(level) => parsed.level = level,
                    // A bare module name keeps everything it logs.
                    None => parsed.modules.push((part.to_owned(), LevelFilter::Trace)),
                },
            }
        }
        Ok(parsed)
    }

    /// The level kept for records from `target`, a module path.
    pub(super) fn level(&self, target: &str) -> LevelFilter {
        (self.modules.iter())
            .filter(|(module, _)| covers(module, target))
            // Of equally long names, the last: a filter's own over the quiet ones.
            .max_by_key(|(module, _)| module.len())
            .map_or(self.level, |(_, level)| *level)
    }

    /// The most anything is kept at, so what's below it is dropped before it's formatted.
    pub(super) fn most(&self) -> LevelFilter {
        (self.modules.iter())
            .map(|(_, level)| *level)
            .fold(self.level, Ord::max)
    }
}

/// Whether `module` is `target` or a module it's inside.
fn covers(module: &str, target: &str) -> bool {
    target
        .strip_prefix(module)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

fn level_named(name: &str) -> Option<LevelFilter> {
    // `none` as well as `log`'s own `off`, as Zed reads it.
    if name.eq_ignore_ascii_case("none") {
        return Some(LevelFilter::Off);
    }
    name.parse().ok()
}

fn invalid(part: &str) -> String {
    format!("“{part}” isn't a module and a level, like `colosso::changes=debug`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_keeps_info_and_above() {
        let filter = Filter::parse("").unwrap();
        assert_eq!(filter.level("colosso::shell"), LevelFilter::Info);
        assert_eq!(filter.most(), LevelFilter::Info);
    }

    #[test]
    fn a_module_covers_the_modules_inside_it_and_the_longest_wins() {
        let filter = Filter::parse("warn, colosso=info, colosso::changes=trace").unwrap();
        assert_eq!(filter.level("gpui_fast::window"), LevelFilter::Warn);
        assert_eq!(filter.level("colosso::shell"), LevelFilter::Info);
        assert_eq!(filter.level("colosso::changes"), LevelFilter::Trace);
        assert_eq!(filter.level("colosso::changes::state"), LevelFilter::Trace);
        // A name only covers whole modules.
        assert_eq!(filter.level("colosso_git"), LevelFilter::Warn);
        assert_eq!(filter.most(), LevelFilter::Trace);
    }

    #[test]
    fn a_bare_module_keeps_everything_it_logs() {
        let filter = Filter::parse("colosso_git").unwrap();
        assert_eq!(filter.level("colosso_git::command"), LevelFilter::Trace);
        assert_eq!(filter.level("colosso::shell"), LevelFilter::Info);
    }

    #[test]
    fn levels_read_in_any_case_and_none_turns_a_module_off() {
        let filter = Filter::parse("DEBUG,gix=None").unwrap();
        assert_eq!(filter.level("colosso"), LevelFilter::Debug);
        assert_eq!(filter.level("gix::status"), LevelFilter::Off);
    }

    #[test]
    fn a_quiet_module_stays_quiet_unless_a_filter_names_it() {
        let quiet = Filter::parse("debug").unwrap();
        assert_eq!(quiet.level("gpui_apple::text_system"), LevelFilter::Error);
        let named = Filter::parse("gpui_apple::text_system=warn").unwrap();
        assert_eq!(named.level("gpui_apple::text_system"), LevelFilter::Warn);
    }

    #[test]
    fn a_level_that_isnt_one_is_refused() {
        assert!(Filter::parse("colosso=loud").is_err());
    }
}
