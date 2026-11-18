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
the code so the edge closes no cycle; --prune then takes the line off.",
};

pub static RULES: [&Rule; 5] = [&COMMENT, &SAFETY, &NEW_EDGE, &GONE_EDGE, &ADDED_EDGE];
