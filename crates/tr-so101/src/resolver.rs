//! USB device resolver: locate the Feetech serial port by VID/PID/Serial.
//!
//! Uses [`usb_resolver`] to scan the USB bus at startup (cold-scan, no hotplug).

use anyhow::{Context, Result};
use usb_resolver::DeviceRule;

/// Config for USB device discovery — mirrors `[arm.<type>]` from TOML.
#[derive(Debug, Clone)]
pub struct UsbDeviceConfig {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
}

/// Parse a hex string like `"0x0483"` or `"0483"` into `u16`.
pub fn parse_hex_u16(s: &str) -> Result<u16> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u16::from_str_radix(s, 16)
        .with_context(|| format!("invalid hex value: {}", s))
}

/// Resolve the serial port path for a connected SO-101 arm.
///
/// Scans the USB bus via [`usb_resolver::get_monitor`], matches against
/// `config` using [`DeviceRule::matches`], and returns the OS-specific
/// device path (`/dev/cu.*` on macOS, `/dev/ttyUSB*` on Linux).
pub fn resolve_arm_port(config: &UsbDeviceConfig) -> Result<String> {
    let rule = DeviceRule {
        role: "arm".into(),
        vid: config.vid,
        pid: config.pid,
        serial: config.serial.clone(),
        port_path: None,
    };

    let monitor = usb_resolver::get_monitor();
    let devices = monitor.scan_now().context("USB scan failed")?;

    for raw in &devices {
        if let Some(method) = rule.matches(raw) {
            let path = raw
                .system_path_alt
                .clone()
                .or_else(|| Some(raw.system_path.clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "matched USB device but no serial path (vid={:#06x}, pid={:#06x})",
                        config.vid,
                        config.pid
                    )
                })?;

            log::info!(
                "USB match: {:?} → {} (vid={:#06x}, pid={:#06x})",
                method,
                path,
                raw.vid,
                raw.pid
            );
            return Ok(path);
        }
    }

    Err(anyhow::anyhow!(
        "no matching USB device: vid={:#06x}, pid={:#06x}{}",
        config.vid,
        config.pid,
        config
            .serial
            .as_ref()
            .map(|s| format!(", serial={}", s))
            .unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_with_prefix() {
        assert_eq!(parse_hex_u16("0x0483").unwrap(), 0x0483);
        assert_eq!(parse_hex_u16("0x5740").unwrap(), 0x5740);
    }

    #[test]
    fn parse_hex_without_prefix() {
        assert_eq!(parse_hex_u16("0483").unwrap(), 0x0483);
    }

    #[test]
    fn parse_hex_lowercase() {
        assert_eq!(parse_hex_u16("0xabcd").unwrap(), 0xabcd);
    }

    #[test]
    fn parse_hex_max() {
        assert_eq!(parse_hex_u16("0xffff").unwrap(), 0xffff);
    }

    #[test]
    fn parse_hex_invalid() {
        assert!(parse_hex_u16("0xghij").is_err());
        assert!(parse_hex_u16("").is_err());
        assert!(parse_hex_u16("0x1_0000").is_err()); // overflow
    }

    #[test]
    fn parse_hex_zero() {
        assert_eq!(parse_hex_u16("0x0000").unwrap(), 0);
        assert_eq!(parse_hex_u16("0").unwrap(), 0);
    }

    #[test]
    fn device_config_defaults() {
        let cfg = UsbDeviceConfig {
            vid: 0x0483,
            pid: 0x5740,
            serial: None,
        };
        assert_eq!(cfg.vid, 0x0483);
        assert_eq!(cfg.pid, 0x5740);
        assert!(cfg.serial.is_none());
    }

    #[test]
    fn device_config_with_serial() {
        let cfg = UsbDeviceConfig {
            vid: 0x0483,
            pid: 0x5740,
            serial: Some("5A7A0555021".into()),
        };
        assert_eq!(cfg.serial, Some("5A7A0555021".into()));
    }
}
