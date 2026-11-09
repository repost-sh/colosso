//! The pf anchor that keeps Screen Sharing on the loopback interface (`docs/agent-desktop.md`
//! 5.2): ports 5900 and 3283 dropped on every interface but `lo0`. pf's rules don't survive a
//! restart, so the daemon applies them at each load, before Screen Sharing is ever on.

use koyo_desk_wire::paths;

use crate::work::{Outcome, Work};

/// The anchor's rules file, in the shared folder.
fn rules_file() -> String {
    format!("{}/pf-koyo.conf", paths::SHARED)
}

/// Whether the stock `/etc/pf.conf` loads `com.apple` anchors, under which ours sits.
pub fn conf_loads_anchors(work: &mut Work) -> bool {
    work.mac
        .read(std::path::Path::new("/etc/pf.conf"))
        .is_ok_and(|conf| String::from_utf8_lossy(&conf).contains("anchor \"com.apple/*\""))
}

/// Whether the anchor's rule is loaded and pf is enabled, so the rule is in force. The stock
/// `/etc/pf.conf`, loaded at boot (`com.apple.pfctl`), evaluates every `com.apple/*` anchor.
pub fn loaded(work: &mut Work) -> bool {
    let rules = work.run("/sbin/pfctl", &["-a", paths::PF_ANCHOR, "-sr"]);
    let info = work.run("/sbin/pfctl", &["-s", "info"]);
    rules.out.contains("block drop in quick")
        && rules.out.contains("5900")
        && info.out.contains("Status: Enabled")
}

/// Loads the rule and enables pf with a reference of ours. The reference's token, to release
/// it on removal.
pub fn apply(work: &mut Work) -> Outcome<Option<String>> {
    let file = rules_file();
    work.write(
        "writing the firewall rule",
        &file,
        paths::PF_RULE.as_bytes(),
        0o644,
    )?;
    work.must(
        "loading the firewall rule",
        "/sbin/pfctl",
        &["-a", paths::PF_ANCHOR, "-f", &file],
    )?;
    let enabled = work.must("enabling pf", "/sbin/pfctl", &["-E"])?;
    if !loaded(work) {
        return Err(format!(
            "the firewall rule didn't load into {}",
            paths::PF_ANCHOR
        ));
    }
    work.did(format!(
        "loaded the firewall anchor {}: ports 5900 and 3283 only on lo0",
        paths::PF_ANCHOR
    ));
    Ok(token(&format!("{}\n{}", enabled.out, enabled.err)))
}

/// `pfctl -E`'s token: "Token : 16030791349805928466".
fn token(said: &str) -> Option<String> {
    said.lines()
        .find_map(|line| line.trim().strip_prefix("Token"))
        .map(|rest| rest.trim_start_matches([' ', ':']).trim().to_owned())
        .filter(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()))
}

/// The rule flushed and our reference released.
pub fn release(work: &mut Work, token: Option<&str>) {
    work.run("/sbin/pfctl", &["-a", paths::PF_ANCHOR, "-F", "all"]);
    if let Some(token) = token {
        work.run("/sbin/pfctl", &["-X", token]);
    }
    work.run("/bin/rm", &["-f", &rules_file()]);
    work.did(format!(
        "flushed the firewall anchor {} and released pf",
        paths::PF_ANCHOR
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_read_from_what_pfctl_says() {
        let said = "No ALTQ support in kernel\nALTQ related functions disabled\npf enabled\nToken : 16030791349805928466\n";
        assert_eq!(token(said).as_deref(), Some("16030791349805928466"));
        assert_eq!(token("pf already enabled\n"), None);
    }
}
