//! A simulator of Koyo's own (`docs/devices.md` 4): found in Koyo's device set, its picture into
//! the ring, its input, its turn. Booting, shutting down and its apps go through `simctl`.

pub mod ax;
pub mod core;
pub(crate) mod display;
pub mod hid;
mod proxy;
pub mod simctl;
pub mod turn;
pub mod uhid;

use std::sync::mpsc;

use koyo_device_wire::{Edge, LogLevel, Orientation, Point, Size};

use self::core::{Device, Display};
use self::hid::Digitizer;
use self::uhid::Universal;
use crate::link::Teller;
use crate::places;
use crate::ring::Ring;
use crate::sys;
use crate::xpc::Event;

pub struct Simulator {
    pub udid: String,
    pub set: String,
    device: Device,
    display: Option<Display>,
    digitizer: Option<Digitizer>,
    universal: Option<Universal>,
    /// The places of the fingers down through the universal service, three or more.
    many: Option<Vec<(f64, f64)>>,
    /// Quarter turns clockwise that put the picture upright: how its interface faces, as its
    /// display says (`display.rs`), not how the device was told to face.
    quarters: u8,
    core_device: Option<(String, Vec<u64>)>,
    /// Send rights to the services asked often (which way it faces, every two seconds; a turn),
    /// each looked up once: a lookup is a send right more, never given back.
    ports: std::collections::HashMap<&'static str, u32>,
    pub scale: f64,
    teller: Teller,
}

impl Simulator {
    /// A send right to the service `name` inside the simulator.
    pub fn lookup(&self, name: &str) -> Result<u32, String> {
        self.device.lookup(name)
    }

    pub fn find(
        developer: &str,
        set: &str,
        udid: &str,
        teller: Teller,
    ) -> Result<Simulator, String> {
        let device = Device::find(developer, set, udid)?;
        let scale = device.scale();
        Ok(Simulator {
            udid: udid.to_owned(),
            set: set.to_owned(),
            device,
            display: None,
            digitizer: None,
            universal: None,
            many: None,
            quarters: 0,
            ports: std::collections::HashMap::new(),
            core_device: display::core_device_version(),
            scale,
            teller,
        })
    }

    pub fn booted(&self) -> bool {
        self.device.state() == core::BOOTED
    }

    pub fn booted_before(&self) -> bool {
        self.device.booted_before()
    }

    /// Its picture into `ring`: the surface now, and every frame from here on.
    pub fn attach(&mut self, ring: &Ring) -> Result<(), String> {
        let display = self.device.display()?;
        let surface = display
            .surface()
            .ok_or("the simulator's display has no picture yet")?;
        ring.mount(surface)?;
        let queue = sys::serial_queue(c"dev.koyo.device.frames", sys::QOS_USER_INTERACTIVE);
        let (frames, surfaces) = (ring.clone(), ring.clone());
        display.watch(
            queue,
            move || frames.frame(),
            move |replaced| {
                if let Err(error) = surfaces.mount(replaced) {
                    surfaces.log_error(&error);
                }
            },
        )?;
        self.display = Some(display);
        Ok(())
    }

    /// One try at its input: the answer to whether `dtuhidd` is there comes on the receiver.
    pub fn connect_input(&mut self) -> Result<mpsc::Receiver<Event>, String> {
        let port = self.device.lookup(hid::DIGITIZER)?;
        let (digitizer, answered) = Digitizer::connect(port)?;
        self.digitizer = Some(digitizer);
        Ok(answered)
    }

    /// Let go of a try at its input that didn't answer.
    pub fn drop_input(&mut self) {
        self.digitizer = None;
    }

    pub fn input_alive(&self) -> bool {
        self.digitizer.as_ref().is_some_and(Digitizer::alive)
    }

    /// Which way its interface faces, as the picture is turned upright.
    pub fn facing(&self) -> Orientation {
        places::facing(self.quarters)
    }

    /// Fingers at `at`, points of the upright picture of `size`, touching or lifting.
    pub fn touch(
        &mut self,
        at: &[Point],
        touching: bool,
        edge: Option<Edge>,
        size: Size,
    ) -> Result<(), String> {
        let quarters = self.quarters;
        let shares: Vec<(f64, f64)> = at
            .iter()
            .map(|p| places::on_surface(quarters, (p.x / size.width, p.y / size.height)))
            .collect();
        if shares.len() <= 2 {
            self.lift_many();
            let digitizer = self
                .digitizer
                .as_mut()
                .ok_or("the simulator's input isn't ready")?;
            digitizer.touch(&shares, !touching, edge.map_or(0, Edge::flag));
            return Ok(());
        }
        if let Some(digitizer) = &mut self.digitizer {
            digitizer.lift();
        }
        if self.universal.is_none() {
            let port = self.device.lookup(uhid::UNIVERSAL)?;
            let (universal, said) = Universal::connect(port)?;
            self.universal = Some(universal);
            self.log_info(&format!("three fingers and more: {said}"));
        }
        if let Some(universal) = &self.universal {
            universal.touch(&shares, touching);
        }
        self.many = touching.then_some(shares);
        Ok(())
    }

    fn lift_many(&mut self) {
        if let (Some(many), Some(universal)) = (self.many.take(), &self.universal) {
            universal.touch(&many, false);
        }
    }

    /// Lifts every finger, wherever it is.
    pub fn lift(&mut self) {
        self.lift_many();
        if let Some(digitizer) = &mut self.digitizer {
            digitizer.lift();
        }
    }

    pub fn key(&self, usage: u16, down: bool) {
        if let Some(digitizer) = &self.digitizer {
            digitizer.key(usage, down);
        }
    }

    pub fn button(&self, page: u32, usage: u32, down: bool) {
        if let Some(digitizer) = &self.digitizer {
            digitizer.button(page, usage, down);
        }
    }

    /// Turns the device to face `to`. Its interface follows if the app in front turns that way;
    /// the host then reads how it faces ([`Simulator::follow`]) and turns the picture by that.
    pub fn turn(&mut self, to: Orientation) -> Result<(), String> {
        const WORKSPACE: &str = "PurpleWorkspacePort";
        let port = self.port(WORKSPACE)?;
        let sent = turn::send(port, to);
        if sent != 0 {
            self.ports.remove(WORKSPACE);
            return Err(format!("the turn wasn't sent: {sent:#x}"));
        }
        Ok(())
    }

    /// The send right to `name`, looked up the first time, or again after it failed.
    fn port(&mut self, name: &'static str) -> Result<u32, String> {
        if let Some(port) = self.ports.get(name) {
            return Ok(*port);
        }
        let port = self.device.lookup(name)?;
        self.ports.insert(name, port);
        Ok(port)
    }

    /// Reads which way its interface faces and turns the picture by it; whether it changed.
    pub fn follow(&mut self, ring: &Ring) -> Result<bool, String> {
        let port = self.port(display::SERVICE)?;
        let version = self
            .core_device
            .as_ref()
            .ok_or("CoreDevice's version isn't known: is Xcode 27 installed?")?;
        let quarters = display::ask(port, &self.udid, version).inspect_err(|_| {
            self.ports.remove(display::SERVICE);
        })?;
        if quarters == self.quarters {
            return Ok(false);
        }
        self.quarters = quarters;
        ring.turn(quarters)?;
        Ok(true)
    }

    pub fn detach(&mut self) {
        self.lift();
        if let Some(display) = self.display.take() {
            display.unwatch();
        }
        self.digitizer = None;
        self.universal = None;
    }

    fn log_info(&self, text: &str) {
        self.teller.log(LogLevel::Info, text);
    }
}
