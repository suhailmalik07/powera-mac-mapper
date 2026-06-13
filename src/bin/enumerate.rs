//! Device fingerprinting tool.
//!
//! Lists every HID device macOS exposes, flagging anything that looks like a
//! game controller (and PowerA's USB vendor id, 0x20D6, in particular).
//!
//! Run with the controller plugged in:
//!     cargo run --bin enumerate

use hidapi::HidApi;

/// PowerA's USB vendor id.
const POWERA_VID: u16 = 0x20D6;

fn main() {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => {
            eprintln!("failed to init HID API: {e}");
            std::process::exit(1);
        }
    };

    let mut devices: Vec<_> = api.device_list().collect();
    // Stable, readable ordering: by vendor then product.
    devices.sort_by_key(|d| (d.vendor_id(), d.product_id(), d.interface_number()));

    println!("Found {} HID interface(s):\n", devices.len());

    let mut powera_seen = false;
    let mut gamepad_seen = false;

    for d in devices {
        let vid = d.vendor_id();
        let pid = d.product_id();

        // HID usage page 0x01 (Generic Desktop) + usage 0x04/0x05 = joystick/gamepad.
        let is_gamepad = d.usage_page() == 0x01 && matches!(d.usage(), 0x04 | 0x05);
        let is_powera = vid == POWERA_VID;

        if is_powera {
            powera_seen = true;
        }
        if is_gamepad {
            gamepad_seen = true;
        }

        let tag = match (is_powera, is_gamepad) {
            (true, _) => "  <-- PowerA",
            (_, true) => "  <-- gamepad",
            _ => "",
        };

        println!(
            "  {:04x}:{:04x}  iface={:<3} usage={:#04x}/{:#04x}  {} / {}{}",
            vid,
            pid,
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.manufacturer_string().unwrap_or("?"),
            d.product_string().unwrap_or("?"),
            tag,
        );
    }

    println!();
    if powera_seen {
        println!("PowerA device present as HID. Note the VID:PID above for the reader.");
    } else if gamepad_seen {
        println!("A gamepad is present but not under PowerA's vendor id. Note its VID:PID.");
    } else {
        println!(
            "No PowerA / gamepad HID interface found.\n\
             If the controller is plugged in, it is likely an Xbox-protocol (GIP) pad,\n\
             which is NOT a HID device. Confirm with:  ioreg -p IOUSB -l -w 0 | grep -i powera"
        );
    }
}
