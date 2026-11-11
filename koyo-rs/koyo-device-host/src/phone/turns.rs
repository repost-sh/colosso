//! The host's turn for a real device: what its session said, made into how the device stands.
//! Connected, its picture is turned upright by its LCD's own turn and the device is ready, or
//! locked, once the first picture is there; a turn by the person's hand turns the picture; the
//! device's lock pushes say when it locks and unlocks; unplugged, it's gone.

use koyo_device_wire::State;

use super::Event;
use crate::device::Device;
use crate::host::Host;
use crate::start::Phase;

impl Host {
    /// Takes what the device's session said.
    pub fn phone_on(&mut self) {
        let Device::Phone(phone) = &mut self.device else {
            return;
        };
        for event in phone.events() {
            match event {
                Event::Ready { display, .. } => {
                    self.turned(display.quarters);
                    self.phase = Phase::Ready;
                }
                Event::Display(display) => self.turned(display.quarters),
                Event::Locked(_) | Event::Noticed => {}
                Event::Unplugged(why) => {
                    self.gone = true;
                    self.tell_state(State::Gone { why });
                }
                Event::Ended(code, why) => self.fail(code, why),
            }
        }
        self.tell_standing();
    }

    fn turned(&mut self, quarters: u8) {
        if let Err(why) = self.ring.turn(quarters) {
            self.ring.log_error(&why);
        }
    }
}
