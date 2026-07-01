//! USB device scanner — scan for USB devices and print suggested
//! `[arm.so101]` config snippets.
//!
//! Usage:
//!   cargo run -p tr-so101 --example usb_scan

use usb_resolver::DeviceRule;

fn main() {
    println!("── USB Serial Device Scanner ──\n");

    let monitor = usb_resolver::get_monitor();

    let devices = match monitor.scan_now() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("scan failed: {e}");
            std::process::exit(1);
        }
    };

    if devices.is_empty() {
        println!("No USB devices found.");
        println!("Connect the SO-101 arm via USB and try again.");
        return;
    }

    println!("Found {} device(s):\n", devices.len());

    let stm32_rule = DeviceRule {
        role: "arm".into(),
        vid: 0x0483,
        pid: 0x5740,
        serial: None,
        port_path: None,
    };

    for (i, dev) in devices.iter().enumerate() {
        println!("── Device {} ──", i + 1);
        println!("  VID   : 0x{:04X}", dev.vid);
        println!("  PID   : 0x{:04X}", dev.pid);
        println!("  Serial: {}", dev.serial.as_deref().unwrap_or("(none)"));
        println!("  Port  : {}", dev.port_path);
        if let Some(ref alt) = dev.system_path_alt {
            println!("  Dev   : {}", alt);
        }

        if let Some(method) = stm32_rule.matches(dev) {
            println!("\n  => matches SO-101 (STM32 CDC): {:?}", method);
            println!("  Suggested config:");
            println!("    [arm.so101]");
            println!("    vid = \"0x{:04X}\"", dev.vid);
            println!("    pid = \"0x{:04X}\"", dev.pid);
            if let Some(ref serial) = dev.serial {
                println!("    serial = \"{}\"", serial);
            }
        } else {
            println!("\n  => generic rule:");
            println!("    vid = \"0x{:04X}\"", dev.vid);
            println!("    pid = \"0x{:04X}\"", dev.pid);
            if let Some(ref serial) = dev.serial {
                println!("    serial = \"{}\"", serial);
            }
        }
        println!();
    }
}
