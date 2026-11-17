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

pub static RULES: [&Rule; 2] = [&COMMENT, &SAFETY];
