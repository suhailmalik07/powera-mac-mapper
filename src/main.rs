//! PowerA controller input reader (userspace, macOS).
//!
//! Opens the controller's Xbox 360 / XInput vendor interface over libusb,
//! reads 20-byte input reports from its interrupt IN endpoint, and prints the
//! decoded state live. No kernel driver or HID stack involved.
//!
//!     cargo run --bin powera

use powera_driver::{parse_report, GamepadState, DEFAULT_DEADZONE};
use rusb::{Context, Direction, TransferType, UsbContext};
use std::time::Duration;

const POWERA_VID: u16 = 0x20D6;
// Known PowerA product ids: 0x4022 = wired pad, 0x4024 = 2.4GHz adapter.
// We don't filter on PID — we match any PowerA device exposing an XInput
// interface, so the wired pad, the dongle, and future variants all work.

/// Vendor interface signature for the Xbox 360 / XInput control interface.
const XINPUT_CLASS: u8 = 0xFF;
const XINPUT_SUBCLASS: u8 = 0x5D;
const XINPUT_PROTOCOL: u8 = 0x01;

fn main() {
    if let Err(e) = run() {
        eprintln!("\nerror: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let deadzone = parse_deadzone_arg().unwrap_or(DEFAULT_DEADZONE);

    let ctx = Context::new()?;

    // Find a PowerA device that exposes an XInput interface, and locate that
    // interface + its interrupt IN endpoint from the descriptors (rather than
    // hard-coding PID / iface 0 / ep 0x81). Works for the wired pad, the
    // 2.4GHz adapter, etc.
    let mut found = None;
    for device in ctx.devices()?.iter() {
        let is_powera = device
            .device_descriptor()
            .map(|desc| desc.vendor_id() == POWERA_VID)
            .unwrap_or(false);
        if !is_powera {
            continue;
        }
        if let Some((iface_num, ep_in)) = find_xinput_endpoint(&device)? {
            let pid = device.device_descriptor().map(|d| d.product_id()).unwrap_or(0);
            found = Some((device, pid, iface_num, ep_in));
            break;
        }
    }
    let (device, pid, iface_num, ep_in) = found.ok_or(
        "No PowerA XInput device found. Plug in the wired cable (switch on USB) \
         or the 2.4GHz adapter (switch on 2.4 RF), and power the controller on.",
    )?;

    let handle = device.open().map_err(|e| {
        format!("could not open device ({e}). On macOS, no extra entitlement is normally needed for a vendor interface.")
    })?;

    // Linux needs the kernel driver detached; macOS has no XInput driver to
    // detach, so this is a harmless no-op there.
    let _ = handle.set_auto_detach_kernel_driver(true);

    handle.claim_interface(iface_num).map_err(|e| {
        format!("could not claim interface {iface_num} ({e}). Another process may hold it.")
    })?;

    println!(
        "Reading PowerA {POWERA_VID:04x}:{pid:04x}  (XInput iface {iface_num}, ep {ep_in:#04x})\n\
         Deadzone: {:.0}%.  Press buttons / move sticks. Ctrl-C to quit.\n",
        deadzone * 100.0
    );

    let mut buf = [0u8; 32];
    let mut last: Option<GamepadState> = None;

    loop {
        match handle.read_interrupt(ep_in, &mut buf, Duration::from_millis(1000)) {
            Ok(n) => {
                if let Some(state) = parse_report(&buf[..n]) {
                    // Only redraw when something actually changed.
                    if last != Some(state) {
                        render(&state, deadzone);
                        last = Some(state);
                    }
                }
            }
            Err(rusb::Error::Timeout) => continue, // no input this second; keep waiting
            Err(rusb::Error::NoDevice) => return Err("controller disconnected".into()),
            Err(e) => return Err(format!("interrupt read failed: {e}").into()),
        }
    }
}

/// Locate the XInput interface number and its interrupt IN endpoint address.
fn find_xinput_endpoint<T: UsbContext>(
    device: &rusb::Device<T>,
) -> Result<Option<(u8, u8)>, rusb::Error> {
    let config = device.active_config_descriptor()?;
    for iface in config.interfaces() {
        for alt in iface.descriptors() {
            let is_xinput = alt.class_code() == XINPUT_CLASS
                && alt.sub_class_code() == XINPUT_SUBCLASS
                && alt.protocol_code() == XINPUT_PROTOCOL;
            if !is_xinput {
                continue;
            }
            for ep in alt.endpoint_descriptors() {
                if ep.direction() == Direction::In
                    && ep.transfer_type() == TransferType::Interrupt
                {
                    return Ok(Some((alt.interface_number(), ep.address())));
                }
            }
        }
    }
    Ok(None)
}

/// Parse an optional `--deadzone <fraction>` argument (e.g. `--deadzone 0.12`).
fn parse_deadzone_arg() -> Option<f32> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == "--deadzone" {
            return args.next()?.parse::<f32>().ok();
        }
        if let Some(v) = a.strip_prefix("--deadzone=") {
            return v.parse::<f32>().ok();
        }
    }
    None
}

/// Render one line of decoded state, overwriting in place.
fn render(s: &GamepadState, deadzone: f32) {
    // Sticks: normalized to -1.0..=1.0 with a scaled radial deadzone applied.
    let (lx, ly) = s.left_stick_norm(deadzone);
    let (rx, ry) = s.right_stick_norm(deadzone);

    let buttons = s.pressed_buttons();
    let buttons = if buttons.is_empty() {
        "-".to_string()
    } else {
        buttons.join(" ")
    };

    // \r + clear-to-end-of-line keeps it on a single updating row.
    print!(
        "\r\x1b[2K\
         L({:+.2},{:+.2}) R({:+.2},{:+.2})  LT {:>3} RT {:>3}  | {}",
        lx,
        ly,
        rx,
        ry,
        s.left_trigger,
        s.right_trigger,
        buttons,
    );
    use std::io::Write;
    std::io::stdout().flush().ok();
}
