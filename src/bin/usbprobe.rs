//! Raw USB descriptor probe (libusb via rusb).
//!
//! Dumps the PowerA controller's configuration: every interface, its class,
//! and every endpoint (address + direction + type). We need the GIP data
//! interface number and its interrupt IN/OUT endpoint addresses before we can
//! send the init handshake or read input.
//!
//! Build/run (requires the `gip` feature, which pulls in rusb):
//!     cargo run --features gip --bin usbprobe

use rusb::{Direction, TransferType, UsbContext};

const POWERA_VID: u16 = 0x20D6;
const POWERA_PID: u16 = 0x4022;

fn main() {
    let ctx = rusb::Context::new().expect("init libusb context");

    let devices = ctx.devices().expect("list devices");
    let mut found = false;

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };
        // Match any PowerA device (wired pad 0x4022, 2.4GHz adapter 0x4024, ...).
        if desc.vendor_id() != POWERA_VID {
            continue;
        }
        let _ = POWERA_PID; // kept for reference; we match on vendor now
        found = true;

        println!(
            "PowerA {:04x}:{:04x}  bus={} addr={}  {} config(s)",
            desc.vendor_id(),
            desc.product_id(),
            device.bus_number(),
            device.address(),
            desc.num_configurations(),
        );

        // Try to read the string descriptors via an open handle (best effort).
        if let Ok(handle) = device.open() {
            if let Ok(langs) = handle.read_languages(std::time::Duration::from_millis(200)) {
                if let Some(lang) = langs.first() {
                    let t = std::time::Duration::from_millis(200);
                    let prod = handle.read_product_string(*lang, &desc, t).unwrap_or_default();
                    let serial =
                        handle.read_serial_number_string(*lang, &desc, t).unwrap_or_default();
                    println!("  product={prod:?}  serial={serial:?}");
                }
            }
        } else {
            println!("  (could not open handle to read strings — may need privileges)");
        }

        for cfg_idx in 0..desc.num_configurations() {
            let config = match device.config_descriptor(cfg_idx) {
                Ok(c) => c,
                Err(e) => {
                    println!("  config {cfg_idx}: <error {e}>");
                    continue;
                }
            };
            println!(
                "  config #{} value={} interfaces={}",
                cfg_idx,
                config.number(),
                config.num_interfaces()
            );

            for iface in config.interfaces() {
                for alt in iface.descriptors() {
                    println!(
                        "    iface {} alt {}: class={:#04x} subclass={:#04x} proto={:#04x} endpoints={}",
                        alt.interface_number(),
                        alt.setting_number(),
                        alt.class_code(),
                        alt.sub_class_code(),
                        alt.protocol_code(),
                        alt.num_endpoints(),
                    );
                    for ep in alt.endpoint_descriptors() {
                        let dir = match ep.direction() {
                            Direction::In => "IN ",
                            Direction::Out => "OUT",
                        };
                        let ty = match ep.transfer_type() {
                            TransferType::Control => "control",
                            TransferType::Isochronous => "isochronous",
                            TransferType::Bulk => "bulk",
                            TransferType::Interrupt => "interrupt",
                        };
                        println!(
                            "      ep {:#04x} {dir} {ty:<11} max_packet={} interval={}",
                            ep.address(),
                            ep.max_packet_size(),
                            ep.interval(),
                        );
                    }
                }
            }
        }
    }

    if !found {
        eprintln!(
            "PowerA {POWERA_VID:04x}:{POWERA_PID:04x} not found on the USB bus.\n\
             Make sure the connection slider is set to USB and the cable is in."
        );
        std::process::exit(1);
    }
}
