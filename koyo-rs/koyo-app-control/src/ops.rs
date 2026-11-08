//! Every operation's input and output, one module per domain (`docs/app-control.md` 5).
//! Fields are camelCase on the wire; inputs refuse unknown fields; each field's doc comment
//! is its description in the schema agents read.
//!
//! Selectors are strings, as section 3 defines them: a `project` is its main checkout's path
//! or its name when unique; a `worktree` is a checkout path, a branch name, or `current`;
//! a `pane` is the id `tab_list` lists.

pub mod activity;
pub mod app;
pub mod browser;
pub mod chat;
pub mod file;
pub mod pane;
pub mod project;
pub mod settings;
pub mod shared;
pub mod tab;
pub mod worktree;
