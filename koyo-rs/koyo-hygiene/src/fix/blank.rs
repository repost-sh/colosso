pub fn tidy(text: &str, seams: &[usize]) -> String {
    if seams.is_empty() {
        return text.to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut pending_seams = seams.iter().peekable();
    let mut seam_before_line = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        while pending_seams.next_if(|&&seam| seam <= offset).is_some() {
            seam_before_line = true;
        }
        offset += line.len();
        if is_blank(line) {
            let previous = kept.last().copied();
            let unwanted = previous.is_none_or(|above| is_blank(above) || opens_group(above));
            if seam_before_line && unwanted {
                continue;
            }
        } else if seam_before_line && closes_group(line) {
            drop_trailing_blanks(&mut kept);
        }
        kept.push(line);
        seam_before_line = false;
    }
    if seam_before_line || pending_seams.next().is_some() {
        drop_trailing_blanks(&mut kept);
    }
    kept.concat()
}

fn drop_trailing_blanks(kept: &mut Vec<&str>) {
    while kept.last().is_some_and(|line| is_blank(line)) {
        kept.pop();
    }
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn opens_group(line: &str) -> bool {
    line.trim_end().ends_with(['{', '(', '['])
}

fn closes_group(line: &str) -> bool {
    line.trim_start().starts_with(['}', ')', ']'])
}
