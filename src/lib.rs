//! Decoding for the Xbox 360 / XInput wired input protocol.
//!
//! The PowerA Battle Dragon (and most third-party "for PC" pads) emulate an
//! Xbox 360 controller: a vendor-specific USB interface (class 0xFF, subclass
//! 0x5D, protocol 0x01) that streams fixed 20-byte input reports on an
//! interrupt IN endpoint, with no initialization handshake required.
//!
//! Report layout (little-endian), verified against the Linux `xpad` driver and
//! the partsnotincluded.com breakdown:
//!
//! ```text
//! byte 0      message type = 0x00
//! byte 1      length       = 0x14 (20)
//! byte 2      dpad: up(0) down(1) left(2) right(3), start(4) back(5) L3(6) R3(7)
//! byte 3      LB(0) RB(1) guide(2) -, A(4) B(5) X(6) Y(7)
//! byte 4      left trigger  (0..=255)
//! byte 5      right trigger (0..=255)
//! byte 6..8   left stick  X (i16)
//! byte 8..10  left stick  Y (i16)
//! byte 10..12 right stick X (i16)
//! byte 12..14 right stick Y (i16)
//! byte 14..20 unused
//! ```

/// Length of a full Xbox 360 wired input report.
pub const REPORT_LEN: usize = 20;

/// Decoded controller state from a single input report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GamepadState {
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub start: bool,
    pub back: bool,
    /// Left stick click (L3).
    pub l3: bool,
    /// Right stick click (R3).
    pub r3: bool,
    /// Left bumper.
    pub lb: bool,
    /// Right bumper.
    pub rb: bool,
    /// Guide / Xbox button.
    pub guide: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    /// Left trigger, 0..=255.
    pub left_trigger: u8,
    /// Right trigger, 0..=255.
    pub right_trigger: u8,
    /// Left stick (x, y), each -32768..=32767. Y is up-positive.
    pub left_stick: (i16, i16),
    /// Right stick (x, y), each -32768..=32767. Y is up-positive.
    pub right_stick: (i16, i16),
}

/// Parse a raw interrupt-IN buffer into [`GamepadState`].
///
/// Returns `None` if the buffer is too short or is not an input report
/// (header bytes other than `0x00 0x14`) — the 360 endpoint can also carry
/// non-input messages (e.g. LED/announce) that we ignore here.
pub fn parse_report(buf: &[u8]) -> Option<GamepadState> {
    // Need through byte 13 for the right stick; reject anything shorter.
    if buf.len() < 14 {
        return None;
    }
    // 0x00 = input message type, 0x14 = 20-byte length. Anything else isn't input.
    if buf[0] != 0x00 || buf[1] != 0x14 {
        return None;
    }

    let buttons1 = buf[2];
    let buttons2 = buf[3];
    let bit = |byte: u8, n: u8| byte & (1 << n) != 0;
    let i16le = |i: usize| i16::from_le_bytes([buf[i], buf[i + 1]]);

    Some(GamepadState {
        dpad_up: bit(buttons1, 0),
        dpad_down: bit(buttons1, 1),
        dpad_left: bit(buttons1, 2),
        dpad_right: bit(buttons1, 3),
        start: bit(buttons1, 4),
        back: bit(buttons1, 5),
        l3: bit(buttons1, 6),
        r3: bit(buttons1, 7),
        lb: bit(buttons2, 0),
        rb: bit(buttons2, 1),
        guide: bit(buttons2, 2),
        a: bit(buttons2, 4),
        b: bit(buttons2, 5),
        x: bit(buttons2, 6),
        y: bit(buttons2, 7),
        left_trigger: buf[4],
        right_trigger: buf[5],
        left_stick: (i16le(6), i16le(8)),
        right_stick: (i16le(10), i16le(12)),
    })
}

/// Default radial deadzone, as a fraction of full stick deflection (8%).
/// Large enough to absorb this controller's small right-stick center drift.
pub const DEFAULT_DEADZONE: f32 = 0.08;

/// Apply a *scaled radial* deadzone to a raw stick and normalize to -1.0..=1.0.
///
/// The stick is treated as a 2D vector. If its magnitude is within `deadzone`
/// (0.0..1.0), the output is (0, 0) — this kills both jitter and a small
/// off-center rest position. Outside the deadzone, the remaining magnitude is
/// rescaled so output ramps from 0 at the edge of the dead region up to 1 at
/// full deflection (no sudden jump), preserving the stick's direction.
pub fn deadzone_stick(x: i16, y: i16, deadzone: f32) -> (f32, f32) {
    // i16 spans -32768..=32767; divide by 32767 and clamp so -32768 -> -1.0.
    let nx = (x as f32 / 32767.0).clamp(-1.0, 1.0);
    let ny = (y as f32 / 32767.0).clamp(-1.0, 1.0);
    let mag = (nx * nx + ny * ny).sqrt();

    let deadzone = deadzone.clamp(0.0, 0.99);
    if mag <= deadzone || mag == 0.0 {
        return (0.0, 0.0);
    }
    // Rescale magnitude [deadzone, 1] -> [0, 1], keep direction (nx/mag, ny/mag).
    let scaled = ((mag - deadzone) / (1.0 - deadzone)).min(1.0);
    let factor = scaled / mag;
    (nx * factor, ny * factor)
}

impl GamepadState {
    /// Left stick normalized to -1.0..=1.0 with a scaled radial deadzone applied.
    pub fn left_stick_norm(&self, deadzone: f32) -> (f32, f32) {
        deadzone_stick(self.left_stick.0, self.left_stick.1, deadzone)
    }

    /// Right stick normalized to -1.0..=1.0 with a scaled radial deadzone applied.
    pub fn right_stick_norm(&self, deadzone: f32) -> (f32, f32) {
        deadzone_stick(self.right_stick.0, self.right_stick.1, deadzone)
    }

    /// Names of every digital button currently pressed, in a stable order.
    pub fn pressed_buttons(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut push = |on: bool, name: &'static str| {
            if on {
                v.push(name);
            }
        };
        push(self.a, "A");
        push(self.b, "B");
        push(self.x, "X");
        push(self.y, "Y");
        push(self.lb, "LB");
        push(self.rb, "RB");
        push(self.l3, "L3");
        push(self.r3, "R3");
        push(self.start, "Start");
        push(self.back, "Back");
        push(self.guide, "Guide");
        push(self.dpad_up, "Up");
        push(self.dpad_down, "Down");
        push(self.dpad_left, "Left");
        push(self.dpad_right, "Right");
        v
    }
}

pub mod mapping;

/// Opening and reading the controller's XInput interface over USB (libusb).
///
/// Works for any PowerA device exposing the XInput interface — the wired pad
/// or the 2.4GHz adapter. Used by the `powera` and `play` binaries.
pub mod controller {
    use super::{parse_report, GamepadState};
    use rusb::{Context, Direction, TransferType, UsbContext};
    use std::time::Duration;

    const POWERA_VID: u16 = 0x20D6;

    /// An opened, claimed XInput controller ready to read input reports from.
    pub struct Controller {
        handle: rusb::DeviceHandle<Context>,
        ep_in: u8,
        buf: [u8; 32],
    }

    impl Controller {
        /// Find the first PowerA device exposing an XInput interface, open and
        /// claim it. Returns a descriptive error if none is connected.
        pub fn open() -> Result<Controller, String> {
            let ctx = Context::new().map_err(|e| e.to_string())?;
            for device in ctx.devices().map_err(|e| e.to_string())?.iter() {
                let is_powera = device
                    .device_descriptor()
                    .map(|d| d.vendor_id() == POWERA_VID)
                    .unwrap_or(false);
                if !is_powera {
                    continue;
                }
                if let Some((iface, ep_in)) = find_xinput(&device).map_err(|e| e.to_string())? {
                    let handle = device.open().map_err(|e| e.to_string())?;
                    let _ = handle.set_auto_detach_kernel_driver(true);
                    handle
                        .claim_interface(iface)
                        .map_err(|e| format!("claim interface {iface}: {e}"))?;
                    return Ok(Controller { handle, ep_in, buf: [0u8; 32] });
                }
            }
            Err("No PowerA XInput device found. Plug in the 2.4GHz dongle (switch on \
                 2.4 RF) or the wired pad and power the controller on."
                .into())
        }

        /// Read one input report. `Ok(None)` means the read timed out with no
        /// new data; `Ok(Some(_))` is decoded state; `Err` is a fatal error.
        pub fn read(&mut self, timeout_ms: u64) -> Result<Option<GamepadState>, String> {
            match self
                .handle
                .read_interrupt(self.ep_in, &mut self.buf, Duration::from_millis(timeout_ms))
            {
                Ok(n) => Ok(parse_report(&self.buf[..n])),
                Err(rusb::Error::Timeout) => Ok(None),
                Err(rusb::Error::NoDevice) => Err("controller disconnected".into()),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    fn find_xinput<T: UsbContext>(
        device: &rusb::Device<T>,
    ) -> Result<Option<(u8, u8)>, rusb::Error> {
        let cfg = device.active_config_descriptor()?;
        for iface in cfg.interfaces() {
            for alt in iface.descriptors() {
                if alt.class_code() == 0xFF
                    && alt.sub_class_code() == 0x5D
                    && alt.protocol_code() == 0x01
                {
                    for ep in alt.endpoint_descriptors() {
                        if ep.direction() == Direction::In
                            && ep.transfer_type() == TransferType::Interrupt
                        {
                            return Ok(Some((alt.interface_number(), ep.address())));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A neutral report: header + all zeros. Sticks centered, nothing pressed.
    #[test]
    fn neutral_report() {
        let mut buf = [0u8; REPORT_LEN];
        buf[0] = 0x00;
        buf[1] = 0x14;
        let s = parse_report(&buf).expect("valid header");
        assert_eq!(s, GamepadState::default());
        assert!(s.pressed_buttons().is_empty());
    }

    #[test]
    fn rejects_short_and_wrong_header() {
        assert!(parse_report(&[0x00, 0x14, 0x00]).is_none()); // too short
        let mut buf = [0u8; REPORT_LEN];
        buf[1] = 0x14; // type byte wrong (0x01)
        buf[0] = 0x01;
        assert!(parse_report(&buf).is_none());
    }

    #[test]
    fn decodes_buttons() {
        let mut buf = [0u8; REPORT_LEN];
        buf[0] = 0x00;
        buf[1] = 0x14;
        buf[2] = 0b0001_0001; // dpad up + start
        buf[3] = 0b0001_0001; // LB + A
        let s = parse_report(&buf).unwrap();
        assert!(s.dpad_up && s.start && s.lb && s.a);
        assert!(!s.dpad_down && !s.b && !s.guide);
        assert_eq!(s.pressed_buttons(), vec!["A", "LB", "Start", "Up"]);
    }

    #[test]
    fn decodes_triggers_and_sticks() {
        let mut buf = [0u8; REPORT_LEN];
        buf[0] = 0x00;
        buf[1] = 0x14;
        buf[4] = 255; // LT fully pressed
        buf[5] = 128; // RT half
        buf[6..8].copy_from_slice(&(-32768i16).to_le_bytes()); // LX full left
        buf[8..10].copy_from_slice(&32767i16.to_le_bytes()); // LY full up
        buf[10..12].copy_from_slice(&1234i16.to_le_bytes()); // RX
        buf[12..14].copy_from_slice(&(-5678i16).to_le_bytes()); // RY
        let s = parse_report(&buf).unwrap();
        assert_eq!(s.left_trigger, 255);
        assert_eq!(s.right_trigger, 128);
        assert_eq!(s.left_stick, (-32768, 32767));
        assert_eq!(s.right_stick, (1234, -5678));
    }

    #[test]
    fn deadzone_kills_center_and_small_drift() {
        // Dead center.
        assert_eq!(deadzone_stick(0, 0, 0.08), (0.0, 0.0));
        // Small off-center drift (~3% on one axis) is inside an 8% deadzone.
        let (x, y) = deadzone_stick(1000, -600, 0.08);
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn deadzone_passes_full_deflection() {
        // Full right: should reach ~1.0 on x, ~0 on y.
        let (x, y) = deadzone_stick(32767, 0, 0.08);
        assert!((x - 1.0).abs() < 1e-4, "x={x}");
        assert!(y.abs() < 1e-6, "y={y}");
    }

    #[test]
    fn deadzone_rescales_and_preserves_direction() {
        // Just outside the deadzone the output starts near 0 (no jump)...
        let (x, _) = deadzone_stick((0.10 * 32767.0) as i16, 0, 0.08);
        assert!(x > 0.0 && x < 0.05, "expected small ramp, got {x}");
        // ...and a 45° push stays on the diagonal (x == y).
        let v = (0.7 * 32767.0) as i16;
        let (dx, dy) = deadzone_stick(v, v, 0.08);
        assert!((dx - dy).abs() < 1e-4, "dx={dx} dy={dy}");
    }
}
