//! The device a host drives (`docs/devices.md` 3): a simulator of Koyo's set, or a real iPhone
//! or iPad by cable. What both do (touches, keys, buttons, the picture) goes through here, and
//! what a real device refuses is said here once.

use koyo_device_wire::{Ask, Edge, Orientation, PixelSize, Point, Refusal, Screen, Size};

use crate::phone::Phone;
use crate::sim::Simulator;

pub enum Device {
    Simulator(Box<Simulator>),
    Phone(Phone),
}

/// What doesn't count as movement when telling whether the screen stands still.
#[derive(Clone, Copy, Debug)]
pub struct Stillness {
    /// How far a colour may move before a point counts as changed.
    pub tolerance: u8,
    /// How many sampled points may move on their own.
    pub few: usize,
}

/// A real device's stream is encoded afresh, so a still screen's pictures differ a little from
/// one to the next; and an iPad's Home Screen has a live Clock icon and clock widget, whose
/// hands move 1 to 11 sampled points of 88,236 (measured, M6).
const PHONE_STILLNESS: Stillness = Stillness { tolerance: 20, few: 16 };

impl Device {
    pub fn scale(&self) -> f64 {
        match self {
            Device::Simulator(sim) => sim.scale,
            Device::Phone(phone) => phone.scale,
        }
    }

    pub fn simulator(&self) -> Option<&Simulator> {
        match self {
            Device::Simulator(sim) => Some(sim),
            Device::Phone(_) => None,
        }
    }

    pub fn simulator_mut(&mut self) -> Option<&mut Simulator> {
        match self {
            Device::Simulator(sim) => Some(sim),
            Device::Phone(_) => None,
        }
    }

    /// Whether the device has an accessibility tree to read: a real device has only its text.
    pub fn has_tree(&self) -> bool {
        self.simulator().is_some()
    }

    /// A real device is locked: acts are refused until the person unlocks it.
    pub fn locked(&self) -> bool {
        matches!(self, Device::Phone(phone) if phone.locked())
    }

    /// A real device's lock may have just changed: acts wait until it's known (a key sent to a
    /// locked device goes to its passcode field).
    pub fn unsure(&self) -> bool {
        matches!(self, Device::Phone(phone) if phone.unsure())
    }

    /// A simulator's pixels are exact; see [`PHONE_STILLNESS`].
    pub fn stillness(&self) -> Stillness {
        match self {
            Device::Simulator(_) => Stillness { tolerance: 0, few: 0 },
            Device::Phone(_) => PHONE_STILLNESS,
        }
    }

    /// What the device refuses of `ask` before anything is tried, in words that say what to do
    /// (`koyo_device_wire::real::refuses`, which the app asks first too).
    pub fn refuses(&self, ask: &Ask) -> Option<Refusal> {
        self.simulator().map_or_else(|| koyo_device_wire::real::refuses(ask), |_| None)
    }

    /// The picture as shown: upright, in points of the device's scale.
    pub fn screen(&self, pixels: PixelSize) -> Screen {
        let scale = self.scale();
        Screen {
            size: Size {
                width: f64::from(pixels.width) / scale,
                height: f64::from(pixels.height) / scale,
            },
            scale,
            pixels,
            orientation: match self {
                Device::Simulator(sim) => sim.facing(),
                Device::Phone(phone) => phone.held(),
            },
            // A real device's isn't read yet: its picture stays square.
            corner: self.simulator().map_or(0., |sim| sim.corner),
        }
    }

    pub fn touch(&mut self, at: &[Point], touching: bool, edge: Option<Edge>, size: Size) -> Result<(), String> {
        match self {
            Device::Simulator(sim) => sim.touch(at, touching, edge, size),
            // A real device goes by where the finger is, not by an edge's flag.
            Device::Phone(phone) => phone.touch(at, touching, size),
        }
    }

    pub fn lift(&mut self) {
        match self {
            Device::Simulator(sim) => sim.lift(),
            Device::Phone(phone) => phone.lift(),
        }
    }

    pub fn key(&self, usage: u16, down: bool) {
        match self {
            Device::Simulator(sim) => sim.key(usage, down),
            Device::Phone(phone) => phone.key(usage, down),
        }
    }

    pub fn button(&self, page: u32, usage: u32, down: bool) {
        match self {
            Device::Simulator(sim) => sim.button(page, usage, down),
            Device::Phone(phone) => phone.button(page, usage, down),
        }
    }

    /// Turns a simulator; a real device's turn was refused before ([`Device::refuses`]).
    pub fn turn(&mut self, to: Orientation) -> Result<(), String> {
        match self {
            Device::Simulator(sim) => sim.turn(to),
            Device::Phone(_) => Ok(()),
        }
    }

    pub fn detach(&mut self) {
        match self {
            Device::Simulator(sim) => sim.detach(),
            Device::Phone(phone) => phone.detach(),
        }
    }
}
