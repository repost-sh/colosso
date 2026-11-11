//! What the Mac might have, for the tests and the off-screen pictures.

use super::{Found, System, Xcode};
use crate::devices::Simulator;
use crate::devices::simulators::Model;

/// Xcode 27.2 in use.
pub fn ready() -> Xcode {
    Xcode::Ready("27.2".into())
}

/// What the Mac has, up to the step that's missing.
pub fn found(xcode: Xcode, components: bool, system: bool) -> Found {
    Found {
        xcode,
        components,
        systems: (system.then(|| System {
            name: "iOS 27.2".into(),
            bytes: 8_315_434_484,
        }))
        .into_iter()
        .collect(),
        models: vec![
            Model::new("iPhone 18 Pro", false),
            Model::new("iPhone 17 Pro", false),
            Model::new("iPad Pro 13-inch (M5)", true),
        ],
    }
}

/// A simulator of Koyo's set, running.
pub fn booted(udid: &str) -> Simulator {
    Simulator {
        booted: true,
        booted_before: true,
        ..simulator(udid)
    }
}

/// A simulator of Koyo's set, never booted.
pub fn simulator(udid: &str) -> Simulator {
    Simulator {
        udid: udid.into(),
        name: "iPhone 18 Pro".into(),
        system: "iOS 27.2".into(),
        booted: false,
        booted_before: false,
        ipad: false,
    }
}
