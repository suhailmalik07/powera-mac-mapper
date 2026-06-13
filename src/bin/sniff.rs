//! Raw HID report sniffer for reverse-engineering the report layout.
//!
//! Opens the PowerA controller and prints input reports. By default it only
//! prints when bytes change, and marks which byte indices changed since the
//! last report — so you can press one control at a time and see exactly which
//! bytes it drives.
//!
//! Usage:
//!     cargo run --bin sniff            # only print on change (recommended)
//!     cargo run --bin sniff -- --all   # print every report (for timing/noise)

use hidapi::HidApi;
use std::time::Duration;

const POWERA_VID: u16 = 0x20D6;
const POWERA_PID: u16 = 0x4022;

fn main() {
    let print_all = std::env::args().any(|a| a == "--all");

    let api = HidApi::new().unwrap_or_else(|e| {
        eprintln!("failed to init HID API: {e}");
        std::process::exit(1);
    });

    let device = api.open(POWERA_VID, POWERA_PID).unwrap_or_else(|e| {
        eprintln!(
            "failed to open {POWERA_VID:04x}:{POWERA_PID:04x}: {e}\n\
             Is the controller plugged in? On macOS, input reading may require\n\
             granting this terminal 'Input Monitoring' in System Settings > Privacy."
        );
        std::process::exit(1);
    });

    // Blocking reads with a timeout so Ctrl-C stays responsive.
    device.set_blocking_mode(true).ok();

    println!(
        "Opened PowerA {POWERA_VID:04x}:{POWERA_PID:04x}.\n\
         Press one control at a time and watch which byte index changes.\n\
         Sticks are usually 2 bytes each; triggers 1 byte; buttons are bits.\n\
         (Ctrl-C to stop.)\n"
    );

    let mut buf = [0u8; 64];
    let mut prev: Option<Vec<u8>> = None;

    loop {
        let n = match device.read_timeout(&mut buf, 200) {
            Ok(0) => continue, // timeout, no data
            Ok(n) => n,
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        };
        let report = &buf[..n];

        let changed: Vec<usize> = match &prev {
            Some(p) if p.len() == report.len() => (0..report.len())
                .filter(|&i| p[i] != report[i])
                .collect(),
            _ => (0..report.len()).collect(), // first report or length change: all "new"
        };

        if !print_all && prev.is_some() && changed.is_empty() {
            continue;
        }

        // Hex dump with changed bytes marked by [ ].
        let mut line = format!("len={n:2}  ");
        for (i, b) in report.iter().enumerate() {
            if changed.contains(&i) {
                line.push_str(&format!("[{b:02x}]"));
            } else {
                line.push_str(&format!(" {b:02x} "));
            }
        }
        if !changed.is_empty() && prev.is_some() {
            let idxs: Vec<String> = changed.iter().map(|i| i.to_string()).collect();
            line.push_str(&format!("   changed: {}", idxs.join(",")));
        }
        println!("{line}");

        prev = Some(report.to_vec());

        // tiny sleep to keep output readable if --all
        if print_all {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
