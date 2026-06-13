//! Raw XInput-endpoint dump for calibration.
//!
//! Reads the first PowerA XInput device's interrupt IN endpoint and prints the
//! raw report bytes whenever they change, marking which byte indices changed.
//! Use it to confirm the report layout: move ONE control at a time and watch
//! which bytes move.
//!
//!     cargo run --bin rawread

use rusb::{Context, Direction, TransferType, UsbContext};
use std::time::Duration;

const POWERA_VID: u16 = 0x20D6;

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
                .map(|x| x.vendor_id() == POWERA_VID)
                .unwrap_or(false)
        })
        .ok_or("no PowerA device found")?;

    let cfg = device.active_config_descriptor()?;
    let (mut iface_num, mut ep_in) = (None, None);
    for iface in cfg.interfaces() {
        for alt in iface.descriptors() {
            if alt.class_code() == 0xFF && alt.sub_class_code() == 0x5D && alt.protocol_code() == 0x01
            {
                iface_num = Some(alt.interface_number());
                for ep in alt.endpoint_descriptors() {
                    if ep.direction() == Direction::In
                        && ep.transfer_type() == TransferType::Interrupt
                    {
                        ep_in = Some(ep.address());
                    }
                }
            }
        }
    }
    let iface_num = iface_num.ok_or("no XInput interface")?;
    let ep_in = ep_in.ok_or("no interrupt IN endpoint")?;

    let handle = device.open()?;
    let _ = handle.set_auto_detach_kernel_driver(true);
    handle.claim_interface(iface_num)?;

    println!("Raw XInput dump (iface {iface_num}, ep {ep_in:#04x}). Move ONE control at a time.\n\
              Byte index ruler:");
    print!("        ");
    for i in 0..20 {
        print!("{i:>2} ");
    }
    println!("\n");

    let mut buf = [0u8; 32];
    let mut prev: Option<Vec<u8>> = None;
    loop {
        let n = match handle.read_interrupt(ep_in, &mut buf, Duration::from_millis(500)) {
            Ok(n) if n > 0 => n,
            Ok(_) | Err(rusb::Error::Timeout) => continue,
            Err(e) => return Err(e.into()),
        };
        let report = &buf[..n];
        let changed: Vec<usize> = match &prev {
            Some(p) if p.len() == report.len() => {
                (0..report.len()).filter(|&i| p[i] != report[i]).collect()
            }
            _ => vec![],
        };
        if prev.is_some() && changed.is_empty() {
            continue;
        }
        let mut line = format!("n={n:2}  ");
        for (i, b) in report.iter().enumerate() {
            if changed.contains(&i) {
                line.push_str(&format!("[{b:02x}]"));
            } else {
                line.push_str(&format!(" {b:02x} "));
            }
        }
        if !changed.is_empty() {
            let idxs: Vec<String> = changed.iter().map(|i| i.to_string()).collect();
            line.push_str(&format!("  <- {}", idxs.join(",")));
        }
        println!("{line}");
        prev = Some(report.to_vec());
    }
}
