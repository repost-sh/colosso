//! How long ago something was, in a few words, wherever Koyo says it: when an MCP client was last
//! used (`settings_view/mcp_clients.rs`), when an account's usage was read (`accounts`).

/// "just now", "3 min ago", "2 h ago", "4 days ago", for something `seconds` ago.
pub fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ages_read_in_a_few_words() {
        assert_eq!(super::ago(5), "just now");
        assert_eq!(super::ago(180), "3 min ago");
        assert_eq!(super::ago(7300), "2 h ago");
        assert_eq!(super::ago(4 * 86_400), "4 days ago");
    }
}
