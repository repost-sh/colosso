#[derive(Debug, PartialEq, Eq)]
pub struct Rule {
    pub name: &'static str,
    pub fix: &'static str,
}

pub static COMMENT: Rule = Rule {
    name: "comment",
    fix: "Koyo's source has no comments. Delete each one and say it where a program checks it:
  what the code does             a clearer name for the function, variable or type
  a condition that must hold     a type that cannot hold the wrong value, or a test named for it
  why a lint is silenced         #[expect(the_lint, reason = \"...\")]
  why an unsafe block is sound   // SAFETY: ... directly above the unsafe, the one comment allowed
To delete them all: script/cargo run -q -p koyo-hygiene -- --fix",
};

pub static SAFETY: Rule = Rule {
    name: "safety",
    fix: "A // SAFETY: comment argues for one unsafe block or impl and sits directly above it.
Move it there, or delete it if the code below it is safe.",
};

pub static NEW_EDGE: Rule = Rule {
    name: "new edge",
    fix: "The import closes a cycle between the koyo crate's top-level modules: the module
it reaches reaches back, so neither changes without the other. Don't add it. Take what the code
needs as an argument or an event instead, or move the item into a module that doesn't reach back.",
};

pub static GONE_EDGE: Rule = Rule {
    name: "gone edge",
    fix: "The edge no longer lies on a cycle, so its line leaves koyo-rs/module-edges.txt:
script/cargo run -q -p koyo-hygiene -- --prune",
};

pub static ADDED_EDGE: Rule = Rule {
    name: "added edge",
    fix: "koyo-rs/module-edges.txt only shrinks, and the last commit's list lacks this line. Change
the code so the edge closes no cycle; --prune then takes the line off. An import in a file git
finds renamed, or of one, keeps its line by its modules' new names, in place of the old line.",
};

pub static BLOCKING_CALL: Rule = Rule {
    name: "blocking call",
    fix: "The main thread draws Koyo's frames and runs the browser engine, so the koyo crate and
the browser host read files, probe paths, start and wait on processes, and wait on an executor or
a park only where a `Blocking` (koyo/src/blocking.rs, koyo-browser-host/src/blocking.rs) is in
scope, which code on the main thread isn't handed: in a function that takes one, or in the
closure blocking::spawn, thread, on_quit or in_test lends one to. Take `may: Blocking<'_>` from
the caller if it has one, and the callers of yours inherit the rule. Otherwise start the work with
blocking::spawn(cx.background_executor(), |may| ...), or in the browser host blocking::thread,
and show what it returns when it lands. A file on its crate's list (koyo-rs/blocking-debt.txt,
koyo-rs/blocking-debt-browser-host.txt) holds no more such calls outside a Blocking's scope than
its line allows.",
};

pub static LAUNCH: Rule = Rule {
    name: "launch read",
    fix: "blocking::at_launch holds the main thread before the first window, and only a crate's
root (koyo/src/main.rs, koyo-browser-host/src/lib.rs) may: no debt allows it elsewhere. Take
`may: Blocking<'_>` from the root's launch instead, or read with blocking::spawn once the window
is up.",
};

pub static GONE_CALL: Rule = Rule {
    name: "gone call",
    fix: "The file holds fewer blocking calls than its crate's list allows it, so its line comes
down, or off once it holds none: script/cargo run -q -p koyo-hygiene -- --prune",
};

pub static ADDED_CALL: Rule = Rule {
    name: "added call",
    fix: "A crate's list of blocking calls only shrinks, and the last commit's list lacks this
line or allows the file fewer calls. Move the calls onto a `Blocking` instead; --prune then lowers
the line. A file git finds renamed keeps its line by its new path, in place of the old line.",
};

pub static RULES: [&Rule; 9] = [
    &COMMENT,
    &SAFETY,
    &NEW_EDGE,
    &GONE_EDGE,
    &ADDED_EDGE,
    &BLOCKING_CALL,
    &LAUNCH,
    &GONE_CALL,
    &ADDED_CALL,
];
