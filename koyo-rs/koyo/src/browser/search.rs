//! The search engine an address's words go to, and that's asked what it suggests as they're
//! typed: one of a few, as Firefox and Chrome offer them, Google unless the person picks
//! another in Settings → Browser.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use crate::settings;

/// A search engine the person can pick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    #[default]
    Google,
    DuckDuckGo,
    Bing,
    Ecosia,
    Kagi,
}

/// Picks the search engine, from the Browser settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct SetSearchEngine(pub SearchEngine);

/// What an engine is called, and its addresses, each of which the URL-encoded words end.
struct Engine {
    name: &'static str,
    /// What the new-tab launcher's search row says it does.
    hint: &'static str,
    results: &'static str,
    /// Where it answers what it suggests in Firefox's format, `["words", ["suggestion", …]]`:
    /// the address Firefox asks it at, or Kagi's, which Firefox doesn't list, as its own
    /// search description names it.
    suggests: &'static str,
}

impl SearchEngine {
    pub const ALL: [SearchEngine; 5] = [
        SearchEngine::Google,
        SearchEngine::DuckDuckGo,
        SearchEngine::Bing,
        SearchEngine::Ecosia,
        SearchEngine::Kagi,
    ];

    fn engine(self) -> Engine {
        match self {
            SearchEngine::Google => Engine {
                name: "Google",
                hint: "Search with Google",
                results: "https://www.google.com/search?q=",
                suggests: "https://www.google.com/complete/search?client=firefox&ie=utf-8&oe=utf-8&q=",
            },
            SearchEngine::DuckDuckGo => Engine {
                name: "DuckDuckGo",
                hint: "Search with DuckDuckGo",
                results: "https://duckduckgo.com/?q=",
                suggests: "https://ac.duckduckgo.com/ac/?type=list&q=",
            },
            SearchEngine::Bing => Engine {
                name: "Bing",
                hint: "Search with Bing",
                results: "https://www.bing.com/search?q=",
                suggests: "https://www.bing.com/osjson.aspx?query=",
            },
            SearchEngine::Ecosia => Engine {
                name: "Ecosia",
                hint: "Search with Ecosia",
                results: "https://www.ecosia.org/search?q=",
                suggests: "https://ac.ecosia.org/autocomplete?type=list&q=",
            },
            SearchEngine::Kagi => Engine {
                name: "Kagi",
                hint: "Search with Kagi",
                results: "https://kagi.com/search?q=",
                suggests: "https://kagisuggest.com/api/autosuggest?q=",
            },
        }
    }

    pub fn label(self) -> &'static str {
        self.engine().name
    }

    /// What the new-tab launcher's row that searches says.
    pub fn hint(self) -> &'static str {
        self.engine().hint
    }

    /// The page of its results for `words`.
    pub fn results(self, words: &str) -> String {
        format!("{}{}", self.engine().results, encode(words))
    }

    /// Where it's asked what it suggests for `words`.
    pub fn suggestions(self, words: &str) -> String {
        format!("{}{}", self.engine().suggests, encode(words))
    }

    /// A menu of the engines, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        settings::checked_menu(SearchEngine::ALL, self, SearchEngine::label, |engine| {
            Box::new(SetSearchEngine(engine))
        })
    }
}

fn encode(words: &str) -> String {
    form_urlencoded::byte_serialize(words.as_bytes()).collect()
}
