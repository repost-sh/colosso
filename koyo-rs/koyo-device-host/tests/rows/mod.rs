//! The real-simulator rows of the matrix (`.superpowers/devices/qa/MATRIX.md`): the picture here,
//! touches and keys in `touch.rs`, the system's gestures in `system.rs`.

mod system;
mod touch;
mod tree;
mod turned;

pub use system::system;
pub use touch::{keys, touches};
pub use tree::tree;
pub use turned::{turned, turned_tearing};

use std::time::{Duration, Instant};

use koyo_device_wire::{Screen, ToHost};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};

use crate::support::pages::open;
use crate::support::{Driver, Pages, Target, row};

/// The pixel at shares `(x, y)` of `surface`, as red, green, blue.
pub fn pixel(surface: &IOSurfaceRef, x: f64, y: f64) -> (u8, u8, u8) {
    // SAFETY: locked for reading, the base address holds `height` rows of `bytes_per_row`.
    unsafe {
        surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let (w, h, stride) = (surface.width(), surface.height(), surface.bytes_per_row());
        let (px, py) = (((w - 1) as f64 * x) as usize, ((h - 1) as f64 * y) as usize);
        let base = surface
            .base_address()
            .as_ptr()
            .cast::<u8>()
            .add(py * stride + px * 4);
        let bgra = std::slice::from_raw_parts(base, 4);
        let got = (bgra[2], bgra[1], bgra[0]);
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        got
    }
}

/// Whether a column of `surface` from 30% to 70% down its middle holds two colours: a frame
/// caught half written, on a page that is one colour all over each frame.
pub fn mixed(surface: &IOSurfaceRef) -> bool {
    // SAFETY: as `pixel`.
    unsafe {
        surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let (w, h, stride) = (surface.width(), surface.height(), surface.bytes_per_row());
        let base = surface.base_address().as_ptr().cast::<u8>();
        let at = |y: usize| {
            base.add(y * stride + (w / 2) * 4)
                .cast::<u32>()
                .read_unaligned()
                & 0x00FF_FFFF
        };
        let first = at(h * 3 / 10);
        let two = (h * 3 / 10..h * 7 / 10).step_by(4).any(|y| at(y) != first);
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        two
    }
}

fn top_cpu(pid: u32) -> f64 {
    let out = std::process::Command::new("/usr/bin/top")
        .args([
            "-l",
            "2",
            "-s",
            "3",
            "-stats",
            "cpu",
            "-pid",
            &pid.to_string(),
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .rev()
        .find_map(|line| line.trim().parse::<f64>().ok())
        .unwrap_or(-1.)
}

/// The host's own count of frames presented and copied, its last two lines.
fn stats(host: &mut Driver, when: &str) {
    let _ = host.until(Duration::from_secs(6), |_| false);
    let lines: Vec<&String> = host
        .logs
        .iter()
        .filter(|line| line.starts_with("frames:"))
        .collect();
    for line in lines.iter().rev().take(2).rev() {
        eprintln!("stats {when}: {line}");
    }
}

pub fn picture(target: &Target, pages: &Pages, host: &mut Driver, screen: &Screen) {
    open(target, pages, "flash.html", "ready-flash");
    let rate = host.count_frames(Duration::from_secs(3)) as f64 / 3.;
    stats(host, "animating");
    let format = host
        .shown()
        .map(|s| s.pixel_format() == u32::from_be_bytes(*b"BGRA"));
    row(
        "QS1",
        "frames a second while a page animates",
        &format!("{rate:.1}, BGRA {format:?}"),
        rate >= 55. && format == Some(true),
    );
    let cpu = top_cpu(host.pid);
    row(
        "QG3",
        "host CPU at 60 frames a second (PoC 2.1-2.4% with the ring)",
        &format!("{cpu:.1}%"),
        cpu >= 0.,
    );
    // Tearing: what the app draws, read as fast as it goes, the newest slot held as the app holds it.
    let (mut reads, mut torn) = (0u64, 0u64);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        let _ = host.until(Duration::ZERO, |_| false);
        if let Some(surface) = host.shown() {
            reads += 1;
            torn += u64::from(mixed(&surface));
        }
    }
    row(
        "QS3",
        "reads of the shown slot holding two frames",
        &format!("{torn} of {reads}"),
        torn == 0 && reads >= 30_000,
    );
    open(target, pages, "colours.html", "ready-colours");
    let _ = host.until(Duration::from_millis(500), |_| false);
    host.drain();
    let shown = host.shown().unwrap();
    // The page's bands are where Safari puts its view: each colour is looked for down the middle.
    let column: Vec<(u8, u8, u8)> = (0..400)
        .map(|i| pixel(&shown, 0.5, f64::from(i) / 400.))
        .collect();
    let wanted = [
        (255, 0, 0),
        (0, 255, 0),
        (0, 0, 255),
        (128, 128, 128),
        (255, 255, 255),
    ];
    let mut worst = 0u8;
    let mut read = Vec::new();
    for (r, g, b) in wanted {
        let off = |p: &(u8, u8, u8)| p.0.abs_diff(r).max(p.1.abs_diff(g)).max(p.2.abs_diff(b));
        let nearest = column
            .iter()
            .min_by_key(|p| off(p))
            .copied()
            .unwrap_or_default();
        worst = worst.max(off(&nearest));
        read.push(format!("{nearest:?}"));
    }
    row(
        "QS5",
        "known sRGB colours read back from the ring (U1)",
        &format!("{}; worst {worst} off", read.join(" ")),
        worst <= 2,
    );
    drop(shown);
    open(target, pages, "lab.html", "ready");
    let still = host.count_frames(Duration::from_secs(3));
    stats(host, "still");
    let cpu = top_cpu(host.pid);
    row(
        "QS2",
        "a still screen: frames in 3 s, host CPU",
        &format!("{still} frames, {cpu:.1}%"),
        still <= 1 && cpu < 1.,
    );
    full_ring(host, screen);
}

/// QS8: every slot held by the app while the picture changes, then let go: the newest frame
/// still comes, though the screen has gone still.
fn full_ring(host: &mut Driver, screen: &Screen) {
    let Some((generation, _)) = host.latest else {
        return row("QS8", "a full ring", "no frame yet", false);
    };
    host.holds = false;
    let slots: Vec<_> = (0..3)
        .filter_map(|slot| host.surface(generation, slot))
        .collect();
    for surface in &slots {
        surface.increment_use_count();
    }
    let before = host.frames;
    host.input(koyo_device_wire::Input::Touches {
        fingers: vec![koyo_device_wire::Finger {
            id: 0,
            at: koyo_device_wire::Point {
                x: screen.size.width / 2.,
                y: screen.size.height / 2.,
            },
            down: true,
        }],
        edge: None,
    });
    host.input(koyo_device_wire::Input::Touches {
        fingers: vec![koyo_device_wire::Finger {
            id: 0,
            at: koyo_device_wire::Point {
                x: screen.size.width / 2.,
                y: screen.size.height / 2.,
            },
            down: false,
        }],
        edge: None,
    });
    let held = host.count_frames(Duration::from_secs(1));
    for surface in &slots {
        surface.decrement_use_count();
    }
    let started = Instant::now();
    let came = host.until(Duration::from_secs(1), |m| {
        matches!(m, koyo_device_wire::FromHost::Frame { .. })
    });
    let after = started.elapsed();
    host.holds = true;
    row(
        "QS8",
        "a full ring: frames while all held, then the owed one once let go",
        &format!(
            "{held} while held (from {before}), owed one {} in {} ms",
            if came.is_some() { "came" } else { "never came" },
            after.as_millis()
        ),
        held == 0 && came.is_some(),
    );
}

/// QS7: not drawn and no agent at work, nothing is copied; drawn again, the picture comes at once.
pub fn unseen(target: &Target, pages: &Pages, host: &mut Driver) {
    open(target, pages, "flash.html", "ready-flash");
    host.tell(&ToHost::Visible { visible: false });
    // An agent's last answer keeps the picture current for ten seconds.
    let _ = host.until(Duration::from_secs(11), |_| false);
    let hidden = host.count_frames(Duration::from_secs(3));
    host.tell(&ToHost::Visible { visible: true });
    let started = Instant::now();
    let came = host.until(Duration::from_secs(1), |m| {
        matches!(m, koyo_device_wire::FromHost::Frame { .. })
    });
    let first = started.elapsed();
    row(
        "QS7",
        "unseen while animating: frames in 3 s; seen again: the first frame",
        &format!("{hidden}; {} ms", first.as_millis()),
        hidden == 0 && came.is_some() && first < Duration::from_millis(100),
    );
    open(target, pages, "lab.html", "ready");
    host.tell(&ToHost::Visible { visible: false });
    let _ = host.until(Duration::from_secs(11), |_| false);
    host.tell(&ToHost::Visible { visible: true });
    let started = Instant::now();
    let came = host.until(Duration::from_secs(1), |m| {
        matches!(m, koyo_device_wire::FromHost::Frame { .. })
    });
    let first = started.elapsed();
    row(
        "QS7",
        "a still screen, seen again: the first frame",
        &format!("{} ms", first.as_millis()),
        came.is_some() && first < Duration::from_millis(100),
    );
}
