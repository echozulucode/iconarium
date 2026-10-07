//! Deterministic dataset generator for performance and stress testing (plan §33).
//!
//! ```text
//! cargo run -p svg-core --release --example gen_dataset -- <A|B|C|D|E|all> <out_dir> [--seed N]
//! cargo run -p svg-core --release --example gen_dataset -- hash <dataset_dir>
//! ```
//!
//! Each dataset is written to `<out_dir>/<letter>/` (an existing generated dataset there is
//! replaced; a non-empty directory without the `.svgb-dataset` marker is refused).
//!
//! | Set | Content |
//! |-----|---------|
//! | A   | 1,000 varied icons in a nested folder tree |
//! | B   | 10,000 icons |
//! | C   | 50,000 icons |
//! | D   | 10,000 engineering diagrams with significant `<text>` (labels, titles, notes) |
//! | E   | pathological files (limits, malformed, encodings, Unicode, scripts, ...) |
//!
//! A–D also contain hidden directories/files (`.git/`, `.cache/`, `.backup/`, `.name.svg`)
//! that discovery must skip, and non-SVG decoys (`.svgz`, `.svg.bak`, `.png`, `README.md`).
//! The visible `.svg` count is exactly the dataset size.
//!
//! Output is byte-identical for a given seed on every platform: the PRNG is splitmix64,
//! file content depends only on (seed, file index), and no libm transcendental functions
//! are used. `hash` prints a BLAKE3 digest over (relative path, length, content) of every
//! file in sorted order so two generations can be compared.
//!
//! This file is also compiled into `tests/pathological.rs` (via `#[path]`) so the test
//! suite exercises exactly the cases the dataset contains, at reduced sizes.

#![allow(dead_code)]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use svg_core::config::{Limits, MB};

pub const DEFAULT_SEED: u64 = 0x5EED_2026;
/// Marker file written at a dataset root (hidden, so discovery ignores it).
pub const MARKER: &str = ".svgb-dataset";

// ============================================================================ PRNG

/// splitmix64 — tiny, fast, and identical on every platform.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }
    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn frange(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

/// Independent sub-seed for (seed, a, b).
pub fn derive(seed: u64, a: u64, b: u64) -> u64 {
    let mut r = Rng::new(
        seed ^ a.wrapping_mul(0xA24B_AED4_963E_E407) ^ b.wrapping_mul(0x9FB2_1C65_1E98_DF25),
    );
    r.next_u64();
    r.next_u64()
}

// ============================================================================ helpers

const PI: f64 = std::f64::consts::PI;

/// Deterministic sine (Taylor series; only IEEE basic ops, so identical everywhere).
fn dsin(x: f64) -> f64 {
    let tau = 2.0 * PI;
    let mut x = x % tau;
    if x > PI {
        x -= tau;
    } else if x < -PI {
        x += tau;
    }
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for k in 1..10 {
        term *= -x2 / ((2 * k) as f64 * (2 * k + 1) as f64);
        sum += term;
    }
    sum
}
fn dcos(x: f64) -> f64 {
    dsin(x + PI / 2.0)
}

/// Append a number rounded to one decimal, without trailing ".0" or "-0".
fn num(out: &mut String, v: f64) {
    let t = (v * 10.0).round() as i64;
    if t < 0 {
        out.push('-');
    }
    let a = t.unsigned_abs();
    let _ = write!(out, "{}", a / 10);
    if !a.is_multiple_of(10) {
        let _ = write!(out, ".{}", a % 10);
    }
}
fn n(v: f64) -> String {
    let mut s = String::new();
    num(&mut s, v);
    s
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c.to_ascii_lowercase());
        } else if !o.ends_with('-') && !o.is_empty() {
            o.push('-');
        }
    }
    o.trim_end_matches('-').to_string()
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> io::Result<u64> {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&p, bytes)?;
    Ok(bytes.len() as u64)
}

fn create(root: &Path, rel: &str) -> io::Result<BufWriter<File>> {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(BufWriter::with_capacity(1 << 20, File::create(p)?))
}

// ---------------------------------------------------------------- CRC32 / PNG / gzip

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC_TABLE: [u32; 256] = crc_table();

/// Running CRC32 state (pre/post-inverted by the caller).
fn crc_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

/// Writer adapter that tracks CRC32 and Adler-32 of what passes through.
struct Sums<W: Write> {
    inner: W,
    crc: u32,
}
impl<W: Write> Write for Sums<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc = crc_update(self.crc, &buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Write deflate *stored* blocks for `total` bytes produced by `fill` (called with a
/// buffer to fill completely, in order). Returns the Adler-32 of the raw data.
fn write_stored_deflate(
    w: &mut impl Write,
    total: u64,
    mut fill: impl FnMut(&mut [u8]),
) -> io::Result<(u32, u32)> {
    const BLOCK: u64 = 65_535;
    let mut remaining = total;
    let (mut a, mut b) = (1u32, 0u32);
    let mut crc = 0xFFFF_FFFFu32;
    let mut buf = vec![0u8; BLOCK as usize];
    loop {
        let len = remaining.min(BLOCK) as usize;
        let last = remaining <= BLOCK;
        w.write_all(&[u8::from(last)])?;
        w.write_all(&(len as u16).to_le_bytes())?;
        w.write_all(&(!(len as u16)).to_le_bytes())?;
        let chunk = &mut buf[..len];
        fill(chunk);
        for &x in chunk.iter() {
            a = (a + x as u32) % 65_521;
            b = (b + a) % 65_521;
        }
        crc = crc_update(crc, chunk);
        w.write_all(chunk)?;
        remaining -= len as u64;
        if last {
            break;
        }
    }
    Ok(((b << 16) | a, !crc))
}

fn stored_deflate_len(total: u64) -> u64 {
    let blocks = total.div_ceil(65_535).max(1);
    total + 5 * blocks
}

/// Stream an uncompressed RGBA PNG (`pixel(x, y)` → RGBA) to `w`.
pub fn write_png(
    w: &mut impl Write,
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> io::Result<()> {
    w.write_all(b"\x89PNG\r\n\x1a\n")?;
    let chunk = |w: &mut dyn Write, ty: &[u8; 4], data: &[u8]| -> io::Result<()> {
        w.write_all(&(data.len() as u32).to_be_bytes())?;
        w.write_all(ty)?;
        w.write_all(data)?;
        let crc = !crc_update(crc_update(0xFFFF_FFFF, ty), data);
        w.write_all(&crc.to_be_bytes())
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(w, b"IHDR", &ihdr)?;

    let row = 1 + 4 * width as u64;
    let raw = row * height as u64;
    let idat_len = 2 + stored_deflate_len(raw) + 4;
    w.write_all(&(idat_len as u32).to_be_bytes())?;
    let mut s = Sums {
        inner: &mut *w,
        crc: crc_update(0xFFFF_FFFF, b"IDAT"),
    };
    s.write_all(&[0x78, 0x01])?;
    // Raw scanlines: filter byte 0 then RGBA pixels.
    let mut pos: u64 = 0;
    let (adler, _) = write_stored_deflate(&mut s, raw, |buf| {
        for byte in buf.iter_mut() {
            let in_row = pos % row;
            *byte = if in_row == 0 {
                0
            } else {
                let px = ((in_row - 1) / 4) as u32;
                let y = (pos / row) as u32;
                pixel(px, y)[((in_row - 1) % 4) as usize]
            };
            pos += 1;
        }
    })?;
    s.write_all(&adler.to_be_bytes())?;
    let crc = !s.crc;
    w.write_all(&crc.to_be_bytes())?;
    chunk(w, b"IEND", &[])
}

fn png_bytes(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut v = Vec::new();
    write_png(&mut v, width, height, pixel).expect("in-memory write");
    v
}

fn sample_png() -> Vec<u8> {
    png_bytes(16, 16, |x, y| {
        let on = (x / 4 + y / 4) % 2 == 0;
        if on {
            [0x1E, 0x88, 0xE5, 0xFF]
        } else {
            [0xFF, 0xB3, 0x00, 0xFF]
        }
    })
}

/// gzip container with stored deflate blocks (valid, uncompressed).
pub fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut v = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
    let mut i = 0usize;
    let (_, crc) = write_stored_deflate(&mut v, data.len() as u64, |buf| {
        buf.copy_from_slice(&data[i..i + buf.len()]);
        i += buf.len();
    })
    .expect("in-memory write");
    v.extend_from_slice(&crc.to_le_bytes());
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v
}

/// Streaming base64 encoder.
struct B64<W: Write> {
    inner: W,
    pending: [u8; 3],
    n: usize,
    out: Vec<u8>,
}
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
impl<W: Write> B64<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            pending: [0; 3],
            n: 0,
            out: Vec::with_capacity(1 << 16),
        }
    }
    fn emit(&mut self, b: [u8; 3], len: usize) {
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let a = B64_ALPHABET;
        self.out.push(a[(v >> 18) as usize & 63]);
        self.out.push(a[(v >> 12) as usize & 63]);
        self.out.push(if len > 1 {
            a[(v >> 6) as usize & 63]
        } else {
            b'='
        });
        self.out
            .push(if len > 2 { a[v as usize & 63] } else { b'=' });
    }
    fn finish(mut self) -> io::Result<W> {
        if self.n > 0 {
            let mut b = self.pending;
            for x in b.iter_mut().skip(self.n) {
                *x = 0;
            }
            self.emit(b, self.n);
        }
        self.inner.write_all(&self.out)?;
        Ok(self.inner)
    }
}
impl<W: Write> Write for B64<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &x in buf {
            self.pending[self.n] = x;
            self.n += 1;
            if self.n == 3 {
                let p = self.pending;
                self.emit(p, 3);
                self.n = 0;
            }
        }
        if self.out.len() >= 1 << 16 {
            self.inner.write_all(&self.out)?;
            self.out.clear();
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn b64(data: &[u8]) -> String {
    let mut v = Vec::new();
    let mut e = B64::new(&mut v);
    e.write_all(data).unwrap();
    e.finish().unwrap();
    String::from_utf8(v).unwrap()
}

// ============================================================================ vocabulary

const COLORS: &[&str] = &[
    "#1e88e5",
    "#43a047",
    "#e53935",
    "#fb8c00",
    "#8e24aa",
    "#00897b",
    "#3949ab",
    "#6d4c41",
    "#546e7a",
    "#fdd835",
    "#d81b60",
    "#00acc1",
    "#7cb342",
    "#222",
    "#555",
    "#999",
    "#fff",
    "currentColor",
    "black",
    "white",
    "steelblue",
    "darkorange",
    "seagreen",
    "crimson",
];
const NOUNS: &[&str] = &[
    "ethernet",
    "switch",
    "router",
    "motor",
    "controller",
    "pump",
    "valve",
    "sensor",
    "relay",
    "breaker",
    "fuse",
    "transformer",
    "inverter",
    "battery",
    "panel",
    "gateway",
    "firewall",
    "server",
    "cable",
    "port",
    "fan",
    "heater",
    "cooler",
    "tank",
    "pipe",
    "flange",
    "gear",
    "shaft",
    "bearing",
    "arrow",
    "circle",
    "square",
    "star",
    "logo",
    "icon",
    "plc",
    "hmi",
    "drive",
    "encoder",
    "antenna",
    "cloud",
    "database",
    "user",
    "lock",
    "key",
    "bell",
    "gauge",
    "thermometer",
    "meter",
    "compressor",
    "turbine",
    "generator",
    "busbar",
    "terminal",
    "socket",
    "plug",
    "chip",
    "cpu",
    "memory",
    "disk",
    "folder",
    "file",
    "chart",
    "home",
    "settings",
    "search",
    "filter",
    "warning",
    "alarm",
    "check",
    "close",
    "menu",
    "play",
    "pause",
    "stop",
];
const QUALIFIERS: &[&str] = &[
    "outline",
    "filled",
    "small",
    "large",
    "left",
    "right",
    "up",
    "down",
    "red",
    "blue",
    "green",
    "alt",
    "bold",
    "thin",
    "round",
    "sharp",
    "mini",
    "2x",
    "dark",
    "light",
    "v2",
    "old",
    "new",
    "active",
    "disabled",
    "hover",
    "industrial",
    "managed",
    "redundant",
];
const SIZES: &[&str] = &["12", "16", "20", "24", "32", "48", "64", "128"];

type Tree = &'static [(&'static str, &'static [&'static str])];

const ICON_TREE: Tree = &[
    (
        "electrical",
        &[
            "symbols",
            "breakers",
            "transformers",
            "motors",
            "relays",
            "one-line",
            "panels",
            "switchgear",
        ],
    ),
    (
        "network",
        &[
            "switches",
            "routers",
            "firewalls",
            "servers",
            "wireless",
            "cabling",
            "topology",
        ],
    ),
    (
        "controls",
        &[
            "plc",
            "hmi",
            "sensors",
            "actuators",
            "drives",
            "io-modules",
            "safety",
        ],
    ),
    (
        "c4",
        &[
            "context",
            "containers",
            "components",
            "people",
            "systems",
            "deployment",
        ],
    ),
    (
        "ui",
        &[
            "icons",
            "buttons",
            "navigation",
            "status",
            "toolbar",
            "avatars",
        ],
    ),
    (
        "mechanical",
        &["gears", "bearings", "fasteners", "couplings", "shafts"],
    ),
    (
        "process",
        &[
            "tanks",
            "pumps",
            "valves",
            "instruments",
            "piping",
            "heat-exchangers",
        ],
    ),
    (
        "hydraulics",
        &["cylinders", "pumps", "valves", "accumulators"],
    ),
    ("logos", &["vendors", "internal", "partners", "archive"]),
    (
        "Legacy Assets",
        &["v1", "v2", "Old Style", "Copy of icons", "2019", "2021"],
    ),
];
const DIAGRAM_TREE: Tree = &[
    (
        "diagrams",
        &["network", "power", "process", "safety", "architecture"],
    ),
    ("c4", &["context", "containers", "components", "deployment"]),
    ("controls", &["plc", "io", "loops", "alarms"]),
    ("electrical", &["one-line", "panels", "schedules"]),
    ("projects", &["site-a", "site-b", "phase-2", "as-built"]),
];
const DEEPER: &[&str] = &[
    "16",
    "24",
    "32",
    "48",
    "outline",
    "filled",
    "duotone",
    "light",
    "dark",
    "v2",
    "v3",
    "archive",
    "2024",
    "2025",
    "export",
    "final",
    "draft",
    "large",
    "small",
    "misc",
    "color",
    "mono",
    "rev-b",
    "customer-x",
    "shared",
];

const NODE_TYPES: &[(&str, &str, &str)] = &[
    ("plc", "PLC", "PLC"),
    ("switch", "Ethernet Switch", "SW"),
    ("motor", "Motor Controller", "MC"),
    ("pump", "Pump", "P"),
    ("valve", "Control Valve", "CV"),
    ("hmi", "HMI Panel", "HMI"),
    ("scada", "Supervisory Server", "SCADA"),
    ("vfd", "Variable Frequency Drive", "VFD"),
    ("pt", "Pressure Transmitter", "PT"),
    ("rtu", "Remote Terminal Unit", "RTU"),
    ("fw", "Firewall", "FW"),
    ("hist", "Historian", "HIS"),
    ("cb", "Circuit Breaker", "CB"),
    ("xfmr", "Transformer", "TX"),
    ("ups", "UPS", "UPS"),
    ("gw", "Protocol Gateway", "GW"),
    ("io", "Remote I/O Rack", "RIO"),
    ("ft", "Flow Meter", "FT"),
    ("tank", "Storage Tank", "TK"),
    ("chiller", "Chiller", "CH"),
    ("eng", "Engineering Workstation", "EWS"),
    ("mcc", "Motor Control Center", "MCC"),
];
const LINKS: &[&str] = &[
    "Ethernet",
    "Modbus TCP",
    "Profinet",
    "EtherNet/IP",
    "4-20 mA",
    "24 VDC",
    "480 VAC",
    "RS-485",
    "Fiber",
    "DeviceNet",
    "HART",
    "OPC UA",
    "Redundant Ring",
    "Hardwired Interlock",
];
const SITES: &[&str] = &[
    "Pump Station",
    "Substation",
    "Water Treatment Plant",
    "Compressor Building",
    "Boiler House",
    "Tank Farm",
    "Control Room",
    "Packaging Line",
    "Cooling Tower",
    "Wastewater Lift Station",
    "North Feeder",
    "Chemical Dosing Skid",
];
const DIAGRAM_KINDS: &[&str] = &[
    "Control Network",
    "One-Line Diagram",
    "P&ID Overview",
    "I/O Architecture",
    "Power Distribution",
    "Communication Architecture",
    "Safety System Layout",
    "C4 Container View",
    "Alarm Hierarchy",
];
const PROSE: &[&str] = &[
    "primary",
    "secondary",
    "redundant",
    "supervisory",
    "control",
    "network",
    "interface",
    "motor",
    "pump",
    "valve",
    "feeder",
    "breaker",
    "turbine",
    "generator",
    "cooling",
    "loop",
    "pressure",
    "temperature",
    "flow",
    "level",
    "station",
    "node",
    "link",
    "panel",
    "cabinet",
    "field",
    "device",
    "remote",
    "local",
    "manual",
    "automatic",
    "interlock",
    "alarm",
    "trip",
    "setpoint",
    "shall",
    "be",
    "routed",
    "via",
    "the",
    "and",
    "to",
    "from",
    "with",
    "all",
    "cables",
    "conduit",
    "grounded",
    "per",
    "standard",
    "drawing",
    "revision",
    "approved",
];
const NOTES: &[&str] = &[
    "ALL FIELD WIRING SHALL BE SHIELDED TWISTED PAIR.",
    "NETWORK SWITCHES ARE CONFIGURED IN A REDUNDANT RING TOPOLOGY.",
    "REFER TO I/O SCHEDULE FOR TERMINAL ASSIGNMENTS.",
    "MOTOR CONTROLLERS INTERLOCKED WITH LOW-LEVEL SWITCH.",
    "SUPERVISORY SERVER SYNCHRONIZES TIME VIA NTP.",
    "DO NOT SCALE DRAWING.",
    "FIBER PATCH PANELS LOCATED IN MAIN CONTROL ROOM.",
    "PUMP P-101 AND P-102 OPERATE IN DUTY/STANDBY.",
    "VALVE POSITIONS SHOWN IN NORMAL OPERATING STATE.",
    "UPS PROVIDES 30 MINUTES OF RUNTIME AT FULL LOAD.",
];
const PEOPLE: &[&str] = &[
    "J. Smith",
    "A. Patel",
    "M. Garcia",
    "L. Chen",
    "R. Okafor",
    "K. Novak",
];

// ============================================================================ icon content

fn color(r: &mut Rng) -> &'static str {
    r.pick(COLORS)
}

fn paint(r: &mut Rng, grads: usize) -> String {
    if grads > 0 && r.chance(0.35) {
        format!("url(#g{})", r.below(grads as u64))
    } else if r.chance(0.08) {
        "none".into()
    } else {
        color(r).into()
    }
}

fn transform(r: &mut Rng, vb: f64) -> String {
    match r.below(5) {
        0 => format!(
            "rotate({} {} {})",
            r.range(-180, 180),
            n(vb / 2.0),
            n(vb / 2.0)
        ),
        1 => format!(
            "translate({} {})",
            n(r.frange(-vb / 8.0, vb / 8.0)),
            n(r.frange(-vb / 8.0, vb / 8.0))
        ),
        2 => format!("scale({})", n(r.frange(0.5, 1.2))),
        3 => format!(
            "matrix({} 0 0 {} {} {})",
            n(r.frange(0.6, 1.1)),
            n(r.frange(0.6, 1.1)),
            n(r.frange(0.0, vb / 6.0)),
            n(r.frange(0.0, vb / 6.0))
        ),
        _ => format!("skewX({})", r.range(-20, 20)),
    }
}

fn path_data(r: &mut Rng, vb: f64) -> String {
    let mut d = String::new();
    let p = |r: &mut Rng| r.frange(vb * 0.05, vb * 0.95);
    d.push('M');
    num(&mut d, p(r));
    d.push(' ');
    num(&mut d, p(r));
    for _ in 0..r.range(2, 8) {
        match r.below(6) {
            0 => {
                d.push_str(" L");
                num(&mut d, p(r));
                d.push(' ');
                num(&mut d, p(r));
            }
            1 => {
                d.push_str(" C");
                for i in 0..6 {
                    if i > 0 {
                        d.push(' ');
                    }
                    num(&mut d, p(r));
                }
            }
            2 => {
                d.push_str(" Q");
                for i in 0..4 {
                    if i > 0 {
                        d.push(' ');
                    }
                    num(&mut d, p(r));
                }
            }
            3 => {
                let rad = r.frange(vb * 0.05, vb * 0.3);
                let _ = write!(
                    d,
                    " A{} {} 0 {} {} ",
                    n(rad),
                    n(rad),
                    r.below(2),
                    r.below(2)
                );
                num(&mut d, p(r));
                d.push(' ');
                num(&mut d, p(r));
            }
            4 => {
                d.push_str(" H");
                num(&mut d, p(r));
            }
            _ => {
                d.push_str(" V");
                num(&mut d, p(r));
            }
        }
    }
    if r.chance(0.5) {
        d.push_str(" Z");
    }
    d
}

fn polygon_points(r: &mut Rng, vb: f64) -> String {
    let star = r.chance(0.5);
    let k = r.range(3, 8) as usize;
    let cx = vb / 2.0;
    let rad = vb * r.frange(0.25, 0.45);
    let rot = r.frange(0.0, PI);
    let mut s = String::new();
    let count = if star { k * 2 } else { k };
    for i in 0..count {
        let a = rot + 2.0 * PI * i as f64 / count as f64;
        let rr = if star && i % 2 == 1 { rad * 0.45 } else { rad };
        if i > 0 {
            s.push(' ');
        }
        num(&mut s, cx + rr * dcos(a));
        s.push(',');
        num(&mut s, cx + rr * dsin(a));
    }
    s
}

fn shape(r: &mut Rng, vb: f64, grads: usize, out: &mut String) {
    let fill = paint(r, grads);
    let stroke = if r.chance(0.4) {
        format!(
            r#" stroke="{}" stroke-width="{}"{}"#,
            color(r),
            n((vb / 24.0) * r.frange(0.5, 2.5)),
            if r.chance(0.3) {
                r#" stroke-linecap="round" stroke-linejoin="round""#
            } else {
                ""
            }
        )
    } else {
        String::new()
    };
    let extra = if r.chance(0.1) {
        format!(r#" opacity="{}""#, n(r.frange(0.3, 0.9)))
    } else if r.chance(0.1) {
        format!(
            r#" class="{}""#,
            r.pick(&["primary", "accent", "shadow", "fg", "bg"])
        )
    } else {
        String::new()
    };
    let c = |r: &mut Rng| n(r.frange(vb * 0.05, vb * 0.95));
    match r.below(8) {
        0 => {
            let _ = write!(
                out,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{fill}"{stroke}{extra}/>"#,
                c(r),
                c(r),
                n(r.frange(vb * 0.1, vb * 0.6)),
                n(r.frange(vb * 0.1, vb * 0.6)),
                n(r.frange(0.0, vb * 0.1))
            );
        }
        1 => {
            let _ = write!(
                out,
                r#"<circle cx="{}" cy="{}" r="{}" fill="{fill}"{stroke}{extra}/>"#,
                c(r),
                c(r),
                n(r.frange(vb * 0.05, vb * 0.4))
            );
        }
        2 => {
            let _ = write!(
                out,
                r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="{fill}"{stroke}{extra}/>"#,
                c(r),
                c(r),
                n(r.frange(vb * 0.05, vb * 0.4)),
                n(r.frange(vb * 0.05, vb * 0.3))
            );
        }
        3 => {
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="{}"{extra}/>"#,
                c(r),
                c(r),
                c(r),
                c(r),
                color(r),
                n(vb / 24.0 * r.frange(1.0, 3.0))
            );
        }
        4 => {
            let mut pts = String::new();
            for i in 0..r.range(3, 7) {
                if i > 0 {
                    pts.push(' ');
                }
                let _ = write!(pts, "{},{}", c(r), c(r));
            }
            let _ = write!(
                out,
                r#"<polyline points="{pts}" fill="none" stroke="{}" stroke-width="{}"{extra}/>"#,
                color(r),
                n(vb / 24.0 * r.frange(1.0, 2.5))
            );
        }
        5 => {
            let _ = write!(
                out,
                r#"<polygon points="{}" fill="{fill}"{stroke}{extra}/>"#,
                polygon_points(r, vb)
            );
        }
        6 if r.chance(0.15) => {
            let _ = write!(
                out,
                r#"<text x="{}" y="{}" font-family="sans-serif" font-size="{}" fill="{}">{}</text>"#,
                n(vb * 0.1),
                n(vb * 0.6),
                n(vb * 0.25),
                color(r),
                r.pick(NOUNS).to_uppercase()
            );
        }
        _ => {
            let _ = write!(
                out,
                r#"<path d="{}" fill="{fill}"{stroke}{extra}/>"#,
                path_data(r, vb)
            );
        }
    }
}

/// A varied icon/illustration SVG.
pub fn icon_svg(r: &mut Rng, label: &str) -> String {
    let vb = *r.pick(&[
        16.0, 20.0, 24.0, 24.0, 24.0, 32.0, 48.0, 64.0, 100.0, 128.0, 256.0, 512.0,
    ]);
    let illustration = r.chance(0.03);
    let mut s = String::with_capacity(if illustration { 32_768 } else { 2048 });
    if r.chance(0.5) {
        s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }
    s.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg""#);
    if r.chance(0.2) {
        s.push_str(r#" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1""#);
    }
    let vh = if r.chance(0.1) { vb * 0.75 } else { vb };
    match r.below(10) {
        0..=5 => {
            let k = *r.pick(&[1.0, 1.0, 2.0, 0.5]);
            let _ = write!(s, r#" width="{}" height="{}""#, n(vb * k), n(vh * k));
        }
        6 => {}
        7 => s.push_str(r#" width="100%" height="100%""#),
        8 => {
            let _ = write!(
                s,
                r#" width="{}mm" height="{}mm""#,
                n(vb / 4.0),
                n(vh / 4.0)
            );
        }
        _ => {
            let _ = write!(s, r#" width="{}pt" height="{}pt""#, n(vb), n(vh));
        }
    }
    if r.chance(0.05) {
        let _ = write!(
            s,
            r#" viewBox="{} {} {} {}""#,
            n(-vb / 2.0),
            n(-vh / 2.0),
            n(vb),
            n(vh)
        );
        // shift content into the negative-origin box
        let _ = write!(
            s,
            r#"><g transform="translate({} {})""#,
            n(-vb / 2.0),
            n(-vh / 2.0)
        );
        s.push('>');
        icon_body(r, vb, label, illustration, &mut s);
        s.push_str("</g></svg>\n");
        return s;
    }
    let _ = write!(s, r#" viewBox="0 0 {} {}""#, n(vb), n(vh));
    if r.chance(0.15) {
        let _ = write!(s, r#" id="{}""#, slug(label));
    }
    s.push('>');
    icon_body(r, vb, label, illustration, &mut s);
    s.push_str("</svg>\n");
    s
}

fn icon_body(r: &mut Rng, vb: f64, label: &str, illustration: bool, s: &mut String) {
    if r.chance(0.4) {
        let _ = write!(s, "<title>{}</title>", xml_escape(label));
    }
    if r.chance(0.1) {
        let _ = write!(
            s,
            "<desc>{} icon for {} diagrams</desc>",
            xml_escape(label),
            r.pick(SITES)
        );
    }
    let grads = if r.chance(0.3) {
        r.range(1, 2) as usize
    } else {
        0
    };
    if grads > 0 {
        s.push_str("<defs>");
        for g in 0..grads {
            let stops = r.range(2, 4);
            let linear = r.chance(0.6);
            if linear {
                let _ = write!(
                    s,
                    r#"<linearGradient id="g{g}" x1="0" y1="0" x2="{}" y2="1">"#,
                    r.below(2)
                );
            } else {
                let _ = write!(s, r#"<radialGradient id="g{g}" cx="0.5" cy="0.5" r="0.6">"#);
            }
            for i in 0..stops {
                let _ = write!(
                    s,
                    r#"<stop offset="{}" stop-color="{}"/>"#,
                    n(i as f64 / (stops - 1) as f64),
                    r.pick(&COLORS[..17])
                );
            }
            s.push_str(if linear {
                "</linearGradient>"
            } else {
                "</radialGradient>"
            });
        }
        s.push_str("</defs>");
    }
    if r.chance(0.1) {
        s.push_str("<style>.primary{fill:#1e88e5}.accent{fill:#fb8c00}.shadow{opacity:.3}</style>");
    }
    let count = if illustration {
        r.range(80, 400)
    } else {
        r.range(1, 12)
    };
    let mut open = 0;
    for _ in 0..count {
        if open < 3 && r.chance(0.15) {
            let id = if r.chance(0.3) {
                format!(r#" id="{}-{}""#, r.pick(QUALIFIERS), r.below(100))
            } else {
                String::new()
            };
            let _ = write!(s, r#"<g transform="{}"{id}>"#, transform(r, vb));
            open += 1;
        }
        shape(r, vb, grads, s);
        if open > 0 && r.chance(0.2) {
            s.push_str("</g>");
            open -= 1;
        }
    }
    for _ in 0..open {
        s.push_str("</g>");
    }
}

// ============================================================================ diagram content

/// An engineering-diagram SVG with boxes, connectors, labels, notes and a title block.
pub fn diagram_svg(r: &mut Rng, site: &str, kind: &str, number: u32) -> String {
    let cols = r.range(3, 7) as usize;
    let rows = r.range(2, 6) as usize;
    let cw = 260.0;
    let ch = 170.0;
    let w = 80.0 + cols as f64 * cw + 300.0;
    let h = 140.0 + rows as f64 * ch + 220.0;
    let mut s = String::with_capacity(24 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    let _ = writeln!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" id=\"dwg-{number}\" class=\"diagram {}\">",
        n(w),
        n(h),
        n(w),
        n(h),
        slug(kind)
    );
    let title = format!("{site} {} — {kind}", r.range(1, 12));
    let _ = writeln!(s, "<title>{}</title>", xml_escape(&title));
    let mut desc = String::new();
    for i in 0..r.range(20, 70) {
        if i > 0 {
            desc.push(' ');
        }
        desc.push_str(r.pick(PROSE));
    }
    let _ = writeln!(s, "<desc>{}.</desc>", xml_escape(&desc));
    s.push_str(concat!(
        "<defs>\n",
        "<marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"6\" markerHeight=\"6\" orient=\"auto-start-reverse\"><path d=\"M0 0 L10 5 L0 10 z\" fill=\"#333\"/></marker>\n",
        "<style>.node rect{fill:#fff;stroke:#37474f;stroke-width:2}.label{font-family:Arial,Helvetica,sans-serif;font-size:15px;fill:#263238}",
        ".sub{font-size:11px;fill:#607d8b}.wire{fill:none;stroke:#455a64;stroke-width:1.5}.wire-label{font-family:Arial,sans-serif;font-size:10px;fill:#455a64}",
        ".tb text{font-family:'Courier New',monospace;font-size:11px}.critical rect{stroke:#c62828}</style>\n",
        "</defs>\n"
    ));
    let _ = writeln!(
        s,
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#fafafa\" stroke=\"#263238\" stroke-width=\"3\"/>",
        n(w),
        n(h)
    );
    let _ = writeln!(
        s,
        "<text x=\"40\" y=\"56\" font-family=\"Arial\" font-size=\"26\" font-weight=\"bold\" fill=\"#102027\">{}</text>",
        xml_escape(&title)
    );
    // Nodes on a grid with some holes.
    let mut nodes: Vec<(f64, f64, usize, String)> = Vec::new();
    let _ = writeln!(s, "<g id=\"nodes\">");
    for row in 0..rows {
        for col in 0..cols {
            if r.chance(0.15) {
                continue;
            }
            let t = r.below(NODE_TYPES.len() as u64) as usize;
            let (cls, name, pre) = NODE_TYPES[t];
            let tag = format!("{pre}-{}", r.range(100, 999));
            let x = 40.0 + col as f64 * cw;
            let y = 100.0 + row as f64 * ch;
            let critical = r.chance(0.1);
            let _ = write!(
                s,
                "<g id=\"node-{}\" class=\"node {cls}{}\" transform=\"translate({} {})\">",
                slug(&tag),
                if critical { " critical" } else { "" },
                n(x),
                n(y)
            );
            let _ = write!(
                s,
                "<rect width=\"200\" height=\"100\" rx=\"{}\"/>",
                r.pick(&[0, 4, 8, 12])
            );
            let _ = write!(
                s,
                "<text class=\"label\" x=\"100\" y=\"38\" text-anchor=\"middle\">{}</text>",
                xml_escape(name)
            );
            let detail = match r.below(4) {
                0 => format!(
                    "IP 10.{}.{}.{}",
                    r.range(0, 40),
                    r.range(0, 254),
                    r.range(2, 254)
                ),
                1 => format!(
                    "Rated {} V / {} A",
                    r.pick(&[24, 120, 230, 400, 480, 690]),
                    r.range(1, 400)
                ),
                2 => format!(
                    "Loop {}-{}",
                    r.pick(&["FIC", "PIC", "LIC", "TIC"]),
                    r.range(100, 499)
                ),
                _ => format!(
                    "Panel {}{}",
                    r.pick(&["CP", "MCC", "LCP", "RIO"]),
                    r.range(1, 30)
                ),
            };
            let _ = write!(
                s,
                "<text class=\"label sub\" x=\"100\" y=\"62\" text-anchor=\"middle\"><tspan x=\"100\" dy=\"0\">{}</tspan><tspan x=\"100\" dy=\"16\">{}</tspan></text>",
                xml_escape(&tag),
                xml_escape(&detail)
            );
            s.push_str("</g>\n");
            nodes.push((x, y, t, tag));
        }
    }
    s.push_str("</g>\n<g id=\"wires\">\n");
    // Orthogonal connectors.
    for i in 1..nodes.len() {
        let links = if r.chance(0.3) { 2 } else { 1 };
        for _ in 0..links {
            let j = r.below(i as u64) as usize;
            let (x1, y1) = (nodes[j].0 + 200.0, nodes[j].1 + 50.0);
            let (x2, y2) = (nodes[i].0, nodes[i].1 + 50.0);
            let mx = (x1 + x2) / 2.0;
            let link = r.pick(LINKS);
            let _ = write!(
                s,
                "<path id=\"w-{}-{}\" class=\"wire {}\" d=\"M{} {} H{} V{} H{}\" marker-end=\"url(#arrow)\"{}/>",
                slug(&nodes[j].3),
                slug(&nodes[i].3),
                slug(link),
                n(x1),
                n(y1),
                n(mx),
                n(y2),
                n(x2),
                if r.chance(0.2) { " stroke-dasharray=\"6 4\"" } else { "" }
            );
            if r.chance(0.6) {
                let _ = write!(
                    s,
                    "<text class=\"wire-label\" x=\"{}\" y=\"{}\">{}</text>",
                    n(mx + 4.0),
                    n((y1 + y2) / 2.0 - 4.0),
                    xml_escape(link)
                );
            }
            s.push('\n');
        }
    }
    s.push_str("</g>\n");
    // Legend.
    let lx = 60.0 + cols as f64 * cw;
    let _ = write!(
        s,
        "<g id=\"legend\" transform=\"translate({} 100)\"><rect width=\"220\" height=\"{}\" fill=\"#fff\" stroke=\"#90a4ae\"/><text x=\"10\" y=\"22\" font-family=\"Arial\" font-size=\"14\" font-weight=\"bold\">LEGEND</text>",
        n(lx),
        40 + 18 * 6
    );
    for k in 0..6 {
        let _ = write!(
            s,
            "<text x=\"10\" y=\"{}\" font-family=\"Arial\" font-size=\"12\">{} — {}</text>",
            44 + 18 * k,
            r.pick(&["solid", "dashed", "bold", "red", "dotted", "double"]),
            xml_escape(r.pick(LINKS))
        );
    }
    s.push_str("</g>\n");
    // Notes.
    let ny = 140.0 + rows as f64 * ch;
    let _ = write!(
        s,
        "<g id=\"notes\" transform=\"translate(40 {})\"><text font-family=\"Arial\" font-size=\"13\" font-weight=\"bold\">NOTES:</text>",
        n(ny)
    );
    for k in 0..r.range(3, 7) {
        let _ = write!(
            s,
            "<text y=\"{}\" font-family=\"Arial\" font-size=\"12\">{}. {}</text>",
            20 + 17 * k,
            k + 1,
            r.pick(NOTES)
        );
    }
    s.push_str("</g>\n");
    // Title block.
    let _ = write!(
        s,
        "<g class=\"tb\" id=\"title-block\" transform=\"translate({} {})\"><rect width=\"280\" height=\"150\" fill=\"#fff\" stroke=\"#263238\" stroke-width=\"2\"/>",
        n(w - 300.0),
        n(h - 170.0)
    );
    let lines = [
        format!("PROJECT: {}", site.to_uppercase()),
        format!("TITLE: {}", kind.to_uppercase()),
        format!("DWG NO. ENG-{:05}", number),
        format!("REV {}", r.pick(&["A", "B", "C", "D", "0", "1"])),
        format!("DRAWN: {}", r.pick(PEOPLE)),
        format!("CHECKED: {}", r.pick(PEOPLE)),
        format!("SHEET {} OF {}", r.range(1, 3), r.range(3, 9)),
    ];
    for (k, l) in lines.iter().enumerate() {
        let _ = write!(
            s,
            "<text x=\"10\" y=\"{}\">{}</text>",
            20 + 19 * k,
            xml_escape(l)
        );
    }
    s.push_str("</g>\n</svg>\n");
    s
}

// ============================================================================ datasets A–D

#[derive(Debug, Default, Clone)]
pub struct Summary {
    /// Visible `.svg` files (what discovery should report).
    pub svg_files: usize,
    /// `.svg` files under hidden dirs / with hidden names (must be skipped).
    pub hidden_svgs: usize,
    /// Non-SVG decoys (must be skipped).
    pub other_files: usize,
    pub bytes: u64,
}

enum Content {
    Icon(String),
    Diagram {
        site: &'static str,
        kind: &'static str,
        number: u32,
    },
    Png,
    Gz(String),
    Text(&'static str),
}

struct Planned {
    rel: String,
    seed: u64,
    content: Content,
}

fn plan_folders(r: &mut Rng, n: usize, tree: Tree, per_folder: (i64, i64)) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut used = HashSet::new();
    let mut remaining = n;
    while remaining > 0 {
        let (top, subs) = r.pick(tree);
        let depth = r.range(1, 6) as usize;
        let mut parts: Vec<String> = vec![top.to_string()];
        if depth >= 2 {
            parts.push(r.pick(subs).to_string());
        }
        while parts.len() < depth {
            let d = r.pick(DEEPER).to_string();
            if parts.contains(&d) {
                parts.push(format!("{d}-{}", parts.len()));
            } else {
                parts.push(d);
            }
        }
        let base = parts.join("/");
        let mut path = base.clone();
        let mut k = 2;
        while !used.insert(path.clone()) {
            path = format!("{base}-{k}");
            k += 1;
        }
        let count = (r.range(per_folder.0, per_folder.1) as usize).min(remaining);
        out.push((path, count));
        remaining -= count;
    }
    out
}

fn icon_name(r: &mut Rng) -> (String, String) {
    let sep = *r.pick(&["-", "-", "-", "_", " "]);
    let a = *r.pick(NOUNS);
    let b = *r.pick(NOUNS);
    let q = *r.pick(QUALIFIERS);
    let sz = *r.pick(SIZES);
    let label = format!("{a} {b}");
    let stem = match r.below(6) {
        0 => a.to_string(),
        1 => format!("{a}{sep}{b}"),
        2 => format!("{a}{sep}{q}"),
        3 => format!("{a}{sep}{b}{sep}{sz}"),
        4 => format!("ic{sep}{a}{sep}{q}{sep}{sz}"),
        _ => format!("{a}{sep}{b}{sep}{q}"),
    };
    (stem, label)
}

fn unique(used: &mut HashSet<String>, stem: &str, ext: &str) -> String {
    let mut name = format!("{stem}{ext}");
    let mut k = 2;
    while !used.insert(name.to_ascii_lowercase()) {
        name = format!("{stem}-{k}{ext}");
        k += 1;
    }
    name
}

/// Generate an icon (A/B/C) or diagram (D) dataset with exactly `n` visible SVGs.
pub fn generate_tree(dir: &Path, n: usize, seed: u64, diagrams: bool) -> io::Result<Summary> {
    let mut r = Rng::new(seed);
    let tree = if diagrams { DIAGRAM_TREE } else { ICON_TREE };
    let per_folder = if n < 2000 { (40, 250) } else { (50, 500) };
    let folders = plan_folders(&mut r, n, tree, per_folder);
    let mut plan: Vec<Planned> = Vec::with_capacity(n + n / 50 + 16);
    let mut summary = Summary::default();
    let mut idx: u64 = 0;
    let next_seed = |idx: &mut u64| {
        *idx += 1;
        derive(seed, *idx, 0x51)
    };
    let mut dwg = 1000u32;
    for (folder, count) in &folders {
        let mut used = HashSet::new();
        for _ in 0..*count {
            let upper = r.chance(0.01);
            let ext = if upper { ".SVG" } else { ".svg" };
            let (rel, content) = if diagrams {
                let site = *r.pick(SITES);
                let kind = *r.pick(DIAGRAM_KINDS);
                dwg += 1;
                let stem = format!(
                    "{}-{}-{}{}",
                    slug(site),
                    slug(kind),
                    r.range(1, 40),
                    if r.chance(0.3) {
                        format!("-rev-{}", r.pick(&["a", "b", "c"]))
                    } else {
                        String::new()
                    }
                );
                (
                    format!("{folder}/{}", unique(&mut used, &stem, ext)),
                    Content::Diagram {
                        site,
                        kind,
                        number: dwg,
                    },
                )
            } else {
                let (stem, label) = icon_name(&mut r);
                (
                    format!("{folder}/{}", unique(&mut used, &stem, ext)),
                    Content::Icon(label),
                )
            };
            plan.push(Planned {
                rel,
                seed: next_seed(&mut idx),
                content,
            });
            summary.svg_files += 1;
        }
    }
    // Hidden SVGs that discovery must skip.
    let hidden = (n / 500).max(3);
    for i in 0..hidden {
        let (stem, label) = icon_name(&mut r);
        let folder = &folders[r.below(folders.len() as u64) as usize].0;
        let rel = match i % 4 {
            0 => format!(".git/objects/{:02x}/{}.svg", r.below(256), slug(&stem)),
            1 => format!(".cache/thumbs/{}-{i}.svg", slug(&stem)),
            2 => format!("{folder}/.backup/{}-{i}.svg", slug(&stem)),
            _ => format!("{folder}/.{}-{i}.svg", slug(&stem)),
        };
        plan.push(Planned {
            rel,
            seed: next_seed(&mut idx),
            content: Content::Icon(label),
        });
        summary.hidden_svgs += 1;
    }
    // Non-SVG decoys.
    let decoys = (n / 200).max(4);
    for i in 0..decoys {
        let (stem, label) = icon_name(&mut r);
        let folder = &folders[r.below(folders.len() as u64) as usize].0;
        let s = slug(&stem);
        let (rel, content) = match i % 4 {
            0 => (
                format!("{folder}/README-{i}.md"),
                Content::Text("# Icons\n\nGenerated test assets.\n"),
            ),
            1 => (format!("{folder}/{s}-{i}.png"), Content::Png),
            2 => (format!("{folder}/{s}-{i}.svg.bak"), Content::Icon(label)),
            _ => (format!("{folder}/{s}-{i}.svgz"), Content::Gz(label)),
        };
        plan.push(Planned {
            rel,
            seed: next_seed(&mut idx),
            content,
        });
        summary.other_files += 1;
    }

    // Directories first (sequential), then content in parallel (per-file seeds keep
    // output independent of scheduling).
    let mut dirs: Vec<PathBuf> = plan
        .iter()
        .filter_map(|p| dir.join(&p.rel).parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    for d in &dirs {
        fs::create_dir_all(d)?;
    }
    let next = AtomicUsize::new(0);
    let bytes = AtomicU64::new(0);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(8);
    let png = sample_png();
    std::thread::scope(|scope| -> io::Result<()> {
        let mut handles = Vec::new();
        for _ in 0..threads {
            handles.push(scope.spawn(|| -> io::Result<()> {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(p) = plan.get(i) else { return Ok(()) };
                    let mut r = Rng::new(p.seed);
                    let data: Vec<u8> = match &p.content {
                        Content::Icon(label) => icon_svg(&mut r, label).into_bytes(),
                        Content::Diagram { site, kind, number } => {
                            diagram_svg(&mut r, site, kind, *number).into_bytes()
                        }
                        Content::Png => png.clone(),
                        Content::Gz(label) => gzip_stored(icon_svg(&mut r, label).as_bytes()),
                        Content::Text(t) => t.as_bytes().to_vec(),
                    };
                    fs::write(dir.join(&p.rel), &data)?;
                    bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                }
            }));
        }
        for h in handles {
            h.join().expect("generator thread panicked")?;
        }
        Ok(())
    })?;
    summary.bytes = bytes.load(Ordering::Relaxed);
    Ok(summary)
}

// ============================================================================ dataset E

/// What analysis of a pathological file should produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Ready,
    LimitExceeded,
    ParseError,
    /// Any analyzed state (Ready / LimitExceeded / ParseError) — behaviour is
    /// implementation-defined but must be clean and bounded.
    Analyzed,
    /// Must not be discovered at all (hidden, or not an `.svg` name).
    Skipped,
}

#[derive(Debug, Clone)]
pub struct PathoCase {
    pub rel: String,
    pub expect: Expect,
    /// `Some(true)`: thumbnail must render; `Some(false)`: must be a clean error;
    /// `None`: either (but bounded time, no panic).
    pub thumb: Option<bool>,
    pub note: &'static str,
}

/// Sizes for dataset E. `full()` is the on-disk stress set; `small()` keeps the same
/// cases (same expected outcomes under [`PathoParams::limits`]) at test-friendly sizes.
#[derive(Debug, Clone)]
pub struct PathoParams {
    pub max_file_bytes: u64,
    pub max_embedded_raster_bytes: u64,
    pub under_limit_bytes: u64,
    pub over_limit_bytes: u64,
    pub huge_bytes: u64,
    /// Decoded size of the huge embedded PNG.
    pub huge_raster_bytes: u64,
    pub massive_text_bytes: u64,
    pub path_coords: usize,
    /// Far beyond `Limits::max_nesting_depth` (and usvg's own 1,024 guard).
    pub nest_depth: usize,
    /// Over `max_nesting_depth` but under usvg's guard (used to overflow its converter).
    pub nest_mid_depth: usize,
    /// Just under `max_nesting_depth`: must analyze and render.
    pub nest_ok_depth: usize,
    pub node_bomb: usize,
}

impl PathoParams {
    pub fn full() -> Self {
        Self {
            max_file_bytes: 25 * MB,
            max_embedded_raster_bytes: 50 * MB,
            under_limit_bytes: (24.9 * MB as f64) as u64,
            over_limit_bytes: 26 * MB,
            huge_bytes: 100 * MB,
            huge_raster_bytes: 60 * MB,
            massive_text_bytes: 5 * MB,
            path_coords: 1_000_000,
            nest_depth: 5_000,
            nest_mid_depth: 1_000,
            nest_ok_depth: 250,
            node_bomb: 150_000,
        }
    }
    pub fn small() -> Self {
        Self {
            max_file_bytes: 4 * MB,
            max_embedded_raster_bytes: MB,
            under_limit_bytes: (3.9 * MB as f64) as u64,
            over_limit_bytes: (4.2 * MB as f64) as u64,
            huge_bytes: 8 * MB,
            huge_raster_bytes: 2 * MB,
            massive_text_bytes: MB,
            path_coords: 200_000,
            nest_depth: 5_000,
            nest_mid_depth: 1_000,
            nest_ok_depth: 250,
            node_bomb: 120_000,
        }
    }
    /// Limits under which the documented expectations hold.
    pub fn limits(&self) -> Limits {
        Limits {
            max_file_bytes: self.max_file_bytes,
            max_embedded_raster_bytes: self.max_embedded_raster_bytes,
            ..Limits::default()
        }
    }
}

/// Write `<svg>` + many random-walk `<path>` elements, padded with a comment to exactly
/// `target` bytes. Streams; never holds the document in memory.
fn write_padded_paths(dir: &Path, rel: &str, target: u64, r: &mut Rng) -> io::Result<u64> {
    let mut w = create(dir, rel)?;
    let header = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\" viewBox=\"0 0 1000 1000\">";
    let footer = "</svg>\n";
    w.write_all(header.as_bytes())?;
    let mut written = header.len() as u64;
    let mut el = String::with_capacity(8192);
    loop {
        el.clear();
        el.push_str("<path d=\"M");
        let (mut x, mut y) = (r.frange(100.0, 900.0), r.frange(100.0, 900.0));
        num(&mut el, x);
        el.push(' ');
        num(&mut el, y);
        el.push_str(" L");
        for _ in 0..300 {
            x = (x + r.frange(-15.0, 15.0)).clamp(0.0, 1000.0);
            y = (y + r.frange(-15.0, 15.0)).clamp(0.0, 1000.0);
            el.push(' ');
            num(&mut el, x);
            el.push(' ');
            num(&mut el, y);
        }
        let _ = write!(
            el,
            "\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.5\"/>",
            r.pick(&COLORS[..16])
        );
        if written + el.len() as u64 + footer.len() as u64 + 7 > target {
            break;
        }
        w.write_all(el.as_bytes())?;
        written += el.len() as u64;
    }
    let pad = target - written - footer.len() as u64 - 7;
    w.write_all(b"<!--")?;
    let chunk = vec![b'x'; 1 << 16];
    let mut left = pad;
    while left > 0 {
        let k = left.min(chunk.len() as u64) as usize;
        w.write_all(&chunk[..k])?;
        left -= k as u64;
    }
    w.write_all(b"-->")?;
    w.write_all(footer.as_bytes())?;
    w.flush()?;
    Ok(target)
}

fn svg_doc(body: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"200\" height=\"200\" viewBox=\"0 0 200 200\">{body}</svg>\n"
    )
}

fn labelled(title: &str, text: &str) -> String {
    svg_doc(&format!(
        "<title>{}</title><rect x=\"10\" y=\"10\" width=\"180\" height=\"120\" rx=\"12\" fill=\"#1e88e5\"/><text x=\"100\" y=\"170\" text-anchor=\"middle\" font-size=\"18\">{}</text>",
        xml_escape(title),
        xml_escape(text)
    ))
}

/// Write the pathological set into `dir`. Returns every case with its expectation
/// under `p.limits()`.
pub fn write_pathological(dir: &Path, p: &PathoParams, seed: u64) -> io::Result<Vec<PathoCase>> {
    use Expect::*;
    let mut r = Rng::new(seed);
    let mut cases: Vec<PathoCase> = Vec::new();
    let mut add = |rel: &str, expect: Expect, thumb: Option<bool>, note: &'static str| {
        cases.push(PathoCase {
            rel: rel.to_string(),
            expect,
            thumb,
            note,
        });
    };
    fs::create_dir_all(dir)?;

    // ---- malformed / not SVG
    put(dir, "malformed/zero-byte.svg", b"")?;
    add(
        "malformed/zero-byte.svg",
        ParseError,
        Some(false),
        "empty file",
    );
    put(dir, "malformed/unclosed-tags.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><g><rect width=\"5\" height=\"5\"></svg>")?;
    add(
        "malformed/unclosed-tags.svg",
        ParseError,
        Some(false),
        "mismatched tags",
    );
    let full = icon_svg(&mut r, "truncated");
    put(
        dir,
        "malformed/truncated.svg",
        &full.as_bytes()[..full.len() / 2],
    )?;
    add(
        "malformed/truncated.svg",
        ParseError,
        Some(false),
        "file cut in half",
    );
    let mut garbage = vec![0u8; 4096];
    for b in garbage.iter_mut() {
        *b = r.next_u64() as u8;
    }
    garbage[0] = 0x00; // never a BOM
    put(dir, "malformed/binary-garbage.svg", &garbage)?;
    add(
        "malformed/binary-garbage.svg",
        ParseError,
        Some(false),
        "random bytes",
    );
    put(dir, "malformed/plain-text.svg", b"this is not xml at all\n")?;
    add(
        "malformed/plain-text.svg",
        ParseError,
        Some(false),
        "plain text",
    );
    put(
        dir,
        "malformed/html-root.svg",
        b"<!DOCTYPE html><html><body><p>hello</p></body></html>",
    )?;
    add(
        "malformed/html-root.svg",
        ParseError,
        Some(false),
        "non-svg root (html)",
    );
    put(dir, "malformed/xhtml-with-inline-svg.svg", b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\"/></svg></body></html>")?;
    add(
        "malformed/xhtml-with-inline-svg.svg",
        ParseError,
        Some(false),
        "svg nested in xhtml root",
    );
    put(
        dir,
        "malformed/two-roots.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/><svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )?;
    add(
        "malformed/two-roots.svg",
        ParseError,
        Some(false),
        "two root elements",
    );
    put(
        dir,
        "malformed/undeclared-prefix.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><inkscape:foo/></svg>",
    )?;
    add(
        "malformed/undeclared-prefix.svg",
        ParseError,
        Some(false),
        "unknown namespace prefix",
    );
    let gz_src = labelled("compressed drawing", "SVGZ");
    put(
        dir,
        "malformed/gzip-named-svg.svg",
        &gzip_stored(gz_src.as_bytes()),
    )?;
    add(
        "malformed/gzip-named-svg.svg",
        ParseError,
        Some(false),
        "gzip bytes with .svg name (svgz not supported)",
    );
    put(
        dir,
        "compressed-drawing.svgz",
        &gzip_stored(gz_src.as_bytes()),
    )?;
    add(
        "compressed-drawing.svgz",
        Skipped,
        None,
        ".svgz is not discovered",
    );

    // ---- size limits
    write_padded_paths(
        dir,
        &format!(
            "limits/just-under-limit-{}.svg",
            fmt_mb(p.under_limit_bytes)
        ),
        p.under_limit_bytes,
        &mut r,
    )?;
    add(
        &format!(
            "limits/just-under-limit-{}.svg",
            fmt_mb(p.under_limit_bytes)
        ),
        Ready,
        Some(true),
        "just under max_file_bytes; ~thousands of long paths",
    );
    write_padded_paths(
        dir,
        &format!("limits/just-over-limit-{}.svg", fmt_mb(p.over_limit_bytes)),
        p.over_limit_bytes,
        &mut r,
    )?;
    add(
        &format!("limits/just-over-limit-{}.svg", fmt_mb(p.over_limit_bytes)),
        LimitExceeded,
        Some(false),
        "just over max_file_bytes",
    );
    write_padded_paths(
        dir,
        &format!("limits/huge-{}.svg", fmt_mb(p.huge_bytes)),
        p.huge_bytes,
        &mut r,
    )?;
    add(
        &format!("limits/huge-{}.svg", fmt_mb(p.huge_bytes)),
        LimitExceeded,
        Some(false),
        "far over max_file_bytes (streamed)",
    );
    {
        let mut s = String::with_capacity(p.node_bomb * 4 + 200);
        s.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">");
        for _ in 0..p.node_bomb {
            s.push_str("<g/>");
        }
        s.push_str("</svg>");
        put(dir, "limits/node-bomb.svg", s.as_bytes())?;
        add(
            "limits/node-bomb.svg",
            LimitExceeded,
            Some(false),
            "more XML nodes than max_nodes",
        );
    }
    {
        let mut s =
            String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">");
        for _ in 0..p.nest_depth {
            s.push_str("<g>");
        }
        s.push_str("<rect width=\"50\" height=\"50\" fill=\"red\"/>");
        for _ in 0..p.nest_depth {
            s.push_str("</g>");
        }
        s.push_str("</svg>");
        put(
            dir,
            &format!("limits/nested-groups-{}.svg", p.nest_depth),
            s.as_bytes(),
        )?;
        add(
            &format!("limits/nested-groups-{}.svg", p.nest_depth),
            LimitExceeded,
            Some(false),
            "over max_nesting_depth (pre-scan; used to overflow the XML parser's stack)",
        );
        for (depth, expect, thumb, note) in [
            (
                p.nest_mid_depth,
                LimitExceeded,
                Some(false),
                "over max_nesting_depth, under usvg's 1,024 guard (used to overflow usvg's converter)",
            ),
            (p.nest_ok_depth, Ready, Some(true), "just under max_nesting_depth"),
        ] {
            let mut s =
                String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">");
            for i in 0..depth {
                let _ = write!(s, "<g id=\"g{i}\">");
            }
            s.push_str("<rect width=\"50\" height=\"50\" fill=\"green\"/>");
            for _ in 0..depth {
                s.push_str("</g>");
            }
            s.push_str("</svg>");
            let rel = format!("limits/nested-groups-{depth}.svg");
            put(dir, &rel, s.as_bytes())?;
            add(&rel, expect, thumb, note);
        }
    }
    {
        let mut w = create(dir, "limits/huge-single-path.svg")?;
        w.write_all(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\" viewBox=\"0 0 1000 1000\"><path fill=\"none\" stroke=\"#1e88e5\" stroke-width=\"0.3\" d=\"M500 500 L")?;
        let (mut x, mut y) = (500.0f64, 500.0f64);
        let mut buf = String::with_capacity(1 << 16);
        for i in 0..p.path_coords / 2 {
            x = (x + r.frange(-6.0, 6.0)).clamp(0.0, 1000.0);
            y = (y + r.frange(-6.0, 6.0)).clamp(0.0, 1000.0);
            buf.push(' ');
            num(&mut buf, x);
            buf.push(' ');
            num(&mut buf, y);
            if buf.len() > 60_000 || i + 1 == p.path_coords / 2 {
                w.write_all(buf.as_bytes())?;
                buf.clear();
            }
        }
        w.write_all(b"\"/></svg>\n")?;
        w.flush()?;
        add(
            "limits/huge-single-path.svg",
            Ready,
            Some(true),
            "one <path> with path_coords coordinates",
        );
    }

    // ---- embedded rasters
    let png = sample_png();
    put(
        dir,
        "embedded/png-small.svg",
        svg_doc(&format!(
        "<image x=\"20\" y=\"20\" width=\"160\" height=\"160\" href=\"data:image/png;base64,{}\"/>",
        b64(&png)
    ))
        .as_bytes(),
    )?;
    add(
        "embedded/png-small.svg",
        Ready,
        Some(true),
        "valid 16x16 PNG data URI",
    );
    put(dir, "embedded/jpeg-corrupt.svg", svg_doc(&format!(
        "<rect width=\"200\" height=\"200\" fill=\"#eee\"/><image width=\"200\" height=\"200\" xlink:href=\"data:image/jpeg;base64,{}\"/>",
        b64(&garbage[..600])
    )).as_bytes())?;
    add(
        "embedded/jpeg-corrupt.svg",
        Ready,
        Some(true),
        "undecodable JPEG data URI (image skipped)",
    );
    {
        // ~huge_raster_bytes decoded: uncompressed RGBA PNG, streamed through base64.
        let side = ((p.huge_raster_bytes / 4) as f64).sqrt() as u32;
        let mut w = create(dir, "embedded/png-huge.svg")?;
        let _ = write!(
            w,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\" viewBox=\"0 0 {side} {side}\"><image width=\"{side}\" height=\"{side}\" href=\"data:image/png;base64,"
        );
        let mut enc = B64::new(&mut w);
        write_png(&mut enc, side, side, |x, y| {
            [
                (x & 0xFF) as u8,
                (y & 0xFF) as u8,
                ((x ^ y) & 0xFF) as u8,
                0xFF,
            ]
        })?;
        enc.finish()?;
        w.write_all(b"\"/></svg>\n")?;
        w.flush()?;
        add(
            "embedded/png-huge.svg",
            LimitExceeded,
            Some(false),
            "embedded PNG over max_embedded_raster_bytes (or the file-size limit)",
        );
    }

    // ---- external resources
    put(dir, "external/img/badge.png", &png)?;
    add(
        "external/img/badge.png",
        Skipped,
        None,
        "raster next to SVGs (not discovered)",
    );
    put(dir, "external/http-image.svg", svg_doc("<rect width=\"200\" height=\"200\" fill=\"#ddd\"/><image width=\"200\" height=\"200\" href=\"http://example.invalid/tracker.png\"/>").as_bytes())?;
    add(
        "external/http-image.svg",
        Ready,
        Some(true),
        "http image (never fetched)",
    );
    put(dir, "external/file-uri-image.svg", svg_doc("<rect width=\"200\" height=\"200\" fill=\"#ddd\"/><image width=\"200\" height=\"200\" xlink:href=\"file:///nonexistent/dir/photo.png\"/>").as_bytes())?;
    add(
        "external/file-uri-image.svg",
        Ready,
        Some(true),
        "file:// image",
    );
    put(
        dir,
        "external/relative-image.svg",
        svg_doc("<image width=\"200\" height=\"200\" href=\"img/badge.png\"/>").as_bytes(),
    )?;
    add(
        "external/relative-image.svg",
        Ready,
        Some(true),
        "relative image resolved against the SVG's folder",
    );
    put(
        dir,
        "external/use-other-file.svg",
        svg_doc("<use href=\"relative-image.svg#x\"/><rect width=\"20\" height=\"20\"/>")
            .as_bytes(),
    )?;
    add(
        "external/use-other-file.svg",
        Ready,
        Some(true),
        "<use> referencing another file",
    );
    put(dir, "external/css-import.svg", svg_doc("<style>@import url(http://example.invalid/evil.css); rect{fill:teal}</style><rect width=\"200\" height=\"200\"/>").as_bytes())?;
    add(
        "external/css-import.svg",
        Ready,
        Some(true),
        "CSS @import of a remote stylesheet",
    );

    // ---- active content
    put(dir, "content/script-and-handlers.svg", svg_doc("<script type=\"text/javascript\">alert(document.cookie)</script><rect width=\"200\" height=\"200\" fill=\"orange\" onclick=\"alert(1)\" onmouseover=\"steal()\"/><a href=\"javascript:alert(2)\"><circle cx=\"100\" cy=\"100\" r=\"40\"/></a><foreignObject width=\"100\" height=\"50\"><div xmlns=\"http://www.w3.org/1999/xhtml\" onload=\"x()\">html</div></foreignObject>").as_bytes())?;
    add(
        "content/script-and-handlers.svg",
        Ready,
        Some(true),
        "<script>, on* handlers, javascript: href, foreignObject",
    );
    put(dir, "content/smil-animations.svg", svg_doc("<rect width=\"100\" height=\"100\" fill=\"red\"><animate attributeName=\"x\" from=\"0\" to=\"100\" dur=\"2s\" repeatCount=\"indefinite\"/><set attributeName=\"fill\" to=\"blue\" begin=\"1s\"/></rect><g><animateTransform attributeName=\"transform\" type=\"rotate\" from=\"0 100 100\" to=\"360 100 100\" dur=\"5s\" repeatCount=\"indefinite\"/><circle cx=\"150\" cy=\"150\" r=\"20\"><animateMotion path=\"M0 0 L-50 -50 Z\" dur=\"3s\" repeatCount=\"indefinite\"/></circle></g>").as_bytes())?;
    add(
        "content/smil-animations.svg",
        Ready,
        Some(true),
        "SMIL animate/set/animateTransform/animateMotion",
    );
    put(dir, "content/blur-heavy-filter.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4000\" height=\"4000\" viewBox=\"0 0 4000 4000\"><filter id=\"b\" x=\"-1\" y=\"-1\" width=\"3\" height=\"3\"><feGaussianBlur stdDeviation=\"800\"/><feMorphology radius=\"200\"/></filter><rect width=\"4000\" height=\"4000\" fill=\"teal\" filter=\"url(#b)\"/></svg>")?;
    add(
        "content/blur-heavy-filter.svg",
        Ready,
        Some(true),
        "huge blur + morphology filter",
    );
    put(dir, "content/pattern-tiny-tiles.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\" viewBox=\"0 0 1000 1000\"><pattern id=\"p\" width=\"0.05\" height=\"0.05\" patternUnits=\"userSpaceOnUse\"><rect width=\"0.025\" height=\"0.025\" fill=\"black\"/></pattern><rect width=\"1000\" height=\"1000\" fill=\"url(#p)\"/></svg>")?;
    add(
        "content/pattern-tiny-tiles.svg",
        Ready,
        None,
        "pattern with 0.05-unit tiles over 1000x1000",
    );
    put(dir, "content/dash-storm.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\" viewBox=\"0 0 1000 1000\"><path d=\"M0 0 L1000 1000 M1000 0 L0 1000 M0 500 H1000 M500 0 V1000\" stroke=\"black\" stroke-width=\"2\" stroke-dasharray=\"0.01 0.01\"/></svg>")?;
    add(
        "content/dash-storm.svg",
        Ready,
        None,
        "dasharray 0.01 over ~4,800 units (~240k dashes)",
    );
    put(dir, "content/use-self-reference.svg", svg_doc("<use id=\"u\" href=\"#u\"/><g id=\"a\"><use href=\"#b\"/></g><g id=\"b\"><use href=\"#a\"/></g><rect width=\"50\" height=\"50\"/>").as_bytes())?;
    add(
        "content/use-self-reference.svg",
        Ready,
        None,
        "self- and mutually-recursive <use>",
    );
    {
        // "Billion laughs" through <use>: 10^k instances.
        let levels = 6;
        let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><defs><rect id=\"l0\" width=\"1\" height=\"1\"/>");
        for l in 1..=levels {
            let _ = write!(s, "<g id=\"l{l}\">");
            for k in 0..10 {
                let _ = write!(s, "<use href=\"#l{}\" x=\"{k}\"/>", l - 1);
            }
            s.push_str("</g>");
        }
        let _ = write!(s, "</defs><use href=\"#l{levels}\"/></svg>");
        put(dir, "content/use-fanout-1e6.svg", s.as_bytes())?;
        add(
            "content/use-fanout-1e6.svg",
            Ready,
            None,
            "<use> fan-out bomb: 10^6 rendered instances from 60 elements",
        );
    }

    // ---- text
    {
        let mut w = create(dir, "text/massive-text-single-node.svg")?;
        w.write_all(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"200\" viewBox=\"0 0 800 200\"><text x=\"10\" y=\"100\" font-size=\"14\">")?;
        let mut written = 0u64;
        let mut buf = String::with_capacity(1 << 16);
        while written < p.massive_text_bytes {
            buf.clear();
            while buf.len() < 60_000 {
                buf.push_str(r.pick(PROSE));
                buf.push(' ');
            }
            w.write_all(buf.as_bytes())?;
            written += buf.len() as u64;
        }
        w.write_all(b"</text></svg>\n")?;
        w.flush()?;
        add(
            "text/massive-text-single-node.svg",
            LimitExceeded,
            Some(false),
            "massive_text_bytes in ONE <text> node (over max_text_node_chars)",
        );
    }
    {
        let mut w = create(dir, "text/massive-text-many-nodes.svg")?;
        let lines = (p.massive_text_bytes / 250).max(1);
        let _ = write!(w, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2000\" height=\"{}\" viewBox=\"0 0 2000 {}\">", lines * 14, lines * 14);
        let mut line = String::new();
        for i in 0..lines {
            line.clear();
            while line.len() < 220 {
                line.push_str(r.pick(PROSE));
                line.push(' ');
            }
            let _ = write!(
                w,
                "<text x=\"4\" y=\"{}\" font-size=\"12\">{}</text>",
                12 + i * 14,
                line
            );
        }
        w.write_all(b"</svg>\n")?;
        w.flush()?;
        add(
            "text/massive-text-many-nodes.svg",
            LimitExceeded,
            Some(false),
            "massive_text_bytes across ~250-byte <text> elements (over max_render_text_chars)",
        );
    }
    {
        // Largest permitted single run (default max_text_node_chars = 10,000).
        let mut run = String::new();
        while run.len() < 9_000 {
            run.push_str(r.pick(PROSE));
            run.push(' ');
        }
        put(
            dir,
            "text/long-text-run-9000.svg",
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"200\" viewBox=\"0 0 800 200\"><text x=\"10\" y=\"100\" font-size=\"14\">{}</text></svg>\n",
                &run[..9_000]
            )
            .as_bytes(),
        )?;
        add(
            "text/long-text-run-9000.svg",
            Ready,
            Some(true),
            "9,000 characters in one <text>: under max_text_node_chars, must render",
        );
    }

    // ---- entities / DTD
    put(dir, "dtd/entities-illustrator-style.svg", br#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd" [
  <!ENTITY ns_svg "http://www.w3.org/2000/svg">
  <!ENTITY company "Acme Process Controls">
  <!ENTITY st0 "fill:#2a6;stroke:#000;stroke-width:2">
]>
<svg xmlns="&ns_svg;" width="200" height="100" viewBox="0 0 200 100"><title>&company; pump skid</title><rect x="5" y="5" width="190" height="60" style="&st0;"/><text x="10" y="90">&company;</text></svg>
"#)?;
    add(
        "dtd/entities-illustrator-style.svg",
        Ready,
        Some(true),
        "internal DTD entities in xmlns, style and text",
    );
    {
        let mut s =
            String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE svg [\n<!ENTITY lol0 \"lol\">\n");
        for i in 1..10 {
            let _ = write!(s, "<!ENTITY lol{i} \"");
            for _ in 0..10 {
                let _ = write!(s, "&lol{};", i - 1);
            }
            s.push_str("\">\n");
        }
        s.push_str("]>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><text>&lol9;</text></svg>\n");
        put(dir, "dtd/billion-laughs.svg", s.as_bytes())?;
        add(
            "dtd/billion-laughs.svg",
            Analyzed,
            None,
            "entity expansion bomb",
        );
    }
    put(dir, "dtd/xxe-external-entity.svg", b"<?xml version=\"1.0\"?>\n<!DOCTYPE svg [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><text>&xxe;</text></svg>\n")?;
    add(
        "dtd/xxe-external-entity.svg",
        Analyzed,
        None,
        "external entity (must never be resolved)",
    );

    // ---- encodings
    let enc_doc = |decl: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"{decl}\"?>\n{}",
            labelled("Pumpe Größe Ω — encoding test", "Grüße ☃")
        )
    };
    let utf16 = |s: &str, le: bool| -> Vec<u8> {
        let mut v = if le {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for u in s.encode_utf16() {
            v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        v
    };
    put(
        dir,
        "encoding/utf16le-bom.svg",
        &utf16(&enc_doc("UTF-16"), true),
    )?;
    add(
        "encoding/utf16le-bom.svg",
        Ready,
        Some(true),
        "UTF-16 LE with BOM",
    );
    put(
        dir,
        "encoding/utf16be-bom.svg",
        &utf16(&enc_doc("UTF-16"), false),
    )?;
    add(
        "encoding/utf16be-bom.svg",
        Ready,
        Some(true),
        "UTF-16 BE with BOM",
    );
    let no_bom = utf16(&enc_doc("UTF-16"), true)[2..].to_vec();
    put(dir, "encoding/utf16le-no-bom.svg", &no_bom)?;
    add(
        "encoding/utf16le-no-bom.svg",
        ParseError,
        Some(false),
        "UTF-16 without BOM (unsupported)",
    );
    let mut bom8 = vec![0xEF, 0xBB, 0xBF];
    bom8.extend_from_slice(enc_doc("UTF-8").as_bytes());
    put(dir, "encoding/utf8-bom.svg", &bom8)?;
    add("encoding/utf8-bom.svg", Ready, Some(true), "UTF-8 with BOM");
    let mut latin1 = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><title>Caf".to_vec();
    latin1.extend_from_slice(&[0xE9, b' ', 0xC6, 0xF8]);
    latin1.extend_from_slice(b"</title><rect width=\"10\" height=\"10\"/></svg>\n");
    put(dir, "encoding/latin1-declared.svg", &latin1)?;
    add(
        "encoding/latin1-declared.svg",
        Ready,
        Some(true),
        "ISO-8859-1 declared, non-ASCII bytes",
    );
    let mut bad =
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><title>bad ".to_vec();
    bad.extend_from_slice(&[0xC3, 0x28, 0xFF]);
    bad.extend_from_slice(b"</title></svg>");
    put(dir, "encoding/invalid-utf8.svg", &bad)?;
    add(
        "encoding/invalid-utf8.svg",
        ParseError,
        Some(false),
        "invalid UTF-8 without declaration",
    );

    // ---- geometry
    put(dir, "geometry/no-viewbox-no-size.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect x=\"10\" y=\"10\" width=\"80\" height=\"40\" fill=\"#43a047\"/></svg>")?;
    add(
        "geometry/no-viewbox-no-size.svg",
        Ready,
        Some(true),
        "no viewBox, no width/height",
    );
    put(dir, "geometry/percent-size.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" height=\"50%\" viewBox=\"0 0 400 200\"><circle cx=\"200\" cy=\"100\" r=\"90\" fill=\"#8e24aa\"/></svg>")?;
    add(
        "geometry/percent-size.svg",
        Ready,
        Some(true),
        "percentage width/height",
    );
    put(
        dir,
        "geometry/nonzero-origin-viewbox.svg",
        NONZERO_ORIGIN.as_bytes(),
    )?;
    add(
        "geometry/nonzero-origin-viewbox.svg",
        Ready,
        Some(true),
        "viewBox=\"-500 300 1000 800\" (crop test)",
    );
    put(dir, "geometry/huge-dimensions.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000000\" height=\"1000000\" viewBox=\"0 0 10 10\"><circle cx=\"5\" cy=\"5\" r=\"4\"/></svg>")?;
    add(
        "geometry/huge-dimensions.svg",
        Ready,
        Some(true),
        "1,000,000 px square canvas",
    );
    put(dir, "geometry/zero-size.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"0\" height=\"0\"><rect width=\"10\" height=\"10\"/></svg>")?;
    add("geometry/zero-size.svg", Analyzed, None, "width=0 height=0");
    put(dir, "geometry/negative-viewbox.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 -10 10\"><rect width=\"10\" height=\"10\"/></svg>")?;
    add(
        "geometry/negative-viewbox.svg",
        Analyzed,
        None,
        "negative viewBox width",
    );
    put(dir, "geometry/no-namespace.svg", b"<svg viewBox=\"0 0 20 20\" width=\"20\" height=\"20\"><rect width=\"20\" height=\"10\" fill=\"red\"/></svg>")?;
    add(
        "geometry/no-namespace.svg",
        Ready,
        Some(true),
        "root without xmlns (lenient)",
    );
    put(
        dir,
        "geometry/empty-svg.svg",
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>",
    )?;
    add(
        "geometry/empty-svg.svg",
        Ready,
        Some(true),
        "valid but empty document",
    );

    // ---- unicode names and content
    let uni: &[(&str, &str, &str)] = &[
        (
            "unicode/مضخة-التحكم-rtl.svg",
            "مضخة التحكم الرئيسية",
            "\u{200F}محرك ٣\u{200F}",
        ),
        ("unicode/משאבה-ראשית.svg", "משאבה ראשית", "בקר מנוע"),
        (
            "unicode/网络交换机-图标.svg",
            "网络交换机",
            "主控制器 以太网",
        ),
        (
            "unicode/ポンプ制御パネル.svg",
            "ポンプ制御パネル",
            "モーター制御",
        ),
        ("unicode/펌프-제어.svg", "펌프 제어", "이더넷 스위치"),
        (
            "unicode/🚀-launch-👍🏽-👨\u{200D}👩\u{200D}👧.svg",
            "🚀 launch 👍🏽",
            "family 👨\u{200D}👩\u{200D}👧 flag 🇩🇪",
        ),
        ("unicode/café-nfc.svg", "café NFC", "naïve résumé"),
        (
            "unicode/cafe\u{0301}-nfd.svg",
            "cafe\u{0301} NFD",
            "nai\u{0308}ve re\u{0301}sume\u{0301}",
        ),
        (
            "unicode/zalgo-z\u{0337}\u{0322}a\u{0338}l\u{0336}g\u{0335}o.svg",
            "z\u{0337}\u{0322}\u{0310}a\u{0338}\u{0300}l\u{0336}g\u{0335}\u{0301}o\u{0334}",
            "c\u{0327}\u{0301}\u{0302}\u{0303}\u{0304}",
        ),
        (
            "unicode/invoice-\u{202E}gnp.svg",
            "invoice \u{202E}gnp.exe",
            "bidi \u{202E}override\u{202C} text",
        ),
        (
            "unicode/zero\u{200B}width\u{200D}joiner.svg",
            "zero\u{200B}width",
            "soft\u{00AD}hyphen",
        ),
        (
            "ünïcødé/目录/Ελληνικά σύμβολα.svg",
            "Ελληνικά σύμβολα",
            "αντλία βαλβίδα",
        ),
        (
            "special chars/pump (copy) #2 [final] & co's ~$tmp; %20.svg",
            "special characters",
            "& < > \" '",
        ),
    ];
    for (rel, title, text) in uni {
        put(dir, rel, labelled(title, text).as_bytes())?;
        add(
            rel,
            Ready,
            Some(true),
            "unicode / special-character name and content",
        );
    }
    let long = format!("unicode/{}.svg", "very-long-file-name-".repeat(11));
    put(dir, &long, labelled("long name", "long").as_bytes())?;
    add(&long, Ready, Some(true), "~225-byte file name");
    put(
        dir,
        "UPPERCASE-EXTENSION.SVG",
        labelled("upper", "SVG").as_bytes(),
    )?;
    add(
        "UPPERCASE-EXTENSION.SVG",
        Ready,
        Some(true),
        "upper-case .SVG extension",
    );

    // ---- hidden / decoys
    put(
        dir,
        ".git/objects/4f/hidden-object.svg",
        labelled("hidden", "git").as_bytes(),
    )?;
    add(
        ".git/objects/4f/hidden-object.svg",
        Skipped,
        None,
        "inside .git/",
    );
    put(
        dir,
        "geometry/.hidden-file.svg",
        labelled("hidden", "file").as_bytes(),
    )?;
    add("geometry/.hidden-file.svg", Skipped, None, "dot-file");
    put(
        dir,
        ".cache/nested/deeper/thumb.svg",
        labelled("hidden", "cache").as_bytes(),
    )?;
    add(
        ".cache/nested/deeper/thumb.svg",
        Skipped,
        None,
        "inside .cache/",
    );
    put(
        dir,
        "content/backup.svg.bak",
        labelled("bak", "bak").as_bytes(),
    )?;
    add("content/backup.svg.bak", Skipped, None, ".svg.bak decoy");
    put(dir, "content/.svg", labelled("only ext", "x").as_bytes())?;
    add("content/.svg", Skipped, None, "file named exactly '.svg'");

    Ok(cases)
}

pub const NONZERO_ORIGIN: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"500\" height=\"400\" viewBox=\"-500 300 1000 800\"><rect x=\"-500\" y=\"300\" width=\"500\" height=\"400\" fill=\"#e53935\"/><rect x=\"0\" y=\"300\" width=\"500\" height=\"400\" fill=\"#43a047\"/><rect x=\"-500\" y=\"700\" width=\"500\" height=\"400\" fill=\"#1e88e5\"/><rect x=\"0\" y=\"700\" width=\"500\" height=\"400\" fill=\"#fdd835\"/><text x=\"-250\" y=\"500\" font-size=\"60\">Q1</text></svg>\n";

fn fmt_mb(bytes: u64) -> String {
    let mb = bytes as f64 / MB as f64;
    if (mb - mb.round()).abs() < 1e-9 {
        format!("{}mb", mb.round() as u64)
    } else {
        format!("{:.1}mb", mb)
    }
}

// ============================================================================ hashing

/// BLAKE3 over (relative path, length, content) of every file under `dir`, sorted by
/// path. Returns (hex digest, file count, total bytes).
pub fn hash_tree(dir: &Path) -> io::Result<(String, usize, u64)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for e in walkdir::WalkDir::new(dir).follow_links(false) {
        let e = e.map_err(io::Error::other)?;
        if e.file_type().is_file() {
            let rel = e
                .path()
                .strip_prefix(dir)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((rel, e.path().to_path_buf()));
        }
    }
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut h = blake3::Hasher::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for (rel, path) in &files {
        let len = fs::metadata(path)?.len();
        h.update(rel.as_bytes());
        h.update(&[0]);
        h.update(&len.to_le_bytes());
        let mut f = File::open(path)?;
        loop {
            let k = f.read(&mut buf)?;
            if k == 0 {
                break;
            }
            h.update(&buf[..k]);
        }
        total += len;
    }
    Ok((h.finalize().to_hex().to_string(), files.len(), total))
}

// ============================================================================ CLI

fn prepare_dir(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        let empty = fs::read_dir(dir)?.next().is_none();
        if dir.join(MARKER).exists() {
            fs::remove_dir_all(dir)?;
        } else if !empty {
            return Err(io::Error::other(format!(
                "{} exists, is not empty and has no {MARKER} marker; refusing to overwrite",
                dir.display()
            )));
        }
    }
    fs::create_dir_all(dir)
}

pub fn generate(which: char, out: &Path, seed: u64) -> io::Result<Summary> {
    let dir = out.join(which.to_string());
    prepare_dir(&dir)?;
    let sub = derive(seed, which as u64, 0xDA7A);
    let summary = match which {
        'A' => generate_tree(&dir, 1_000, sub, false)?,
        'B' => generate_tree(&dir, 10_000, sub, false)?,
        'C' => generate_tree(&dir, 50_000, sub, false)?,
        'D' => generate_tree(&dir, 10_000, sub, true)?,
        'E' => {
            let cases = write_pathological(&dir, &PathoParams::full(), sub)?;
            let mut manifest = String::from("# path\texpect\tthumbnail\tnote\n");
            let mut s = Summary::default();
            for c in &cases {
                let _ = writeln!(
                    manifest,
                    "{}\t{:?}\t{}\t{}",
                    c.rel,
                    c.expect,
                    match c.thumb {
                        Some(true) => "ok",
                        Some(false) => "error",
                        None => "any",
                    },
                    c.note
                );
                match c.expect {
                    Expect::Skipped => s.other_files += 1,
                    _ => s.svg_files += 1,
                }
            }
            put(&dir, "MANIFEST.tsv", manifest.as_bytes())?;
            s.bytes = hash_tree(&dir)?.2;
            s
        }
        _ => return Err(io::Error::other(format!("unknown dataset '{which}'"))),
    };
    fs::write(
        dir.join(MARKER),
        format!(
            "svg-browser dataset {which} seed={seed} svgs={}\n",
            summary.svg_files
        ),
    )?;
    Ok(summary)
}

fn usage() -> ! {
    eprintln!(
        "usage: gen_dataset <A|B|C|D|E|all> <out_dir> [--seed N]\n       gen_dataset hash <dataset_dir>"
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        usage();
    }
    if args[0] == "hash" {
        let started = Instant::now();
        match hash_tree(Path::new(&args[1])) {
            Ok((hex, count, bytes)) => {
                println!(
                    "{hex}  {count} files  {bytes} bytes  ({:.1} s)",
                    started.elapsed().as_secs_f64()
                );
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let mut seed = DEFAULT_SEED;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                seed = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage());
                i += 2;
            }
            _ => usage(),
        }
    }
    let sets: Vec<char> = match args[0].to_ascii_uppercase().as_str() {
        "ALL" => vec!['A', 'B', 'C', 'D', 'E'],
        s if s.len() == 1 && "ABCDE".contains(s) => vec![s.chars().next().unwrap()],
        _ => usage(),
    };
    let out = PathBuf::from(&args[1]);
    println!("| Set | SVGs (visible) | hidden SVGs | decoys | MB | time |");
    println!("|-----|---------------:|------------:|-------:|---:|-----:|");
    for set in sets {
        let started = Instant::now();
        match generate(set, &out, seed) {
            Ok(s) => println!(
                "| {set} | {} | {} | {} | {:.1} | {:.1} s |",
                s.svg_files,
                s.hidden_svgs,
                s.other_files,
                s.bytes as f64 / MB as f64,
                started.elapsed().as_secs_f64()
            ),
            Err(e) => {
                eprintln!("error generating {set}: {e}");
                std::process::exit(1);
            }
        }
    }
}
