use tree_sitter::Node;

/// Whether an `attribute_item` or `inner_attribute_item` leaves its item out of the shipped
/// build: a test attribute such as `#[test]` or `#[gpui::test]`, or a `cfg` that is false there.
pub fn left_out(item: Node<'_>, source: &str) -> bool {
    let Some(attribute) = attribute_of(item) else {
        return false;
    };
    let Some(path) = attribute.child(0) else {
        return false;
    };
    let path = &source[path.byte_range()];
    if path == "test" || path.ends_with("::test") {
        return true;
    }
    path == "cfg"
        && attribute
            .child_by_field_name("arguments")
            .is_some_and(|arguments| !predicates(arguments, source).into_iter().all(|holds| holds))
}

/// The file a `#[path = "..."]` attribute names.
pub fn path_value(item: Node<'_>, source: &str) -> Option<String> {
    let attribute = attribute_of(item)?;
    if &source[attribute.child(0)?.byte_range()] != "path" {
        return None;
    }
    let value = attribute.child_by_field_name("value")?;
    let mut cursor = value.walk();
    let content = value
        .children(&mut cursor)
        .find(|child| child.kind() == "string_content")
        .map_or("", |content| &source[content.byte_range()]);
    Some(content.to_owned())
}

fn attribute_of(item: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = item.walk();
    item.children(&mut cursor)
        .find(|child| child.kind() == "attribute")
}

/// Whether each predicate of a parenthesised list holds in the shipped build.
fn predicates(token_tree: Node<'_>, source: &str) -> Vec<bool> {
    let mut cursor = token_tree.walk();
    let tokens: Vec<Node<'_>> = token_tree.children(&mut cursor).collect();
    let inner = tokens
        .get(1..tokens.len().saturating_sub(1))
        .unwrap_or_default();
    inner
        .split(|token| token.kind() == ",")
        .filter(|group| !group.is_empty())
        .map(|group| holds(group, source))
        .collect()
}

/// Whether a `cfg` predicate holds in the shipped build: `test` and every `feature = "..."`
/// are off (the koyo crate has no default features), and every other option, such as
/// `target_os = "macos"` or `debug_assertions`, is on.
fn holds(group: &[Node<'_>], source: &str) -> bool {
    let text = |node: &Node<'_>| &source[node.byte_range()];
    match group {
        [option] => text(option) != "test",
        [option, equals, _] if equals.kind() == "=" => text(option) != "feature",
        [function, arguments] if arguments.kind() == "token_tree" => {
            let each = predicates(*arguments, source);
            match text(function) {
                "all" => each.into_iter().all(|holds| holds),
                "any" => each.into_iter().any(|holds| holds),
                "not" => !each.into_iter().all(|holds| holds),
                _ => true,
            }
        }
        _ => true,
    }
}
