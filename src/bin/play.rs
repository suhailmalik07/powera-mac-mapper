//! Keyboard mapper for the PowerA controller — Metal Slug 5 arcade layout.
//!
//! Reads the controller (via the shared XInput reader) and synthesizes macOS
//! keyboard events with CGEvent, matching the emulator's Player-1 bindings:
//!
//!   Left stick + D-pad -> arrow keys (move)
//!   A -> z   B -> x   X -> a   Y -> s          (BUTTON_1..4)
//!   LB -> q  RB -> e                            (L / R top shoulders)
//!   LT -> r  RT -> t                            (L2 / R2 bottom shoulders)
//!   Back/Select -> right shift (insert coin)    Start -> enter
//!
//! Requires Accessibility permission (System Settings > Privacy & Security >
//! Accessibility) for the terminal/app running it — otherwise macOS silently
//! drops the synthetic events. No SIP change or entitlement needed.
//!
//!     cargo run --bin play
//!     cargo run --bin play -- --deadzone 0.10

use core_graphics::event::{CGEvent, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use powera_driver::{controller::Controller, GamepadState, DEFAULT_DEADZONE};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// macOS virtual key codes (kVK_*). A reference set; not all are used.
#[allow(dead_code)]
mod kc {
    pub const A: u16 = 0x00;
    pub const S: u16 = 0x01;
    pub const D: u16 = 0x02;
    pub const Z: u16 = 0x06;
    pub const X: u16 = 0x07;
    pub const Q: u16 = 0x0C;
    pub const W: u16 = 0x0D;
    pub const E: u16 = 0x0E;
    pub const R: u16 = 0x0F;
    pub const T: u16 = 0x11;
    pub const RETURN: u16 = 0x24;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const ESC: u16 = 0x35;
    pub const RSHIFT: u16 = 0x3C;
    pub const LEFT: u16 = 0x7B;
    pub const RIGHT: u16 = 0x7C;
    pub const DOWN: u16 = 0x7D;
    pub const UP: u16 = 0x7E;
}

// Check whether this process is allowed to post synthetic events.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\nerror: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let deadzone = parse_arg("--deadzone").unwrap_or(DEFAULT_DEADZONE);
    let walk = 0.5_f32; // stick fraction before a direction engages
    let trig = 64_u8; // trigger value (0-255) before it counts as pressed

    if unsafe { !AXIsProcessTrusted() } {
        eprintln!(
            "WARNING: this process is NOT trusted for Accessibility, so macOS will\n\
             silently ignore the synthetic key events.\n\
             Grant it in System Settings > Privacy & Security > Accessibility\n\
             (add your terminal app), then re-run.\n"
        );
    }

    let mut controller = Controller::open()?;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "failed to create CGEventSource")?;
    let mut inj = Injector::new(source);

    // Clean shutdown: release everything still held when Ctrl-C is pressed.
    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::SeqCst)).map_err(|e| e.to_string())?;
    }

    println!(
        "Mapping PowerA -> keyboard (Metal Slug layout).  deadzone={:.0}%\n\
         Stick/D-pad=arrows, A=z B=x X=a Y=s, LB=q RB=e, LT=r RT=t,\n\
         Select=RightShift(coin), Start=Enter.  Ctrl-C to stop.\n",
        deadzone * 100.0
    );

    let mut state = GamepadState::default();
    while running.load(Ordering::SeqCst) {
        if let Some(s) = controller.read(8)? {
            state = s;
        }
        inj.apply_keys(desired_keys(&state, deadzone, walk, trig));
    }

    inj.release_all();
    println!("\nReleased all keys. Bye.");
    Ok(())
}

/// Build the set of key codes that should be held down for a given state.
fn desired_keys(s: &GamepadState, deadzone: f32, walk: f32, trig: u8) -> HashSet<u16> {
    let (lx, ly) = s.left_stick_norm(deadzone);
    let mut k = HashSet::new();

    // Movement: left stick OR D-pad -> arrow keys.
    if ly > walk || s.dpad_up {
        k.insert(kc::UP);
    }
    if ly < -walk || s.dpad_down {
        k.insert(kc::DOWN);
    }
    if lx < -walk || s.dpad_left {
        k.insert(kc::LEFT);
    }
    if lx > walk || s.dpad_right {
        k.insert(kc::RIGHT);
    }

    // Face buttons -> BUTTON_1..4.
    if s.a {
        k.insert(kc::Z);
    }
    if s.b {
        k.insert(kc::X);
    }
    if s.x {
        k.insert(kc::A);
    }
    if s.y {
        k.insert(kc::S);
    }

    // Shoulders and triggers.
    if s.lb {
        k.insert(kc::Q);
    }
    if s.rb {
        k.insert(kc::E);
    }
    if s.left_trigger > trig {
        k.insert(kc::R);
    }
    if s.right_trigger > trig {
        k.insert(kc::T);
    }

    // Coin / start.
    if s.back {
        k.insert(kc::RSHIFT);
    }
    if s.start {
        k.insert(kc::RETURN);
    }

    k
}

/// Posts key CGEvents and tracks what is held so it can diff/release.
struct Injector {
    source: CGEventSource,
    keys_down: HashSet<u16>,
}

impl Injector {
    fn new(source: CGEventSource) -> Self {
        Injector {
            source,
            keys_down: HashSet::new(),
        }
    }

    fn key(&self, code: u16, down: bool) {
        if let Ok(ev) = CGEvent::new_keyboard_event(self.source.clone(), code, down) {
            ev.post(CGEventTapLocation::HID);
        }
    }

    /// Press newly-wanted keys and release ones no longer wanted.
    fn apply_keys(&mut self, desired: HashSet<u16>) {
        for &c in desired.difference(&self.keys_down.clone()) {
            self.key(c, true);
        }
        for &c in self.keys_down.clone().difference(&desired) {
            self.key(c, false);
        }
        self.keys_down = desired;
    }

    /// Release every held key (called on exit).
    fn release_all(&mut self) {
        for &c in &self.keys_down.clone() {
            self.key(c, false);
        }
        self.keys_down.clear();
    }
}

/// Parse a numeric `--flag value` or `--flag=value` argument.
fn parse_arg(flag: &str) -> Option<f32> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == flag {
            return args.next()?.parse().ok();
        }
        if let Some(v) = a.strip_prefix(&format!("{flag}=")) {
            return v.parse().ok();
        }
    }
    None
}
