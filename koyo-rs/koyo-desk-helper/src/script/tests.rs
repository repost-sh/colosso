//! Setup's and removal's plans, against the stand-in Mac.

use koyo_desk_wire::helper::SetupState;
use koyo_desk_wire::paths;

use super::Script;
use crate::remove::{self, Admin};
use crate::serve::allowed;
use crate::setup::{self, Setup};
use crate::work::Work;

fn mac_on(version: &str) -> Script {
    Script::default()
        .file("/etc/pf.conf", "anchor \"com.apple/*\"\nload anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"\n")
        .on("sw_vers -productVersion", 0, version)
        .on("sw_vers -buildVersion", 0, "25H1\n")
        .on("id -un 501", 0, "schulzf\n")
        .on("dscl . -read /Users/koyo-agents", 56, "")
        .on("dscl . -read /Groups/koyo-agents", 56, "")
        .on("dscl . -list /Groups PrimaryGroupID", 0, "staff 20\nkoyoagents 600\n")
        .on("pfctl -E", 0, "pf enabled\nToken : 42\n")
        .on("pfctl -s info", 0, "Status: Enabled for 0 days 00:00:01  Debug: Urgent\n")
        .on("pfctl -a com.apple/900.Koyo -sr", 0, "block drop in quick on ! lo0 proto tcp from any to any port = 5900\n")
        .on("launchctl print system/com.apple.screensharing", 113, "")
        .on("launchctl print gui/", 113, "")
}

fn fresh() -> Script {
    mac_on("26.7.2\n")
        .after(
            "sysadminctl -addUser",
            "dscl . -read /Users/koyo-agents UniqueID",
            0,
            "UniqueID: 510\n",
        )
        .after(
            "sysadminctl -addUser",
            "dscl . -read /Users/koyo-agents NFSHomeDirectory",
            0,
            "NFSHomeDirectory: /Users/koyo-agents\n",
        )
}

fn setup_for(person_uid: u32) -> Setup {
    Setup {
        person_uid,
        helper: "/build/koyo-desk-helper".into(),
        host: None,
        accept_risk: false,
    }
}

#[test]
fn an_old_macos_is_refused_before_anything_is_touched() {
    let mut mac = mac_on("26.4\n");
    let mut work = Work::new(&mut mac);
    let refused = setup::run(&mut work, &setup_for(501)).unwrap_err();
    assert!(refused.contains("CVE-2026-65400"), "{refused}");
    assert!(work.steps.is_empty());
    assert!(!mac.called("pfctl") && !mac.called("sysadminctl") && !mac.called("mkdir"));
}

#[test]
fn a_fresh_setup_puts_the_firewall_first_and_the_daemon_last() {
    let mut mac = fresh();
    let mut work = Work::new(&mut mac);
    setup::run(&mut work, &setup_for(501)).unwrap();
    assert!(mac.at("pfctl -a com.apple/900.Koyo -f") < mac.at("sysadminctl -addUser koyo-agents"));
    assert!(mac.at("sysadminctl -addUser") < mac.at("dscl . -authonly koyo-agents"));
    assert!(
        mac.at("dseditgroup -o create -r Koyo Agents -i 601 koyo-agents") < mac.at("sysadminctl")
    );
    assert!(
        mac.at(&format!("write {}", paths::STATE))
            < mac.at("launchctl bootstrap system /Library/LaunchDaemons")
    );
    // Off until a login needs it.
    assert!(
        mac.at("launchctl bootstrap system /Library/LaunchDaemons")
            < mac.at("launchctl bootout system/com.apple.screensharing")
    );
    for line in [
        "dscl . -create /Users/koyo-agents IsHidden 1",
        "dscl . -create /Users/koyo-agents PrimaryGroupID 601",
        "dseditgroup -o edit -d koyo-agents -t user staff",
        "chmod 750 /Users/koyo-agents",
        "dseditgroup -o edit -a koyo-agents -t user com.apple.access_screensharing",
        "launchctl disable user/510/com.apple.Siri.agent",
        "chmod +a user:schulzf allow list,search,add_file,add_subdirectory,delete_child",
    ] {
        assert!(mac.called(line), "never called: {line}");
    }
}

#[test]
fn the_password_goes_only_to_a_terminal_and_the_root_only_state() {
    let mut mac = fresh();
    setup::run(&mut Work::new(&mut mac), &setup_for(501)).unwrap();
    let password = mac.secrets.first().cloned().unwrap();
    assert_eq!(password.len(), 32);
    assert!(mac.log.iter().all(|line| !line.contains(&password)));
    let (state, mode) = mac.files.get(std::path::Path::new(paths::STATE)).unwrap();
    assert_eq!(*mode, 0o600);
    let state: SetupState = serde_json::from_slice(state).unwrap();
    assert_eq!((state.person_uid, state.uid, state.gid), (501, 510, 601));
    assert_eq!(state.password, password);
    assert_eq!(state.pf_token.as_deref(), Some("42"));
}

#[test]
fn an_account_koyo_didnt_make_is_left_alone() {
    let mut mac = mac_on("26.7.2\n").on(
        "dscl . -read /Users/koyo-agents UniqueID",
        0,
        "UniqueID: 777\n",
    );
    let refused = setup::run(&mut Work::new(&mut mac), &setup_for(501)).unwrap_err();
    assert!(refused.contains("Koyo didn't make it"), "{refused}");
    assert!(!mac.called("dscl . -create") && !mac.called("pfctl"));
}

#[test]
fn screen_sharing_already_on_is_left_to_the_person() {
    let mut mac = mac_on("26.7.2\n").on(
        "launchctl print system/com.apple.screensharing",
        0,
        "state = running",
    );
    let refused = setup::run(&mut Work::new(&mut mac), &setup_for(501)).unwrap_err();
    assert!(refused.contains("Screen Sharing is on"), "{refused}");
    assert!(!mac.called("sysadminctl"));
}

fn state() -> SetupState {
    SetupState {
        person_uid: 501,
        uid: 510,
        gid: 601,
        password: "x".into(),
        pf_token: Some("42".into()),
        macos: "26.7.2".into(),
        accepted_risk: false,
        made_helper_tools: true,
    }
}

#[test]
fn an_account_whose_processes_stay_is_never_deleted() {
    // Logged in, and its session won't go.
    let mut mac = Script::default().on("pgrep -U 510", 0, "123\n456\n");
    let refused = remove::run(&mut Work::new(&mut mac), &state(), None).unwrap_err();
    assert!(refused.contains("still there"), "{refused}");
    assert!(mac.called("launchctl bootout gui/510") && mac.called("pkill -9 -U 510"));
    assert!(!mac.called("sysadminctl") && !mac.called("dscl . -delete"));
    // Logged out, and its background agents won't go either.
    let mut mac =
        Script::default()
            .on("launchctl print gui/510", 113, "")
            .on("pgrep -U 510", 0, "123\n");
    let refused = remove::run(&mut Work::new(&mut mac), &state(), None).unwrap_err();
    assert!(refused.contains("processes of the account"), "{refused}");
    assert!(mac.called("launchctl bootout user/510"));
    assert!(!mac.called("sysadminctl"));
}

#[test]
fn background_agents_are_no_session() {
    let mut mac = Script::default().on("launchctl print gui/510", 113, "").on(
        "pgrep -U 510",
        0,
        "9382\n9388\n",
    );
    let mut work = Work::new(&mut mac);
    assert!(!crate::session::has_session(&mut work, 510));
    crate::session::end(&mut work, 510).unwrap();
    assert!(!mac.called("launchctl bootout") && !mac.called("pkill"));
}

#[test]
fn removal_checks_the_administrator_before_touching_anything() {
    let mut mac = Script::default().on(
        "dseditgroup -o checkmember",
        0,
        "no schulzf is NOT a member of admin\n",
    );
    let admin = Admin {
        name: "schulzf",
        password: "pw",
    };
    let refused = remove::run(&mut Work::new(&mut mac), &state(), Some(&admin)).unwrap_err();
    assert!(refused.contains("isn't an administrator"), "{refused}");
    assert!(!mac.called("launchctl") && !mac.called("pgrep"));
}

#[test]
fn removal_ends_the_session_then_deletes_the_record_with_the_administrator() {
    let mut mac = Script::default()
        .on("launchctl print gui/510", 113, "")
        .on(
            "dseditgroup -o checkmember",
            0,
            "yes schulzf is a member of admin\n",
        )
        .on(
            "dscl . -read /Users/koyo-agents UniqueID",
            0,
            "UniqueID: 510\n",
        )
        .on(
            "dscl . -read /Users/koyo-agents NFSHomeDirectory",
            0,
            "NFSHomeDirectory: /Users/koyo-agents\n",
        )
        .after(
            "dscl -u schulzf . -delete /Users/koyo-agents",
            "dscl . -read /Users/koyo-agents UniqueID",
            56,
            "",
        );
    let admin = Admin {
        name: "schulzf",
        password: "pw",
    };
    remove::run(&mut Work::new(&mut mac), &state(), Some(&admin)).unwrap();
    assert!(mac.at("dscl . -authonly schulzf") < mac.at("pgrep -U 510"));
    assert!(
        mac.at("dseditgroup -o edit -d koyo-agents -t user com.apple.access_screensharing")
            < mac.at("sysadminctl -deleteUser koyo-agents")
    );
    assert!(
        mac.at("sysadminctl -deleteUser") < mac.at("dscl -u schulzf . -delete /Users/koyo-agents")
    );
    for line in [
        "pfctl -X 42",
        "tccutil reset All dev.koyo.agent-desktop.host",
        "remove /var/db/com.apple.xpc.launchd/disabled.510.plist",
        &format!("remove {}", paths::SHARED),
        &format!("remove {}", paths::HELPER_BIN),
        "rmdir /Library/PrivilegedHelperTools",
    ] {
        assert!(mac.called(line), "never called: {line}");
    }
    assert!(mac.log.iter().all(|line| !line.contains("pw")));
}

#[test]
fn only_the_person_and_root_are_answered() {
    let state = state();
    assert!(allowed(501, &state));
    assert!(allowed(0, &state));
    assert!(
        !allowed(510, &state),
        "the agent account itself is never answered"
    );
    assert!(!allowed(502, &state));
}

#[test]
fn a_setup_stopped_after_the_account_is_made_finds_it_again() {
    let mut mac = fresh().on("dscl . -authonly koyo-agents", 1, "authentication failed");
    let refused = setup::run(&mut Work::new(&mut mac), &setup_for(501)).unwrap_err();
    assert!(refused.contains("doesn't log in"), "{refused}");
    // The account was recorded before its password was checked.
    assert!(mac.files.contains_key(std::path::Path::new(paths::STATE)));
    let made = mac
        .log
        .iter()
        .filter(|line| line.starts_with("sysadminctl -addUser"))
        .count();
    let mut again = mac.on("dscl . -authonly koyo-agents", 0, "");
    setup::run(&mut Work::new(&mut again), &setup_for(501)).unwrap();
    let made_again = again
        .log
        .iter()
        .filter(|line| line.starts_with("sysadminctl -addUser"))
        .count();
    assert_eq!(made, made_again, "the second run made no second account");
}

#[test]
fn a_login_waits_out_the_last_session() {
    use koyo_desk_wire::helper::{HelperAnswer, HelperAsk};
    use std::time::Duration;

    use crate::console::Session;
    use crate::serve::answer;

    // Ended twenty seconds ago: forty more.
    let mut mac = mac_on("27.2\n");
    mac.ages
        .insert(paths::LAST_END.into(), Duration::from_secs(20));
    let (answered, _) = answer(&mut mac, &mut state(), HelperAsk::PrepareLogin);
    assert!(
        matches!(
            answered,
            HelperAnswer::NotYet {
                wait_ms: 40_000,
                ..
            }
        ),
        "{answered:?}"
    );
    assert!(!mac.called("launchctl disable") && !mac.called("launchctl enable"));
    // Still listed by the kernel.
    let mut mac = mac_on("27.2\n");
    mac.sessions = vec![Session {
        uid: Some(510),
        on_console: false,
    }];
    let (answered, _) = answer(&mut mac, &mut state(), HelperAsk::PrepareLogin);
    assert!(
        matches!(answered, HelperAnswer::NotYet { .. }),
        "{answered:?}"
    );
    // Long enough ago: trimmed again, then the login.
    let mut mac = mac_on("27.2\n").after(
        "launchctl bootstrap system",
        "launchctl print system/com.apple.screensharing",
        0,
        "",
    );
    mac.ages
        .insert(paths::LAST_END.into(), Duration::from_secs(90));
    let (answered, _) = answer(&mut mac, &mut state(), HelperAsk::PrepareLogin);
    assert!(
        matches!(answered, HelperAnswer::Login { .. }),
        "{answered:?}"
    );
    assert!(mac.at("launchctl disable user/510/com.apple.Siri.agent") < mac.at("launchctl enable"));
}

#[test]
fn only_a_session_off_the_console_is_logged_out() {
    use crate::console::Session;
    use crate::session;

    let logout = "launchctl asuser 510";
    let off = |on_console: bool| {
        let mut mac = Script::default().after(logout, "launchctl print gui/510", 113, "");
        mac.sessions = vec![
            Session {
                uid: Some(501),
                on_console: !on_console,
            },
            Session {
                uid: Some(510),
                on_console,
            },
        ];
        mac
    };
    let mut mac = off(false);
    session::end(&mut Work::new(&mut mac), 510).unwrap();
    assert!(mac.called(logout) && !mac.called("launchctl bootout"));
    // On the console it's never logged out from here: booted out, as any other.
    let mut mac = off(true).after(
        "launchctl bootout gui/510",
        "launchctl print gui/510",
        113,
        "",
    );
    session::end(&mut Work::new(&mut mac), 510).unwrap();
    assert!(!mac.called(logout) && mac.called("launchctl bootout gui/510"));
    assert!(mac.called(&format!("write {}", paths::LAST_END)));
}
