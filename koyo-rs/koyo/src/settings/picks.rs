//! Settings a menu's row sets when it's picked, each an action carrying the choice.

use gpui_kit::Action;

use super::BranchPrefix;

/// Turns the preview tab of files picked in the tree on or off, from the tree's menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetPreviewFiles(pub bool);

/// Picks how many language servers run at once, or no limit, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetMostLanguageServers(pub Option<usize>);

/// Picks what new branches are named under, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetBranchPrefix(pub BranchPrefix);
