//! Diagnostic: try to "wake" a clone XInput pad into streaming.
//!
//! Some third-party Xbox 360 clones stay silent until the host sends the
//! commands a real console/PC sends on connect (LED ring + rumble-off). This
//! sends a few candidate output packets to the OUT endpoint, then watches the
//! IN endpoint for input reports.
//!
//!     cargo run --bin wake

use rusb::{Context, Direction, TransferType, UsbContext};
use std::time::Duration;

const POWERA_VID: u16 = 0x20D6;
const POWERA_PID: u16 = 0x4022;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new()?;
    let device = ctx
        .devices()?
        .iter()
        .find(|d| {
            d.device_descriptor()
                .map(|x| x.vendor_id() == POWERA_VID && x.product_id() == POWERA_PID)
                .unwrap_or(false)
        })
        .ok_or("PowerA not found")?;

    // Discover the XInput interface + both interrupt endpoints.
    let cfg = device.active_config_descriptor()?;
    let mut iface_num = None;
    let mut ep_in = None;
    let mut ep_out = None;
    for iface in cfg.interfaces() {
        for alt in iface.descriptors() {
            if alt.class_code() == 0xFF && alt.sub_class_code() == 0x5D && alt.protocol_code() == 0x01
            {
                iface_num = Some(alt.interface_number());
                for ep in alt.endpoint_descriptors() {
                    if ep.transfer_type() == TransferType::Interrupt {
                        match ep.direction() {
                            Direction::In => ep_in = Some(ep.address()),
                            Direction::Out => ep_out = Some(ep.address()),
                        }
                    }
                }
            }
        }
    }
    let iface_num = iface_num.ok_or("no XInput interface")?;
    let ep_in = ep_in.ok_or("no interrupt IN endpoint")?;
    let ep_out = ep_out.ok_or("no interrupt OUT endpoint")?;
    println!("iface {iface_num}, IN {ep_in:#04x}, OUT {ep_out:#04x}");

    let handle = device.open()?;
    let _ = handle.set_auto_detach_kernel_driver(true);
    handle.claim_interface(iface_num)?;

    let t = Duration::from_millis(100);

    // Candidate "host present" packets used by real/clone Xbox 360 wired pads.
    let packets: &[(&str, &[u8])] = &[
        ("LED player-1", &[0x01, 0x03, 0x02]),
        ("LED rotate", &[0x01, 0x03, 0x0A]),
        ("rumble off", &[0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
        // Some clones expect a zero-length-ish kick / 3-byte 0x00 init.
        ("zero init", &[0x00, 0x00, 0x00]),
    ];
    for (name, pkt) in packets {
        match handle.write_interrupt(ep_out, pkt, t) {
            Ok(n) => println!("sent {name:<13} ({n} bytes): {pkt:02x?}"),
            Err(e) => println!("sent {name:<13} FAILED: {e}"),
        }
    }

    println!("\nNow reading IN endpoint for 5s — press buttons...");
    let mut buf = [0u8; 32];
    let mut got = 0usize;
    let deadline_reads = 50; // ~5s at 100ms timeouts
    for _ in 0..deadline_reads {
        match handle.read_interrupt(ep_in, &mut buf, Duration::from_millis(100)) {
            Ok(n) if n > 0 => {
                got += 1;
                println!("  IN[{n:2}]: {:02x?}", &buf[..n]);
                if got >= 15 {
                    break;
                }
            }
            Ok(_) => {}
            Err(rusb::Error::Timeout) => {}
            Err(e) => {
                println!("  read error: {e}");
                break;
            }
        }
    }

    if got == 0 {
        println!("\nStill no input reports. The controller is not streaming over USB —");
        println!("this is a controller power/state issue, not software.");
    } else {
        println!("\nGot {got} report(s)! The wake handshake worked — wiring this into the reader.");
    }
    Ok(())
}
