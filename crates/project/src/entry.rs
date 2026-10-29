//! A folder's entries and the order they're listed in: folders first, then by name as a
//! person sorts names, with case set aside and numbers read as numbers.

use std::cmp::Ordering;
use std::sync::Arc;

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    File,
    Dir,
    /// A symbolic link, and whether it leads to a folder. Nothing is read through one: a
    /// link can lead out of the checkout, or round in a circle.
    Symlink {
        to_dir: bool,
    },
}

/// One file or folder in a folder's listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Arc<str>,
    pub kind: Kind,
    /// Whether git ignores it: by a `.gitignore`, the repository's excludes or the user's,
    /// or by being inside a folder that's ignored.
    pub ignored: bool,
}

impl Kind {
    /// Whether it's shown with the folders: a folder, or a link to one.
    pub fn is_folder(self) -> bool {
        matches!(self, Kind::Dir | Kind::Symlink { to_dir: true })
    }
}

impl Entry {
    /// Whether it's shown with the folders: a folder, or a link to one.
    pub fn is_folder(&self) -> bool {
        self.kind.is_folder()
    }
}

/// The order a listing is in: folders first, then by `natural` name.
pub fn order(a: &Entry, b: &Entry) -> Ordering {
    by_place((a.is_folder(), &a.name), (b.is_folder(), &b.name))
}

/// `order` for an entry by whether it's a folder and its name, so one needn't be made.
pub(crate) fn by_place(a: (bool, &str), b: (bool, &str)) -> Ordering {
    (b.0.cmp(&a.0)).then_with(|| natural(a.1, b.1))
}

/// Names as a person sorts them: letters without regard to case, and runs of digits by
/// their value, so `file2` comes before `file10`. Names equal that way still have an order,
/// by their bytes, so a listing never shuffles between reads.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (Some(cx), Some(cy)) = (x.chars().next(), y.chars().next()) else {
            break (!x.is_empty()).cmp(&!y.is_empty()).then_with(|| a.cmp(b));
        };
        if cx.is_ascii_digit() && cy.is_ascii_digit() {
            let (nx, rest_x) = digits(x);
            let (ny, rest_y) = digits(y);
            let ordering = by_value(nx, ny);
            if ordering.is_ne() {
                return ordering;
            }
            (x, y) = (rest_x, rest_y);
        } else {
            let ordering = if cx.is_ascii() && cy.is_ascii() {
                cx.to_ascii_lowercase().cmp(&cy.to_ascii_lowercase())
            } else {
                cx.to_lowercase().cmp(cy.to_lowercase())
            };
            if ordering.is_ne() {
                return ordering;
            }
            (x, y) = (&x[cx.len_utf8()..], &y[cy.len_utf8()..]);
        }
    }
}

/// The run of digits `text` starts with, and the rest.
fn digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

/// Two runs of digits by the numbers they write, however long.
fn by_value(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// The entry named `name` in `entries`, which are in `order`.
pub(crate) fn find<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    // Folders and files are sorted apart, so the name is looked for among each.
    [true, false].into_iter().find_map(|folder| {
        let at = (entries
            .binary_search_by(|entry| by_place((entry.is_folder(), &entry.name), (folder, name))))
        .ok()?;
        entries.get(at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        names.sort_by(|a, b| natural(a, b));
        names
    }

    #[test]
    fn numbers_sort_by_value_and_letters_without_case() {
        assert_eq!(
            sorted(&["file10", "File2", "file1", "a", "B", "file02", "file", "Ä"]),
            ["a", "B", "file", "file1", "File2", "file02", "file10", "Ä"]
        );
    }

    #[test]
    fn names_alike_but_for_case_still_have_one_order() {
        assert_eq!(
            sorted(&["readme", "README", "Readme"]),
            ["README", "Readme", "readme"]
        );
        assert_eq!(natural("x1", "x01"), Ordering::Greater);
    }

    #[test]
    fn folders_come_first_and_are_found_by_name() {
        let entry = |name: &str, kind| Entry {
            name: name.into(),
            kind,
            ignored: false,
        };
        let mut entries = vec![
            entry("a.txt", Kind::File),
            entry("z", Kind::Dir),
            entry("link", Kind::Symlink { to_dir: true }),
            entry("b", Kind::Symlink { to_dir: false }),
        ];
        entries.sort_by(order);
        let names: Vec<&str> = entries.iter().map(|entry| &*entry.name).collect();
        assert_eq!(names, ["link", "z", "a.txt", "b"]);
        for name in ["link", "z", "a.txt", "b"] {
            assert_eq!(find(&entries, name).map(|entry| &*entry.name), Some(name));
        }
        assert!(find(&entries, "missing").is_none());
    }
}
