pub fn is_horizontal_space(character: char) -> bool {
    character == ' ' || character == '\t'
}

pub fn line_start(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1)
}

pub fn alone_on_line(source: &str, offset: usize) -> bool {
    source[line_start(source, offset)..offset]
        .chars()
        .all(is_horizontal_space)
}

pub fn character_column(source: &str, offset: usize) -> usize {
    source[line_start(source, offset)..offset].chars().count() + 1
}
