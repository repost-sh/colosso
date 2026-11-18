use crate::shipped::{Code, Kind, Token};

use super::names::Names;

/// The functions of `koyo/src/blocking.rs` that lend a `Blocking` to the closure handed them.
const MAKERS: [&str; 5] = ["spawn", "thread", "on_quit", "at_launch", "in_test"];

/// The types a probe's receiver may be when it asks the file system: a path, what holds one,
/// and the file system's own entries.
const PATH_LIKE: [&str; 8] = [
    "Path", "PathBuf", "Cow", "Box", "Rc", "Arc", "DirEntry", "File",
];

/// A call that blocks: where it starts in the file, what it is, as `std::fs::read`, and
/// whether the code it's in may block.
#[derive(Debug, PartialEq, Eq)]
pub struct Call {
    pub line: usize,
    pub start: usize,
    pub what: String,
    pub standing: Standing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// In a function that takes a `Blocking`, or in the closure a maker lends one to.
    Lent,
    /// Anywhere else: the main thread's debt.
    Debt,
    /// `blocking::at_launch` outside the crate root, which no debt allows.
    Launch,
}

/// What a method's receiver must evidently be for a call to block.
#[derive(Clone, Copy)]
enum Receiver {
    /// A path, or anything not evidently something else.
    Path,
    /// A `std::process::Child`, or anything not evidently something else.
    Child,
    /// Evidently an executor: named for one, or a parameter typed as one.
    Executor,
    Any,
}

/// The methods that block, what their receiver must be, and whether they block only when
/// called with no argument.
const METHODS: [(&str, Receiver, bool); 14] = [
    ("canonicalize", Receiver::Path, true),
    ("exists", Receiver::Path, true),
    ("is_dir", Receiver::Path, true),
    ("is_file", Receiver::Path, true),
    ("metadata", Receiver::Path, true),
    ("read_dir", Receiver::Path, true),
    ("read_link", Receiver::Path, true),
    ("symlink_metadata", Receiver::Path, true),
    ("try_exists", Receiver::Path, true),
    ("wait", Receiver::Child, true),
    ("try_wait", Receiver::Child, true),
    ("block", Receiver::Executor, false),
    ("block_on", Receiver::Any, false),
    ("block_with_timeout", Receiver::Any, false),
];

/// Each call in one file's code that blocks the thread it runs on: a path into `std::fs`,
/// `std::os::unix::fs` or `std::os::unix::net`, or to `std::process::Command`, however the
/// file's `use` items name it, and a `pub use` of one; a `Path` probe, a child's wait, or an
/// executor's sync wait, called as a method or named by its path; `std::thread::park` and
/// `park_timeout`; and, unless `may_launch`, `blocking::at_launch`. Each with whether the code
/// around it may block.
pub fn of(code: &Code<'_>, may_launch: bool) -> Vec<Call> {
    let names = Names::of(&code.tokens);
    let lent = lent(code, &names);
    let tokens = &code.tokens;
    let standing = |token: &Token<'_>| {
        if token.scope.is_some_and(|scope| lent[scope]) {
            Standing::Lent
        } else {
            Standing::Debt
        }
    };
    let mut found: Vec<Call> = names
        .exported
        .iter()
        .map(|(token, what)| call(&tokens[*token], what.clone(), Standing::Debt))
        .collect();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        if let Some(&end) = names.declarations.get(&index) {
            index = end;
            continue;
        }
        if token.kind == "."
            && let Some(name) = method_at(code, &names, index)
        {
            found.push(call(name, format!(".{}()", name.text), standing(name)));
            index += 2;
            continue;
        }
        if !path_starts(tokens, index) {
            index += 1;
            continue;
        }
        let (segments, end) = path_at(tokens, index);
        let path = names.resolve(&segments);
        if is_launch(&path) {
            if !may_launch {
                found.push(call(token, path.join("::"), Standing::Launch));
            }
        } else if blocks(&path) {
            found.push(call(token, path.join("::"), standing(token)));
        }
        index = end;
    }
    found.extend(
        names
            .globs
            .iter()
            .map(|(token, what)| call(&tokens[*token], what.clone(), Standing::Debt)),
    );
    found.sort_by_key(|call| call.start);
    found
}

fn call(token: &Token<'_>, what: String, standing: Standing) -> Call {
    Call {
        line: token.line,
        start: token.start,
        what,
        standing,
    }
}

/// Whether the code of each scope may block: a function that takes a `Blocking`, a closure
/// a maker is handed or that takes one, and a closure in a scope that may.
fn lent(code: &Code<'_>, names: &Names) -> Vec<bool> {
    let mut lent: Vec<bool> = Vec::with_capacity(code.scopes.len());
    for scope in &code.scopes {
        let takes = scope.parameters.iter().any(|parameter| {
            let ty = names.resolve(&parameter.ty);
            matches!(
                ty.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
                [.., "blocking", "Blocking"]
            )
        });
        let may = match &scope.kind {
            Kind::Function => takes,
            Kind::Closure { callee } => {
                let callee = names.resolve(callee);
                let words: Vec<&str> = callee.iter().map(String::as_str).collect();
                let made =
                    matches!(words.as_slice(), [.., "blocking", maker] if MAKERS.contains(maker));
                let within = scope.parent.is_some_and(|parent| lent[parent]);
                takes || made || within
            }
        };
        lent.push(may);
    }
    lent
}

fn is_launch(path: &[String]) -> bool {
    matches!(
        path.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
        [.., "blocking", "at_launch"]
    )
}

/// Whether `path` names something that blocks: an item of `std::fs`, not the module alone,
/// which is a local's name as often as not; of `std::os::unix::fs` or `net`;
/// `std::process::Command`; a probe or wait named by its type; or a thread's park.
pub fn blocks(path: &[String]) -> bool {
    let words: Vec<&str> = path.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["std", "fs", _, ..]
        | ["std", "os", "unix", "fs" | "net", _, ..]
        | ["std", "process", "Command", ..]
        | ["std", "thread", "park" | "park_timeout"]
        | [.., "Child", "wait" | "try_wait"]
        | [.., "block_on" | "block_with_timeout"] => true,
        [.., "Path" | "PathBuf", probe] => METHODS
            .iter()
            .any(|(method, receiver, _)| method == probe && matches!(receiver, Receiver::Path)),
        _ => false,
    }
}

/// The method name after the `.` at `dot`, if a method that blocks is called there on a
/// receiver it blocks on.
fn method_at<'code, 'source>(
    code: &'code Code<'source>,
    names: &Names,
    dot: usize,
) -> Option<&'code Token<'source>> {
    let tokens = &code.tokens;
    let name = tokens.get(dot + 1).filter(|token| named(token))?;
    let (_, receiver, bare) = METHODS.iter().find(|(method, _, _)| *method == name.text)?;
    let opens = tokens.get(dot + 2).is_some_and(|next| next.kind == "(");
    let closes = tokens.get(dot + 3).is_some_and(|next| next.kind == ")");
    if !opens || (*bare && !closes) {
        return None;
    }
    let typed = typed_receiver(code, names, dot);
    let blocks = match receiver {
        Receiver::Path => typed.is_none_or(|ty| PATH_LIKE.contains(&ty.as_str())),
        Receiver::Child => typed.is_none_or(|ty| matches!(ty.as_str(), "Child" | "Box")),
        Receiver::Executor => {
            typed.is_some_and(|ty| ty.ends_with("Executor")) || named_executor(tokens, dot)
        }
        Receiver::Any => true,
    };
    blocks.then_some(name)
}

/// The last segment of the type of the receiver before the `.` at `dot`, where the receiver
/// is a plain name a parameter of the scopes around it binds with a type written out.
fn typed_receiver(code: &Code<'_>, names: &Names, dot: usize) -> Option<String> {
    let receiver = dot.checked_sub(1).and_then(|at| code.tokens.get(at))?;
    let before = dot.checked_sub(2).and_then(|at| code.tokens.get(at));
    if receiver.kind != "identifier" || before.is_some_and(|token| matches!(token.kind, "." | "::"))
    {
        return None;
    }
    let mut scope = receiver.scope;
    while let Some(index) = scope {
        let around = &code.scopes[index];
        if let Some(parameter) = around
            .parameters
            .iter()
            .find(|parameter| parameter.name == Some(receiver.text))
        {
            return names.resolve(&parameter.ty).pop();
        }
        if matches!(around.kind, Kind::Function) {
            return None;
        }
        scope = around.parent;
    }
    None
}

/// Whether the receiver before the `.` at `dot` is named for an executor, as `executor` or
/// `cx.background_executor()`.
fn named_executor(tokens: &[Token<'_>], dot: usize) -> bool {
    let kind_at = |at: usize| tokens.get(at).map(|token| token.kind);
    let called = (dot.checked_sub(2))
        .is_some_and(|open| kind_at(open) == Some("(") && kind_at(open + 1) == Some(")"));
    (dot.checked_sub(if called { 3 } else { 1 }))
        .and_then(|at| tokens.get(at))
        .is_some_and(|token| named(token) && token.text.ends_with("executor"))
}

pub fn named(token: &Token<'_>) -> bool {
    matches!(
        token.kind,
        "identifier" | "type_identifier" | "field_identifier" | "crate" | "self" | "super"
    )
}

/// Whether a path starts at `index`: a name not after a `.`, nor after a `::` that follows
/// another segment.
fn path_starts(tokens: &[Token<'_>], index: usize) -> bool {
    if !tokens.get(index).is_some_and(named) {
        return false;
    }
    let before = |back: usize| index.checked_sub(back).and_then(|at| tokens.get(at));
    match before(1).map(|token| token.kind) {
        Some(".") => false,
        Some("::") => !before(2).is_some_and(|token| named(token) || token.kind == ">"),
        _ => true,
    }
}

/// The segments of the path whose first name is at `index`, and the index after them.
fn path_at<'source>(tokens: &[Token<'source>], index: usize) -> (Vec<&'source str>, usize) {
    let mut segments: Vec<&str> = tokens
        .get(index)
        .map(|token| token.text)
        .into_iter()
        .collect();
    let mut end = index + 1;
    while tokens.get(end).is_some_and(|token| token.kind == "::")
        && let Some(next) = tokens.get(end + 1).filter(|token| named(token))
    {
        segments.push(next.text);
        end += 2;
    }
    (segments, end)
}
