// Deterministic procedural SVG library for the in-browser mock backend.
// Metadata is generated eagerly (cheap); SVG markup is generated lazily per asset.
import type { ProcessingState, ViewBox } from "../types";

export interface MockAsset {
  id: number;
  filename: string;
  relPath: string;
  relDir: string;
  fileSize: number;
  mtimeMs: number;
  fingerprint: string;
  /** Final state once "processed". */
  finalState: ProcessingState;
  state: ProcessingState;
  parseError: string | null;
  width: number | null;
  height: number | null;
  viewBox: ViewBox | null;
  elementCount: number | null;
  title: string;
  desc: string;
  /** Visible <text> labels. */
  texts: string[];
  /** IDs and class names. */
  idClass: string;
  kind: "icon" | "c4" | "navy" | "diagram" | "broken";
  seed: number;
  folder: string;
  noun: string;
  variant: string;
}

// ---------------------------------------------------------------------------
// PRNG / hashing
// ---------------------------------------------------------------------------

export function hashString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hex16(n: number, m: number): string {
  return (n >>> 0).toString(16).padStart(8, "0") + (m >>> 0).toString(16).padStart(8, "0");
}

const titleCase = (s: string) =>
  s
    .split("-")
    .map((w) => (w.length <= 3 && /^(io|ups|vfd|hmi|plc|cpu|ct|mcc|api|spa|cdn|uav|waf|ids|dmz|rtd|nat|wan|isp|lte|sfp|usb|pdf|svg|zip|ph)$/.test(w) ? w.toUpperCase() : w[0].toUpperCase() + w.slice(1)))
    .join(" ");

// ---------------------------------------------------------------------------
// Taxonomy
// ---------------------------------------------------------------------------

const FOLDERS: Record<string, string[]> = {
  "electrical/power": ["transformer", "generator", "battery", "motor", "inverter", "rectifier", "busbar", "ground", "fuse", "capacitor", "inductor", "resistor", "solar-panel", "wind-turbine", "ups", "power-supply"],
  "electrical/switchgear": ["circuit-breaker", "contactor", "relay", "disconnect-switch", "isolator", "recloser", "switchboard", "panelboard", "mcc-bucket", "load-break-switch", "transfer-switch", "fuse-holder", "terminal-block", "surge-arrester", "meter", "ct"],
  "electrical/sensors": ["temperature-sensor", "pressure-sensor", "flow-meter", "level-sensor", "proximity-sensor", "encoder", "accelerometer", "thermocouple", "rtd", "strain-gauge", "hall-sensor", "photo-eye", "gas-detector", "humidity-sensor", "vibration-sensor", "limit-switch"],
  "network/routers": ["router", "core-router", "edge-router", "gateway", "firewall-router", "vpn-gateway", "modem", "lte-router", "sd-wan", "branch-router", "load-balancer", "nat", "wan-link", "isp", "cloud", "internet"],
  "network/switches": ["ethernet-switch", "managed-switch", "poe-switch", "layer3-switch", "core-switch", "access-switch", "fiber-switch", "industrial-switch", "patch-panel", "media-converter", "sfp-module", "hub", "vlan", "trunk", "stack", "ethernet-port"],
  "network/wireless": ["access-point", "wifi", "antenna", "radio", "satellite", "bluetooth", "mesh-node", "cell-tower", "lora-gateway", "zigbee", "microwave-link", "rfid-reader", "wireless-bridge", "signal", "repeater", "beacon"],
  "network/security": ["firewall", "shield", "lock", "key", "certificate", "ids", "waf", "vpn", "badge", "fingerprint", "token", "audit", "siem", "honeypot", "dmz", "proxy"],
  "controls/plc": ["plc", "cpu-module", "io-module", "analog-input", "digital-output", "remote-io", "safety-plc", "rack", "backplane", "power-module", "comms-module", "profinet", "modbus", "ethernet-ip", "can-bus", "fieldbus"],
  "controls/hmi": ["hmi-panel", "operator-station", "alarm", "trend", "faceplate", "pushbutton", "selector-switch", "pilot-light", "e-stop", "keypad", "touchscreen", "gauge", "setpoint", "mode-switch", "beacon-tower", "horn"],
  "controls/valves": ["gate-valve", "ball-valve", "butterfly-valve", "check-valve", "control-valve", "solenoid-valve", "relief-valve", "globe-valve", "needle-valve", "three-way-valve", "actuator", "positioner", "damper", "plug-valve", "diaphragm-valve", "manual-valve"],
  "controls/drives": ["vfd", "servo-drive", "stepper-drive", "soft-starter", "motor-starter", "inverter-drive", "brake-resistor", "line-reactor", "emc-filter", "encoder-feedback", "gearbox", "coupling", "spindle", "conveyor", "fan", "blower"],
  "software/c4/context": ["person", "software-system", "external-system", "enterprise-boundary", "user", "admin", "customer", "partner-system", "mobile-user", "operator", "auditor", "legacy-system", "saas", "data-provider", "regulator", "service-desk"],
  "software/c4/container": ["web-app", "api", "database", "message-queue", "cache", "file-store", "spa", "mobile-app", "worker", "scheduler", "event-bus", "search-index", "data-lake", "gateway-container", "auth-service", "cdn"],
  "software/c4/component": ["controller", "repository", "service", "adapter", "facade", "handler", "validator", "mapper", "client", "publisher", "subscriber", "policy", "use-case", "presenter", "gateway-component", "module"],
  "ui/icons/arrows": ["arrow-up", "arrow-down", "arrow-left", "arrow-right", "chevron-up", "chevron-down", "chevron-left", "chevron-right", "refresh", "undo", "redo", "expand", "collapse", "swap", "external-link", "corner-down"],
  "ui/icons/actions": ["add", "remove", "edit", "delete", "save", "copy", "paste", "cut", "search", "filter", "sort", "settings", "share", "download", "upload", "print"],
  "ui/icons/media": ["play", "pause", "stop", "record", "skip-forward", "skip-back", "volume", "mute", "camera", "image", "video", "microphone", "headphones", "speaker", "film", "music"],
  "ui/icons/files": ["file", "folder", "folder-open", "archive", "document", "spreadsheet", "presentation", "pdf", "code-file", "svg-file", "zip", "attachment", "clipboard", "notebook", "bookmark", "inbox"],
  "ui/icons/status": ["check", "close", "info", "warning", "error", "help", "star", "heart", "bell", "flag", "pin", "clock", "calendar", "eye", "eye-off", "circle-dot"],
  "ui/icons/devices": ["laptop", "desktop", "server", "tablet", "phone", "watch", "printer", "monitor", "keyboard", "mouse", "cpu", "memory", "hard-drive", "usb", "plug", "battery-charging"],
  "navy/symbols": ["surface-combatant", "submarine", "aircraft-carrier", "helicopter", "uav", "radar", "sonar", "torpedo", "mine", "buoy", "anchor", "compass", "waypoint", "hostile-track", "friendly-track", "unknown-track"],
  "process/pumps": ["centrifugal-pump", "displacement-pump", "gear-pump", "diaphragm-pump", "vacuum-pump", "metering-pump", "screw-pump", "compressor", "ejector", "turbine", "agitator", "mixer", "heat-exchanger", "cooler", "heater", "boiler"],
  "process/tanks": ["tank", "vessel", "silo", "hopper", "reactor", "column", "separator", "drum", "sphere", "filter", "strainer", "cyclone", "scrubber", "condenser", "evaporator", "dryer"],
  "process/instruments": ["pressure-indicator", "temperature-indicator", "flow-indicator", "level-indicator", "analyzer", "controller-loop", "transmitter", "orifice-plate", "rotameter", "sight-glass", "thermowell", "pressure-gauge", "ph-probe", "conductivity-probe", "totalizer", "recorder"],
  "ui/brand": ["logo", "wordmark", "monogram", "badge-mark", "seal", "emblem", "favicon", "app-icon", "splash", "watermark", "lockup", "glyph", "avatar", "banner", "sticker", "stamp"],
};

const VARIANTS = ["", "-outline", "-filled", "-alt", "-sm", "-lg", "-duotone", "-2", "-3", "-mono", "-bold", "-thin", "-rounded", "-sharp", "-v2"];

const PALETTE: Record<string, string> = {
  electrical: "#e08a00",
  network: "#3b74e6",
  controls: "#12a08a",
  software: "#1168bd",
  ui: "#6b7a90",
  navy: "#3d63c9",
  process: "#2f9e5b",
  brand: "#d04f8a",
};

const DIAGRAM_KINDS: Record<string, string[]> = {
  architecture: ["system-overview", "control-system-architecture", "ot-it-integration", "data-flow", "deployment-view", "security-zones", "integration-landscape", "software-architecture"],
  network: ["plant-network-topology", "ethernet-ring", "vlan-layout", "wireless-coverage", "firewall-zones", "fiber-backbone", "ethernet-control-network", "remote-access"],
  power: ["one-line-diagram", "power-distribution", "ups-topology", "substation-layout", "grounding-plan", "load-schedule", "motor-control-center", "emergency-power"],
  controls: ["motor-control-schematic", "io-allocation", "interlock-logic", "pid-loop-overview", "safety-functions", "panel-layout", "supervisory-control", "sequence-of-operations"],
  process: ["p-and-id-area-100", "cooling-water-system", "compressed-air", "chemical-dosing", "wastewater-treatment", "steam-distribution", "tank-farm", "boiler-feedwater"],
};

export const DIAGRAM_LABELS = [
  "Ethernet Control Interface", "Motor Controller", "Supervisory Controller", "Power Distribution Unit", "Sensor Gateway",
  "Safety PLC", "Historian Server", "Operator Workstation", "Engineering Workstation", "Core Switch", "Firewall DMZ",
  "Remote I/O Rack", "Variable Frequency Drive", "Pump Station 3", "Battery Energy Storage", "Main Breaker 480V",
  "Transformer T-101", "Generator G-1", "Fiber Ring A", "Wireless Bridge", "Data Diode", "Time Server (PTP)",
  "Alarm Management", "Batch Server", "Web HMI Gateway", "MQTT Broker", "OPC UA Server", "Cloud Connector",
  "Fire & Gas Controller", "Emergency Shutdown", "Level Transmitter LT-201", "Flow Control Valve FCV-12",
  "Cooling Water Loop", "Compressor Skid", "Substation Automation", "Protection Relay", "Network Management",
  "Domain Controller", "Backup Server", "Patch Management", "Quality Lab LIMS", "Maintenance CMMS",
  "Ethernet/IP Adapter", "CAN Bus Interface", "Serial Gateway RS-485", "Motor Control Center", "UPS 20 kVA",
  "Solar Inverter Array",
];

// ---------------------------------------------------------------------------
// Library generation
// ---------------------------------------------------------------------------

export interface LibrarySpec {
  seed: number;
  /** Folder prefixes to include (empty = all). */
  include?: string[];
  /** Max icons. */
  iconLimit?: number;
  diagrams?: number;
}

const BASE_TIME = Date.UTC(2026, 6, 1);

export function generateLibrary(spec: LibrarySpec): MockAsset[] {
  const rnd = mulberry32(spec.seed);
  const out: MockAsset[] = [];
  const include = spec.include ?? [];
  const allowed = (path: string) => include.length === 0 || include.some((p) => path.startsWith(p));
  let nextId = 1;

  const pushAsset = (a: Omit<MockAsset, "id" | "fingerprint" | "mtimeMs" | "state"> & { mtimeMs?: number }) => {
    const id = nextId++;
    const mtimeMs = a.mtimeMs ?? BASE_TIME - Math.floor(rnd() * 400) * 86400000 - Math.floor(rnd() * 86400000);
    const fp = hex16(hashString(a.relPath + ":" + a.fileSize), hashString(String(mtimeMs) + a.relPath));
    out.push({ ...a, id, mtimeMs, fingerprint: fp, state: a.finalState });
  };

  // Icons: interleave folders so discovery order looks like a real tree walk (folder by folder).
  let iconCount = 0;
  const iconLimit = spec.iconLimit ?? Infinity;
  for (const [folder, nouns] of Object.entries(FOLDERS)) {
    if (!allowed(folder)) continue;
    const top = folder.split("/")[0];
    for (const variant of VARIANTS) {
      for (const noun of nouns) {
        if (iconCount >= iconLimit) break;
        const filename = `${noun}${variant}.svg`;
        const relPath = `${folder}/${filename}`;
        const seed = hashString(relPath);
        const r = mulberry32(seed);
        const kind: MockAsset["kind"] = folder.includes("/c4/") ? "c4" : top === "navy" ? "navy" : "icon";
        const size = kind === "c4" ? [160, 100] : kind === "navy" ? [64, 64] : variant === "-sm" ? [16, 16] : variant === "-lg" ? [48, 48] : [24, 24];
        const titled = r() < 0.7;
        const desc =
          top === "electrical" && r() < 0.5
            ? "Symbol per IEC 60617"
            : folder === "process/instruments"
              ? "ISA-5.1 instrument bubble"
              : top === "navy"
                ? "MIL-STD-2525 inspired tactical symbol"
                : "";
        const texts =
          kind === "c4"
            ? [titleCase(noun), folder.endsWith("context") ? "[Software System]" : folder.endsWith("container") ? "[Container]" : "[Component]"]
            : kind === "navy"
              ? [noun.includes("hostile") ? "H" : noun.includes("friendly") ? "F" : noun.includes("unknown") ? "U" : ""].filter(Boolean)
              : [];
        pushAsset({
          filename,
          relPath,
          relDir: folder,
          fileSize: 380 + Math.floor(r() * 2400) + (kind === "c4" ? 900 : 0),
          finalState: "ready",
          parseError: null,
          width: size[0],
          height: size[1],
          viewBox: { minX: 0, minY: 0, width: size[0] === 16 || size[0] === 48 ? 24 : size[0], height: size[0] === 16 || size[0] === 48 ? 24 : size[1] },
          elementCount: 3 + Math.floor(r() * 14),
          title: titled ? titleCase(noun) : "",
          desc,
          texts,
          idClass: `icon-${noun}${r() < 0.4 ? " st0 st1" : ""}${variant ? " " + variant.slice(1) : ""}`,
          kind,
          seed,
          folder,
          noun,
          variant,
        });
        iconCount++;
      }
    }
  }

  // Diagrams
  const diagramCount = spec.diagrams ?? 200;
  const kinds = Object.keys(DIAGRAM_KINDS);
  for (let i = 0; i < diagramCount; i++) {
    const kind = kinds[i % kinds.length];
    const folder = `diagrams/${kind}`;
    if (!allowed(folder) && !allowed("diagrams")) continue;
    const bases = DIAGRAM_KINDS[kind];
    const base = bases[Math.floor(i / kinds.length) % bases.length];
    const unit = Math.floor(i / (kinds.length * bases.length)) + 1;
    const rev = String.fromCharCode(65 + (i % 4));
    const filename = `${base}${unit > 1 ? `-unit-${unit}` : ""}-rev-${rev.toLowerCase()}.svg`;
    const relPath = `${folder}/${filename}`;
    const seed = hashString(relPath);
    const r = mulberry32(seed);
    const w = [1600, 2400, 3200, 4800, 1200][Math.floor(r() * 5)];
    const h = Math.round(w * (0.5 + r() * 0.35));
    const offset = i % 9 === 4;
    const vb: ViewBox = offset ? { minX: -w / 4, minY: -h / 5, width: w, height: h } : { minX: 0, minY: 0, width: w, height: h };
    const n = 6 + Math.floor(r() * 9);
    const labels: string[] = [];
    const pool = [...DIAGRAM_LABELS];
    for (let k = 0; k < n && pool.length; k++) labels.push(pool.splice(Math.floor(r() * pool.length), 1)[0]);
    pushAsset({
      filename,
      relPath,
      relDir: folder,
      fileSize: 18000 + Math.floor(r() * 900000) + (i === 37 ? 12_400_000 : 0),
      finalState: "ready",
      parseError: null,
      width: w,
      height: h,
      viewBox: vb,
      elementCount: 60 + n * 9 + Math.floor(r() * 400),
      title: `${titleCase(base)} — Rev ${rev}`,
      desc: r() < 0.5 ? `Drawing ${kind.toUpperCase()}-${1000 + i}. Approved for construction. Source: plant engineering.` : "",
      texts: labels,
      idClass: `layer-1 layer-annotations ${labels.length > 8 ? "cls-1 cls-2" : "cls-1"}`,
      kind: "diagram",
      seed,
      folder,
      noun: base,
      variant: `-rev-${rev.toLowerCase()}`,
    });
  }

  // Pathological entries (only in the full library)
  if (include.length === 0) {
    const broken: [string, ProcessingState, string | null, number][] = [
      ["diagrams/scans/full-plant-scan-highres.svg", "limit_exceeded", null, 48_200_000],
      ["diagrams/scans/site-survey-embedded-raster.svg", "limit_exceeded", null, 31_900_000],
      ["diagrams/legacy/cad-export-unclosed.svg", "parse_error", "XML error: unexpected end of stream at 1:2048", 2048],
      ["ui/icons/status/broken-export.svg", "parse_error", "XML error: invalid name token at 3:14", 611],
    ];
    for (const [relPath, state, err, size] of broken) {
      const parts = relPath.split("/");
      pushAsset({
        filename: parts[parts.length - 1],
        relPath,
        relDir: parts.slice(0, -1).join("/"),
        fileSize: size,
        finalState: state,
        parseError: err,
        width: null,
        height: null,
        viewBox: null,
        elementCount: null,
        title: "",
        desc: "",
        texts: [],
        idClass: "",
        kind: "broken",
        seed: hashString(relPath),
        folder: parts.slice(0, -1).join("/"),
        noun: "",
        variant: "",
      });
    }
  }

  // Filesystem walks discover in (roughly) path order; ids follow discovery order.
  out.sort((a, b) => (a.relPath < b.relPath ? -1 : a.relPath > b.relPath ? 1 : 0));
  out.forEach((a, i) => (a.id = i + 1));
  return out;
}

// ---------------------------------------------------------------------------
// SVG markup
// ---------------------------------------------------------------------------

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function svgFor(a: MockAsset): string {
  switch (a.kind) {
    case "c4":
      return c4Svg(a);
    case "navy":
      return navySvg(a);
    case "diagram":
      return diagramSvg(a);
    case "broken":
      return brokenSvg(a);
    default:
      return iconSvg(a);
  }
}

function header(a: MockAsset, extra = ""): string {
  const vb = a.viewBox!;
  const t = a.title ? `<title>${esc(a.title)}</title>` : "";
  const d = a.desc ? `<desc>${esc(a.desc)}</desc>` : "";
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${a.width}" height="${a.height}" viewBox="${vb.minX} ${vb.minY} ${vb.width} ${vb.height}"${extra}>${t}${d}`;
}

type Glyph = (r: () => number) => string;

const GLYPHS: Record<string, Glyph> = {
  arrow: () => `<path d="M5 12h14M13 6l6 6-6 6"/>`,
  chevron: () => `<path d="M9 6l6 6-6 6"/>`,
  valve: () => `<path d="M3 7v10l9-5zM21 7v10l-9-5z"/><path d="M12 12V5M9 5h6"/>`,
  tank: (r) => `<rect x="6" y="3" width="12" height="18" rx="4"/><path d="M6 ${11 + Math.floor(r() * 5)}h12"/>`,
  rotor: () => `<circle cx="12" cy="12" r="8"/><path d="M12 4l6.9 12H5.1z"/>`,
  network: () => `<circle cx="12" cy="5" r="2.2"/><circle cx="5" cy="18" r="2.2"/><circle cx="19" cy="18" r="2.2"/><path d="M12 7.2v4.3M12 11.5L6.6 16.3M12 11.5l5.4 4.8"/>`,
  switchbox: (r) => `<rect x="3" y="7" width="18" height="10" rx="2"/>${[6, 9, 12, 15, 18].slice(0, 3 + Math.floor(r() * 3)).map((x) => `<path d="M${x} 11v2"/>`).join("")}`,
  gauge: (r) => `<path d="M4 16a8 8 0 1 1 16 0"/><path d="M12 16l${(r() * 8 - 4).toFixed(1)} -6"/><circle cx="12" cy="16" r="1.4"/>`,
  person: () => `<circle cx="12" cy="8" r="3.5"/><path d="M5 20c0-4 3.2-6.5 7-6.5s7 2.5 7 6.5"/>`,
  shield: () => `<path d="M12 3l7 3v5c0 4.6-3 8.2-7 10-4-1.8-7-5.4-7-10V6z"/><path d="M9 12l2 2 4-4"/>`,
  stack: () => `<rect x="4" y="4" width="16" height="5" rx="1.5"/><rect x="4" y="10" width="16" height="5" rx="1.5"/><rect x="4" y="16" width="16" height="4" rx="1.5"/><path d="M7 6.5h.01M7 12.5h.01"/>`,
  wave: () => `<circle cx="12" cy="17" r="1.6"/><path d="M8.5 13.5a5 5 0 0 1 7 0M5.5 10.5a9 9 0 0 1 13 0M2.8 7.6a13 13 0 0 1 18.4 0"/>`,
  doc: () => `<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M9 12h6M9 16h6"/>`,
  folder: () => `<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>`,
  chip: () => `<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="9.5" y="9.5" width="5" height="5"/><path d="M9 3v3M12 3v3M15 3v3M9 18v3M12 18v3M15 18v3M3 9h3M3 12h3M3 15h3M18 9h3M18 12h3M18 15h3"/>`,
  zigzag: () => `<path d="M2 12h4l1.5-4 3 8 3-8 3 8 1.5-4h4"/>`,
  bolt: () => `<path d="M13 2L4 14h7l-1 8 9-12h-7z"/>`,
  gear: () => `<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1L7 17M17 7l2.1-2.1"/><circle cx="12" cy="12" r="7"/>`,
  check: () => `<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.7 2.7L16 9.8"/>`,
  cross: () => `<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>`,
  plus: () => `<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M12 8v8M8 12h8"/>`,
  media: () => `<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M10 9.5v5l4.5-2.5z"/>`,
  cloud: () => `<path d="M7 18a4 4 0 0 1-.5-8A6 6 0 0 1 18 9a4.5 4.5 0 0 1-.5 9z"/>`,
  lock: () => `<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>`,
  device: () => `<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>`,
  star: () => `<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z"/>`,
  mark: (r) => `<circle cx="12" cy="12" r="9"/><path d="M${7 + r() * 2} 15l5-8 5 8z"/>`,
};

function glyphFor(noun: string, seed: number): string {
  const n = noun;
  if (/arrow|swap|external|corner|expand|collapse|undo|redo|refresh/.test(n)) return "arrow";
  if (/chevron/.test(n)) return "chevron";
  if (/valve|damper/.test(n)) return "valve";
  if (/tank|vessel|silo|drum|column|hopper|reactor|separator|sphere|dryer|evaporator|condenser|scrubber|cyclone/.test(n)) return "tank";
  if (/pump|fan|blower|compressor|motor|turbine|agitator|mixer|ejector/.test(n)) return "rotor";
  if (/ethernet|profinet|modbus|fieldbus|can-bus/.test(n)) return "switchbox";
  if (/router|gateway|mesh|hub|vlan|trunk|sd-wan|nat|internet|isp|wan|load-balancer/.test(n)) return "network";
  if (/switch|patch|converter|sfp|port|rack|backplane|module|io|input|output|terminal/.test(n)) return "switchbox";
  if (/sensor|gauge|indicator|meter|setpoint|trend|analyzer|transmitter|probe|recorder|totalizer|rotameter|encoder/.test(n)) return "gauge";
  if (/person|user|admin|customer|operator|auditor|regulator|avatar/.test(n)) return "person";
  if (/shield|firewall|security|waf|ids|dmz|audit|siem|honeypot|policy/.test(n)) return "shield";
  if (/server|database|stack|repository|cache|store|lake|index|queue/.test(n)) return "stack";
  if (/wifi|wireless|antenna|radio|signal|beacon|bluetooth|zigbee|lora|cell|microwave|rfid|repeater|satellite|volume|speaker/.test(n)) return "wave";
  if (/file|document|pdf|spreadsheet|presentation|notebook|clipboard|attachment|certificate|inbox|bookmark|archive|zip/.test(n)) return "doc";
  if (/folder/.test(n)) return "folder";
  if (/plc|cpu|memory|chip|drive|vfd|controller|service|handler|adapter|facade|mapper|module|presenter/.test(n)) return "chip";
  if (/resistor|fuse|inductor|capacitor|reactor|filter|strainer|orifice/.test(n)) return "zigzag";
  if (/battery|power|generator|bolt|ups|inverter|rectifier|surge|busbar|plug|ground|solar|wind/.test(n)) return "bolt";
  if (/settings|gear|coupling|gearbox|spindle|actuator|positioner/.test(n)) return "gear";
  if (/check|add|save/.test(n)) return "check";
  if (/close|error|delete|remove|cut|e-stop|stop|mute/.test(n)) return "cross";
  if (/plus|edit|copy|paste|keypad/.test(n)) return "plus";
  if (/play|pause|record|skip|video|film|camera|image|music|microphone|headphones/.test(n)) return "media";
  if (/cloud|saas|upload|download|share/.test(n)) return "cloud";
  if (/lock|key|vpn|token|badge|fingerprint|proxy/.test(n)) return "lock";
  if (/laptop|desktop|monitor|tablet|phone|watch|touchscreen|hmi|station|faceplate|printer|keyboard|mouse/.test(n)) return "device";
  if (/star|heart|flag|pin|logo|mark|seal|emblem|glyph|sticker|stamp|badge/.test(n)) return "star";
  const keys = Object.keys(GLYPHS);
  return keys[seed % keys.length];
}

function iconSvg(a: MockAsset): string {
  const r = mulberry32(a.seed);
  const top = a.folder.split("/")[0];
  const color = PALETTE[a.folder.includes("brand") ? "brand" : top] ?? "#6b7a90";
  const g = glyphFor(a.noun, a.seed);
  let body = GLYPHS[g](r);
  // Orientation hints
  const rot = /left|back|undo/.test(a.noun) ? 180 : /up/.test(a.noun) ? -90 : /down/.test(a.noun) ? 90 : 0;
  if (rot) body = `<g transform="rotate(${rot} 12 12)">${body}</g>`;
  const v = a.variant;
  const sw = v === "-bold" ? 2.6 : v === "-thin" ? 1.1 : 1.8;
  const join = v === "-sharp" ? "miter" : "round";
  const id = `icon-${a.noun}`;
  if (v === "-filled" || v === "-rounded" || v === "-2") {
    const rx = v === "-rounded" ? 11 : v === "-2" ? 3 : 5;
    const bg = v === "-2" ? shade(color, 0.85) : color;
    const fg = v === "-2" ? color : "#ffffff";
    return `${header(a)}<rect width="24" height="24" rx="${rx}" fill="${bg}"/><g id="${id}" transform="translate(4.2 4.2) scale(.65)" fill="none" stroke="${fg}" stroke-width="${sw + 0.5}" stroke-linecap="round" stroke-linejoin="${join}">${body}</g></svg>`;
  }
  if (v === "-duotone" || v === "-3") {
    const tint = shade(color, 0.78);
    return `${header(a)}<g id="${id}" fill="${tint}" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="${join}">${body}</g></svg>`;
  }
  const stroke = v === "-mono" ? "#4a5361" : color;
  const badge = r() < 0.18 ? `<circle cx="19" cy="5" r="3.2" fill="${v === "-alt" ? "#e5484d" : "#2f9e5b"}" stroke="none"/>` : "";
  return `${header(a)}<g id="${id}" fill="none" stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="${join}">${body}</g>${badge}</svg>`;
}

function shade(hex: string, toWhite: number): string {
  const n = parseInt(hex.slice(1), 16);
  const ch = (s: number) => Math.round(((n >> s) & 255) + (255 - ((n >> s) & 255)) * toWhite);
  return `#${((ch(16) << 16) | (ch(8) << 8) | ch(0)).toString(16).padStart(6, "0")}`;
}

function c4Svg(a: MockAsset): string {
  const isPerson = /person|user|admin|customer|operator|auditor|regulator/.test(a.noun);
  const external = /external|partner|legacy|saas|provider|regulator/.test(a.noun);
  const fill = isPerson ? "#08427b" : external ? "#8a8f98" : a.folder.endsWith("component") ? "#85bbf0" : a.folder.endsWith("container") ? "#438dd5" : "#1168bd";
  const fg = a.folder.endsWith("component") ? "#0b2540" : "#ffffff";
  const [name, kind] = a.texts;
  const rounded = a.variant === "-rounded" || a.variant === "-2" ? 18 : 6;
  const head = isPerson ? `<circle cx="80" cy="18" r="13" fill="${fill}" stroke="#ffffff" stroke-width="2"/>` : "";
  const top = isPerson ? 26 : 6;
  const shape =
    /database|store|lake/.test(a.noun) && !isPerson
      ? `<path d="M10 ${top + 8}a70 8 0 0 1 140 0v${88 - top - 10}a70 8 0 0 1-140 0z" fill="${fill}"/><path d="M10 ${top + 8}a70 8 0 0 0 140 0" fill="none" stroke="#ffffff" stroke-opacity=".5"/>`
      : `<rect x="6" y="${top}" width="148" height="${94 - top}" rx="${rounded}" fill="${fill}"${a.variant === "-outline" ? ` fill-opacity=".12" stroke="${fill}" stroke-width="2"` : ""}/>`;
  const textFill = a.variant === "-outline" ? fill : fg;
  return `${header(a)}${shape}${head}<text id="name" x="80" y="${top + (94 - top) / 2}" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="13" font-weight="600" fill="${textFill}">${esc(name)}</text><text class="c4-kind" x="80" y="${top + (94 - top) / 2 + 16}" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="9.5" fill="${textFill}" fill-opacity=".8">${esc(kind)}</text></svg>`;
}

function navySvg(a: MockAsset): string {
  const hostile = a.noun.includes("hostile") || ["torpedo", "mine"].includes(a.noun);
  const unknown = a.noun.includes("unknown") || a.noun === "buoy";
  const fill = hostile ? "#ff8080" : unknown ? "#ffff80" : "#80e0ff";
  const r = mulberry32(a.seed);
  const frame = hostile
    ? `<path d="M32 4L60 32 32 60 4 32z"/>`
    : unknown
      ? `<path d="M20 8a12 12 0 0 1 24 0 12 12 0 0 1 12 12 12 12 0 0 1 0 24 12 12 0 0 1-12 12 12 12 0 0 1-24 0A12 12 0 0 1 8 44a12 12 0 0 1 0-24A12 12 0 0 1 20 8z"/>`
      : /submarine|sonar|torpedo/.test(a.noun)
        ? `<path d="M6 22h52v8a26 26 0 0 1-52 0z"/>`
        : /helicopter|uav/.test(a.noun)
          ? `<path d="M6 44V30a26 26 0 0 1 52 0v14z"/>`
          : `<circle cx="32" cy="32" r="26"/>`;
  const glyph = /radar|sonar/.test(a.noun)
    ? `<path d="M22 40a14 14 0 0 1 20-20M26 36a8 8 0 0 1 10-10" fill="none"/><circle cx="32" cy="34" r="2" fill="#000"/>`
    : /anchor/.test(a.noun)
      ? `<path d="M32 18v26M24 24h16M20 36a12 12 0 0 0 24 0" fill="none"/>`
      : `<path d="M${20 + r() * 4} 38h24l-4 6H${24 + r() * 2}z" fill="#000"/><path d="M32 22v14" />`;
  const t = a.texts[0] ? `<text x="32" y="37" text-anchor="middle" font-family="Arial" font-weight="700" font-size="12" fill="#000">${a.texts[0]}</text>` : glyph;
  return `${header(a)}<g fill="${fill}" stroke="#000" stroke-width="2" stroke-linejoin="round">${frame}</g><g stroke="#000" stroke-width="2">${t}</g></svg>`;
}

const BOX_FILLS = ["#eef4ff", "#ecfbf3", "#fff6e6", "#f4efff", "#eef7f9"];
const BOX_STROKES = ["#3b74e6", "#2f9e5b", "#d68400", "#7b5cd6", "#1f8aa3"];

function diagramSvg(a: MockAsset): string {
  const r = mulberry32(a.seed);
  const vb = a.viewBox!;
  const transparent = a.seed % 4 === 1;
  const n = a.texts.length;
  const cols = Math.max(2, Math.round(Math.sqrt((n * vb.width) / vb.height)));
  const rows = Math.ceil(n / cols);
  const marginX = vb.width * 0.06;
  const top = vb.height * 0.16;
  const cellW = (vb.width - marginX * 2) / cols;
  const cellH = (vb.height - top - vb.height * 0.06) / rows;
  const bw = cellW * 0.7;
  const bh = Math.min(cellH * 0.55, bw * 0.45);
  const fs = Math.max(12, Math.min(bh * 0.2, bw * 0.075));
  const parts: string[] = [];
  if (!transparent) parts.push(`<rect id="background" x="${vb.minX}" y="${vb.minY}" width="${vb.width}" height="${vb.height}" fill="#ffffff"/>`);
  // Grid hint
  const gridStep = vb.width / 24;
  if (r() < 0.5) {
    const lines: string[] = [];
    for (let x = vb.minX + gridStep; x < vb.minX + vb.width; x += gridStep) lines.push(`M${x.toFixed(0)} ${vb.minY}v${vb.height}`);
    parts.push(`<path d="${lines.join("")}" stroke="#d9dee7" stroke-width="${(vb.width / 1600).toFixed(2)}" opacity=".6"/>`);
  }
  const centers: [number, number][] = [];
  const styleIdx = Math.floor(r() * BOX_FILLS.length);
  for (let i = 0; i < n; i++) {
    const c = i % cols;
    const row = Math.floor(i / cols);
    const cx = vb.minX + marginX + cellW * c + cellW / 2 + (r() - 0.5) * cellW * 0.12;
    const cy = vb.minY + top + cellH * row + cellH / 2;
    centers.push([cx, cy]);
  }
  // Connectors
  const conn: string[] = [];
  for (let i = 1; i < n; i++) {
    const [x0, y0] = centers[i - 1];
    const [x1, y1] = centers[Math.floor(r() * i)];
    const [xa, ya] = centers[i];
    const midY = (y0 + ya) / 2;
    conn.push(`M${xa.toFixed(0)} ${ya.toFixed(0)}V${midY.toFixed(0)}H${(r() < 0.5 ? x0 : x1).toFixed(0)}V${(r() < 0.5 ? y0 : y1).toFixed(0)}`);
  }
  parts.push(`<path class="connectors" d="${conn.join("")}" fill="none" stroke="#5b6472" stroke-width="${Math.max(2, vb.width / 700).toFixed(1)}" stroke-linejoin="round"/>`);
  for (let i = 0; i < n; i++) {
    const [cx, cy] = centers[i];
    const si = (styleIdx + (i % 3 === 2 ? 1 : 0)) % BOX_FILLS.length;
    parts.push(
      `<g id="node-${i + 1}"><rect x="${(cx - bw / 2).toFixed(1)}" y="${(cy - bh / 2).toFixed(1)}" width="${bw.toFixed(1)}" height="${bh.toFixed(1)}" rx="${(bh * 0.12).toFixed(1)}" fill="${BOX_FILLS[si]}" stroke="${BOX_STROKES[si]}" stroke-width="${Math.max(2, vb.width / 900).toFixed(1)}"/>` +
        `<text x="${cx.toFixed(1)}" y="${(cy + fs * 0.35).toFixed(1)}" text-anchor="middle" font-family="Segoe UI, Arial, sans-serif" font-size="${fs.toFixed(1)}" fill="#1f2937">${esc(a.texts[i])}</text></g>`,
    );
  }
  const tfs = Math.max(18, vb.width / 55);
  parts.push(
    `<text class="title" x="${(vb.minX + marginX).toFixed(0)}" y="${(vb.minY + top * 0.45).toFixed(0)}" font-family="Segoe UI, Arial, sans-serif" font-size="${tfs.toFixed(0)}" font-weight="600" fill="#111827">${esc(a.title)}</text>`,
  );
  // Title block
  const tbw = vb.width * 0.22;
  const tbh = vb.height * 0.07;
  parts.push(
    `<g class="title-block"><rect x="${(vb.minX + vb.width - marginX - tbw).toFixed(0)}" y="${(vb.minY + vb.height - tbh - vb.height * 0.02).toFixed(0)}" width="${tbw.toFixed(0)}" height="${tbh.toFixed(0)}" fill="none" stroke="#6b7280" stroke-width="${Math.max(1.5, vb.width / 1400).toFixed(1)}"/></g>`,
  );
  return `${header(a)}${parts.join("")}</svg>`;
}

function brokenSvg(a: MockAsset): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><title>${esc(a.filename)}</title></svg>`;
}
