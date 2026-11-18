use super::*;

fn agent(id: &str) -> Actor {
    Actor {
        id: id.into(),
        label: id.to_uppercase(),
    }
}

#[test]
fn an_agent_claims_a_free_page_and_takes_over_another_agents_only_by_saying_so() {
    let mut control = Control::default();
    assert_eq!(control.agent_claims(agent("a"), false, None), Ok(()));
    assert_eq!(control.agent(), Some(&agent("a")));
    assert_eq!(
        control.agent_claims(agent("b"), false, None),
        Err(Refused::ControlRequired { holder: agent("a") })
    );
    assert_eq!(
        control.agent_claims(agent("a"), false, None),
        Ok(()),
        "renewed"
    );
    assert_eq!(
        control.agent_claims(agent("b"), true, Some("rerunning".into())),
        Ok(())
    );
    assert_eq!(control.agent(), Some(&agent("b")));
    control.agent_releases(&agent("a"));
    assert_eq!(control.agent(), Some(&agent("b")), "not its lease to end");
    control.agent_releases(&agent("b"));
    assert_eq!(control.holder, None);
}

#[test]
fn the_person_takes_a_page_and_agents_stay_out_until_they_give_it_back() {
    let mut control = Control::default();
    control.agent_claims(agent("a"), false, None).ok();
    assert_eq!(control.person_takes(Mode::Implicit), Some(agent("a")));
    assert!(control.person_holds());
    assert_eq!(control.displaced, Some(agent("a")));
    assert!(matches!(
        control.agent_claims(agent("a"), true, None),
        Err(Refused::PersonHasControl {
            mode: Mode::Implicit,
            ..
        })
    ));
    assert_eq!(control.person_takes(Mode::Explicit), None, "already theirs");
    assert!(
        matches!(
            control.agent_claims(agent("a"), true, None),
            Err(Refused::PersonHasControl {
                mode: Mode::Explicit,
                ..
            })
        ),
        "asked for, a page they were using is theirs to keep"
    );
    assert_eq!(control.person_takes(Mode::Implicit), None);
    assert!(
        !control.person_using(),
        "and using it doesn't make it a page they're only using"
    );
    assert!(matches!(
        control.agent_claims(agent("b"), false, None),
        Err(Refused::PersonHasControl { .. })
    ));
    control.person_gives_back();
    assert_eq!(control.holder, None);
    assert_eq!(control.displaced, None);
    assert_eq!(control.agent_claims(agent("b"), false, None), Ok(()));
}

#[test]
fn a_lease_taken_again_after_the_person_had_the_page_is_another() {
    let mut control = Control::default();
    control.agent_claims(agent("a"), false, None).unwrap();
    let first = control.agent_lease().map(|(_, lease)| lease);
    control.agent_claims(agent("a"), false, None).unwrap();
    assert_eq!(control.agent_lease().map(|(_, lease)| lease), first, "renewed");
    control.person_takes(Mode::Implicit);
    control.person_gives_back();
    assert_eq!(control.agent_lease(), None);
    control.agent_claims(agent("a"), false, None).unwrap();
    let again = control.agent_lease().map(|(_, lease)| lease);
    assert_ne!(again, first, "taken again, it's another lease");
}

#[test]
fn a_page_nobody_held_is_the_persons_once_they_use_it() {
    let mut control = Control::default();
    assert_eq!(control.person_takes(Mode::Implicit), None);
    assert!(control.person_holds());
    assert!(control.person_using());
    assert_eq!(control.displaced, None, "nobody to name");
    assert!(control.agent_claims(agent("a"), false, None).is_err());
}
