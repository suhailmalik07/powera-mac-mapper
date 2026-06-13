//! Controller→keyboard mapping: profiles, key table, save/load, and injection.
//!
//! A [`Profile`] binds each [`ControlId`] to a macOS virtual key code. Given a
//! live [`GamepadState`], [`active_keys`] computes the set of keys that should
//! be held, and [`Injector`] posts the corresponding CGEvents (edge-triggered).

use crate::GamepadState;
use core_graphics::event::{CGEvent, CGEventTapLocation};
use core_graphics::event_source::CGEventSource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Every controller input that can be bound to a key.
///
/// Serializes with SCREAMING_SNAKE_CASE names (e.g. `DPAD_UP`, `L_STICK_UP`,
/// `LB`) so shared JSON files are readable and hand-editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlId {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    L3,
    R3,
    Start,
    Back,
    Guide,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LStickUp,
    LStickDown,
    LStickLeft,
    LStickRight,
    RStickUp,
    RStickDown,
    RStickLeft,
    RStickRight,
}

impl ControlId {
    /// All controls, in display order.
    pub const ALL: &'static [ControlId] = &[
        ControlId::A,
        ControlId::B,
        ControlId::X,
        ControlId::Y,
        ControlId::Lb,
        ControlId::Rb,
        ControlId::Lt,
        ControlId::Rt,
        ControlId::L3,
        ControlId::R3,
        ControlId::Start,
        ControlId::Back,
        ControlId::Guide,
        ControlId::DpadUp,
        ControlId::DpadDown,
        ControlId::DpadLeft,
        ControlId::DpadRight,
        ControlId::LStickUp,
        ControlId::LStickDown,
        ControlId::LStickLeft,
        ControlId::LStickRight,
        ControlId::RStickUp,
        ControlId::RStickDown,
        ControlId::RStickLeft,
        ControlId::RStickRight,
    ];

    /// Human-readable label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            ControlId::A => "A",
            ControlId::B => "B",
            ControlId::X => "X",
            ControlId::Y => "Y",
            ControlId::Lb => "LB (L)",
            ControlId::Rb => "RB (R)",
            ControlId::Lt => "LT (L2)",
            ControlId::Rt => "RT (R2)",
            ControlId::L3 => "L3 (stick click)",
            ControlId::R3 => "R3 (stick click)",
            ControlId::Start => "Start",
            ControlId::Back => "Back / Select",
            ControlId::Guide => "Guide / Home",
            ControlId::DpadUp => "D-pad Up",
            ControlId::DpadDown => "D-pad Down",
            ControlId::DpadLeft => "D-pad Left",
            ControlId::DpadRight => "D-pad Right",
            ControlId::LStickUp => "Left Stick Up",
            ControlId::LStickDown => "Left Stick Down",
            ControlId::LStickLeft => "Left Stick Left",
            ControlId::LStickRight => "Left Stick Right",
            ControlId::RStickUp => "Right Stick Up",
            ControlId::RStickDown => "Right Stick Down",
            ControlId::RStickLeft => "Right Stick Left",
            ControlId::RStickRight => "Right Stick Right",
        }
    }

    /// Is this control currently engaged in the given state?
    pub fn is_active(self, s: &GamepadState, deadzone: f32, walk: f32, trig: u8) -> bool {
        let (lx, ly) = s.left_stick_norm(deadzone);
        let (rx, ry) = s.right_stick_norm(deadzone);
        match self {
            ControlId::A => s.a,
            ControlId::B => s.b,
            ControlId::X => s.x,
            ControlId::Y => s.y,
            ControlId::Lb => s.lb,
            ControlId::Rb => s.rb,
            ControlId::Lt => s.left_trigger > trig,
            ControlId::Rt => s.right_trigger > trig,
            ControlId::L3 => s.l3,
            ControlId::R3 => s.r3,
            ControlId::Start => s.start,
            ControlId::Back => s.back,
            ControlId::Guide => s.guide,
            ControlId::DpadUp => s.dpad_up,
            ControlId::DpadDown => s.dpad_down,
            ControlId::DpadLeft => s.dpad_left,
            ControlId::DpadRight => s.dpad_right,
            ControlId::LStickUp => ly > walk,
            ControlId::LStickDown => ly < -walk,
            ControlId::LStickLeft => lx < -walk,
            ControlId::LStickRight => lx > walk,
            ControlId::RStickUp => ry > walk,
            ControlId::RStickDown => ry < -walk,
            ControlId::RStickLeft => rx < -walk,
            ControlId::RStickRight => rx > walk,
        }
    }
}

/// Selectable macOS virtual key codes, as (display name, key code).
/// Curated — enough for arcade/FPS bindings without overwhelming the UI.
pub const KEYS: &[(&str, u16)] = &[
    ("A", 0x00), ("B", 0x0B), ("C", 0x08), ("D", 0x02), ("E", 0x0E),
    ("F", 0x03), ("G", 0x05), ("H", 0x04), ("I", 0x22), ("J", 0x26),
    ("K", 0x28), ("L", 0x25), ("M", 0x2E), ("N", 0x2D), ("O", 0x1F),
    ("P", 0x23), ("Q", 0x0C), ("R", 0x0F), ("S", 0x01), ("T", 0x11),
    ("U", 0x20), ("V", 0x09), ("W", 0x0D), ("X", 0x07), ("Y", 0x10),
    ("Z", 0x06),
    ("0", 0x1D), ("1", 0x12), ("2", 0x13), ("3", 0x14), ("4", 0x15),
    ("5", 0x17), ("6", 0x16), ("7", 0x1A), ("8", 0x1C), ("9", 0x19),
    ("Up", 0x7E), ("Down", 0x7D), ("Left", 0x7B), ("Right", 0x7C),
    ("Space", 0x31), ("Enter", 0x24), ("Tab", 0x30), ("Escape", 0x35),
    ("Delete", 0x33),
    ("Left Shift", 0x38), ("Right Shift", 0x3C),
    ("Left Ctrl", 0x3B), ("Right Ctrl", 0x3E),
    ("Left Option", 0x3A), ("Right Option", 0x3D),
    ("Left Cmd", 0x37), ("Right Cmd", 0x36),
    (",", 0x2B), (".", 0x2F), ("/", 0x2C), (";", 0x29), ("'", 0x27),
];

/// Display name for a key code, or a hex fallback.
pub fn key_name(code: u16) -> String {
    KEYS.iter()
        .find(|(_, c)| *c == code)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("0x{code:02x}"))
}

/// Reverse lookup: key code for a display name (case-insensitive), if known.
pub fn code_for_name(name: &str) -> Option<u16> {
    KEYS.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, c)| *c)
}

/// Serde glue so `bindings` stores readable key NAMES in JSON instead of codes.
mod binding_names {
    use super::{code_for_name, key_name, ControlId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<ControlId, u16>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let named: BTreeMap<ControlId, String> =
            map.iter().map(|(k, v)| (*k, key_name(*v))).collect();
        named.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<ControlId, u16>, D::Error> {
        let named = BTreeMap::<ControlId, String>::deserialize(d)?;
        // Drop any binding whose key name we don't recognize.
        Ok(named
            .into_iter()
            .filter_map(|(k, n)| code_for_name(&n).map(|c| (k, c)))
            .collect())
    }
}

/// A named set of control→key bindings plus the analog thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub deadzone: f32,
    pub walk_threshold: f32,
    pub trigger_threshold: u8,
    /// Bound controls only; absent controls are unbound. Serialized as readable
    /// key names (see [`binding_names`]).
    #[serde(with = "binding_names")]
    pub bindings: BTreeMap<ControlId, u16>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: "Untitled".to_string(),
            deadzone: crate::DEFAULT_DEADZONE,
            walk_threshold: 0.5,
            trigger_threshold: 64,
            bindings: BTreeMap::new(),
        }
    }
}

impl Profile {
    /// A general-purpose starting template (`default.json`).
    pub fn default_mapping() -> Self {
        use ControlId::*;
        let b: &[(ControlId, u16)] = &[
            (A, 0x31),          // Space
            (B, 0x3B),          // Left Ctrl
            (X, 0x06),          // Z
            (Y, 0x07),          // X
            (Lb, 0x0C),         // Q
            (Rb, 0x0E),         // E
            (Lt, 0x0F),         // R
            (Rt, 0x11),         // T
            (Back, 0x3C),       // Right Shift
            (Start, 0x24),      // Enter
            (DpadUp, 0x7E), (DpadDown, 0x7D), (DpadLeft, 0x7B), (DpadRight, 0x7C),
            (LStickUp, 0x7E), (LStickDown, 0x7D), (LStickLeft, 0x7B), (LStickRight, 0x7C),
        ];
        Profile {
            name: "default".to_string(),
            bindings: b.iter().copied().collect(),
            ..Default::default()
        }
    }

    /// The Metal Slug / retrogames.cc arcade layout (`retrogames.cc.json`).
    pub fn retrogames_cc() -> Self {
        use ControlId::*;
        let b: &[(ControlId, u16)] = &[
            (A, 0x06),          // z
            (B, 0x07),          // x
            (X, 0x00),          // a
            (Y, 0x01),          // s
            (Lb, 0x0C),         // q
            (Rb, 0x0E),         // e
            (Lt, 0x0F),         // r
            (Rt, 0x11),         // t
            (Back, 0x3C),       // right shift (insert coin)
            (Start, 0x24),      // enter
            (DpadUp, 0x7E), (DpadDown, 0x7D), (DpadLeft, 0x7B), (DpadRight, 0x7C),
            (LStickUp, 0x7E), (LStickDown, 0x7D), (LStickLeft, 0x7B), (LStickRight, 0x7C),
        ];
        Profile {
            name: "retrogames.cc".to_string(),
            bindings: b.iter().copied().collect(),
            ..Default::default()
        }
    }
}

/// Directory where profiles are stored: a `mappings/` folder in the working
/// directory (so they live with the project and can be committed/shared).
pub fn profiles_dir() -> PathBuf {
    PathBuf::from("mappings")
}

/// Create the `mappings/` folder and seed the bundled profiles if missing.
pub fn ensure_defaults() {
    let dir = profiles_dir();
    let _ = std::fs::create_dir_all(&dir);
    for p in [Profile::default_mapping(), Profile::retrogames_cc()] {
        let path = dir.join(format!("{}.json", sanitize(&p.name)));
        if !path.exists() {
            let _ = save_profile(&p);
        }
    }
}

/// Export a profile to an arbitrary path (for sharing with other people).
pub fn export_profile(p: &Profile, path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(p)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Import a profile from an arbitrary path (a file someone shared).
pub fn import_profile(path: &Path) -> std::io::Result<Profile> {
    let json = std::fs::read_to_string(path)?;
    serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Save a profile as `<name>.json` in the profiles directory.
pub fn save_profile(p: &Profile) -> std::io::Result<PathBuf> {
    let dir = profiles_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", sanitize(&p.name)));
    let json = serde_json::to_string_pretty(p)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Load a profile by file stem (name).
pub fn load_profile(name: &str) -> std::io::Result<Profile> {
    let path = profiles_dir().join(format!("{}.json", sanitize(name)));
    let json = std::fs::read_to_string(path)?;
    serde_json::from_str(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// List available profile names (file stems) in the profiles directory.
pub fn list_profiles() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(profiles_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Compute the set of key codes that should be held for the given state+profile.
pub fn active_keys(profile: &Profile, s: &GamepadState) -> HashSet<u16> {
    let mut keys = HashSet::new();
    for (&id, &code) in &profile.bindings {
        if id.is_active(s, profile.deadzone, profile.walk_threshold, profile.trigger_threshold) {
            keys.insert(code);
        }
    }
    keys
}

/// Posts key CGEvents and tracks which are held, so it can diff and release.
pub struct Injector {
    source: CGEventSource,
    keys_down: HashSet<u16>,
}

impl Injector {
    pub fn new(source: CGEventSource) -> Self {
        Injector { source, keys_down: HashSet::new() }
    }

    fn key(&self, code: u16, down: bool) {
        if let Ok(ev) = CGEvent::new_keyboard_event(self.source.clone(), code, down) {
            ev.post(CGEventTapLocation::HID);
        }
    }

    /// Press newly-wanted keys and release ones no longer wanted.
    pub fn apply(&mut self, desired: HashSet<u16>) {
        for &c in desired.difference(&self.keys_down.clone()) {
            self.key(c, true);
        }
        for &c in self.keys_down.clone().difference(&desired) {
            self.key(c, false);
        }
        self.keys_down = desired;
    }

    /// Release every held key.
    pub fn release_all(&mut self) {
        for &c in &self.keys_down.clone() {
            self.key(c, false);
        }
        self.keys_down.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_human_readable_and_round_trips() {
        let p = Profile::retrogames_cc();
        let json = serde_json::to_string_pretty(&p).unwrap();
        // Readable control + key names, not raw codes.
        assert!(json.contains("\"BACK\": \"Right Shift\""), "json:\n{json}");
        assert!(json.contains("\"A\": \"Z\""));
        // Round-trips back to identical bindings.
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bindings, p.bindings);
        assert_eq!(back.name, "retrogames.cc");
    }

    #[test]
    fn shipped_mapping_files_parse() {
        for name in ["default", "retrogames.cc"] {
            let path = profiles_dir().join(format!("{name}.json"));
            let p = import_profile(&path)
                .unwrap_or_else(|e| panic!("failed to load {}: {e}", path.display()));
            assert_eq!(p.name, name);
            // Back/Select must be Right Shift (the coin button) in both.
            assert_eq!(p.bindings.get(&ControlId::Back), Some(&0x3C));
        }
    }

    #[test]
    fn unknown_key_names_are_dropped_not_fatal() {
        let json = r#"{"name":"x","deadzone":0.08,"walk_threshold":0.5,
            "trigger_threshold":64,"bindings":{"A":"Z","B":"Nonsense"}}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.bindings.get(&ControlId::A), Some(&0x06));
        assert_eq!(p.bindings.get(&ControlId::B), None); // dropped
    }
}
