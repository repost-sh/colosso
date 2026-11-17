use std::ops::Range;

use crate::language::Language;

pub fn may_hold_plain_comment(language: Language, source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut position = 0;
    while let Some(offset) = bytes[position..].iter().position(|&byte| byte == b'/') {
        let start = position + offset;
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|&&byte| byte == b'/')
                .count();
        if opens_plain_comment(language, bytes, start..end) {
            return true;
        }
        position = end;
    }
    false
}

fn opens_plain_comment(language: Language, bytes: &[u8], slashes: Range<usize>) -> bool {
    let after_star = slashes.start > 0 && bytes[slashes.start - 1] == b'*';
    let next = bytes.get(slashes.end).copied();
    match (slashes.len(), next) {
        (1, Some(b'*')) => !opens_doc_block(language, &bytes[slashes.end + 1..]),
        (1, _) => false,
        (2, Some(b'!')) | (3, _) if language == Language::Rust && !after_star => false,
        _ => true,
    }
}

fn opens_doc_block(language: Language, after_opening: &[u8]) -> bool {
    match (language, after_opening.first(), after_opening.get(1)) {
        (Language::Rust, Some(b'!'), _) => true,
        (_, Some(b'*'), Some(third)) => !matches!(third, b'*' | b'/'),
        _ => false,
    }
}
