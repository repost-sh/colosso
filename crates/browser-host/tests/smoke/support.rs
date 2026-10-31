//! What the smoke tests share: the engine to start, a session that keeps the host's rings
//! and hands its frames back, and the pixels read out of a frame.
use colosso_browser_ipc::client::{Connection, Incoming, Launch};
use colosso_browser_ipc::{AppRequest, HostEvent, PixelSize};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn launch(test: &str) -> Launch {
    let engine = PathBuf::from(
        std::env::var("COLOSSO_BROWSER_ENGINE")
            .expect("COLOSSO_BROWSER_ENGINE points at an installed engine"),
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(engine.join("engine.json")).expect("engine.json"))
            .expect("json");
    // Scratch inside the checkout, one root per test so they can run side by
    // side; the host inherits the working directory.
    let scratch = PathBuf::from(format!(
        "../../.superpowers/t/{}-{test}",
        std::process::id()
    ));
    Launch {
        executable: engine.join(manifest["host"].as_str().expect("host path")),
        profiles: scratch.join("profiles"),
    }
}

/// The BGRA pixel at the centre of a surface.
pub fn centre(surface: &IOSurfaceRef) -> [u8; 4] {
    pixels(surface, &[(surface.width() / 2, surface.height() / 2)])[0]
}

/// The BGRA pixels at `points` of a surface.
fn pixels(surface: &IOSurfaceRef, points: &[(usize, usize)]) -> Vec<[u8; 4]> {
    unsafe {
        surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let row = surface.bytes_per_row();
        let base = surface.base_address().as_ptr().cast::<u8>();
        let read = points
            .iter()
            .map(|(x, y)| {
                let p = base.add(y * row + x * 4);
                [*p, *p.add(1), *p.add(2), *p.add(3)]
            })
            .collect();
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        read
    }
}

/// What a frame showed: its centre, and a grid over the whole of it.
pub struct Seen {
    pub centre: [u8; 4],
    pub grid: Vec<[u8; 4]>,
}

impl Seen {
    fn of(surface: &IOSurfaceRef, size: PixelSize) -> Self {
        let (width, height) = (size.width as usize, size.height as usize);
        let points: Vec<_> = (0..5)
            .flat_map(|i| (0..5).map(move |j| (4 + i * (width - 8) / 4, 4 + j * (height - 8) / 4)))
            .collect();
        Self {
            centre: centre(surface),
            grid: pixels(surface, &points),
        }
    }
}

/// A running host and what it sent so far.
pub struct Session {
    pub host: Connection,
    incoming: std::sync::mpsc::Receiver<Incoming>,
    /// Each pane's rings by generation.
    rings: HashMap<(u64, u32), Vec<objc2_core_foundation::CFRetained<IOSurfaceRef>>>,
}

impl Session {
    pub fn start(launch: &Launch) -> Self {
        let (sink, incoming) = std::sync::mpsc::channel();
        let host = Connection::start(launch, move |message| sink.send(message).err().map(|e| e.0))
            .expect("the host starts and says hello");
        Self {
            host,
            incoming,
            rings: HashMap::new(),
        }
    }

    /// Waits for the event `want` accepts, keeping rings, handing frame
    /// slots back and failing on host errors meanwhile.
    pub fn wait(&mut self, what: &str, mut want: impl FnMut(&HostEvent, Option<&Seen>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let Ok(incoming) = self.incoming.recv_timeout(Duration::from_millis(500)) else {
                continue;
            };
            match incoming {
                Incoming::Ring(ring) => {
                    let surfaces = ring
                        .ports
                        .iter()
                        .map(|port| {
                            let surface = IOSurfaceRef::lookup_from_mach_port(*port)
                                .expect("surface from port");
                            colosso_browser_ipc::mach::release(*port);
                            surface
                        })
                        .collect();
                    self.rings.insert((ring.pane, ring.generation), surfaces);
                }
                Incoming::Event(event) => {
                    let mut seen = None;
                    if let HostEvent::Frame {
                        pane,
                        generation,
                        slot,
                        size,
                    } = &event
                    {
                        let surface = self
                            .rings
                            .get(&(*pane, *generation))
                            .and_then(|r| r.get(usize::from(*slot)))
                            .expect("the ring arrives before its frames are needed");
                        seen = Some(Seen::of(surface, *size));
                        self.host
                            .send(&AppRequest::Release {
                                pane: *pane,
                                generation: *generation,
                                slot: *slot,
                            })
                            .expect("release");
                    }
                    if want(&event, seen.as_ref()) {
                        return;
                    }
                    if let HostEvent::Error { pane, message } = &event {
                        panic!("host error on pane {pane:?}: {message}");
                    }
                }
                Incoming::Closed => panic!("the host went away"),
            }
        }
        panic!("timed out waiting for {what}");
    }
}

pub fn is_red(pixel: [u8; 4]) -> bool {
    // BGRA: blue, green, red, alpha.
    pixel[2] > 240 && pixel[1] < 16 && pixel[0] < 16
}
