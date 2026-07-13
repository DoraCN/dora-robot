//! Daemon configuration — loaded from `config/follower.toml` / `config/leader.toml`.

use serde::Deserialize;

/// Root config loaded from TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
    pub arm: ArmConfig,
    #[serde(default)]
    pub zenoh: ZenohConfig,
}

/// Optional zenoh communication settings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ZenohConfig {
    /// Explicit peer endpoints (tcp/<ip>:<port>), e.g. ["tcp/192.168.1.20:7447"]
    #[serde(default)]
    pub peers: Vec<String>,
    /// Fixed listen port, e.g. "7447". Leave empty for random.
    #[serde(default)]
    pub listen: Option<String>,
}

/// Instance identity + hardware type selector.
#[derive(Debug, Clone, Deserialize)]
pub struct ArmConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub arm_type: String,
    pub so101: So101Config,
}

/// SO-101 hardware parameters (static, same for all SO-101 arms).
#[derive(Debug, Clone, Deserialize)]
pub struct So101Config {
    pub baud: u32,
    pub ids: Vec<u8>,
    pub vid: String,
    pub pid: String,
    pub serial: Option<String>,
}

impl DaemonConfig {
    /// Parse from TOML string.
    pub fn from_str(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Sample config for integration tests.
    pub fn sample() -> Self {
        Self::from_str(
            r#"
[arm]
id = "arm_test"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"
pid = "0x5740"
serial = "TEST0001"

[zenoh]
# peers = ["tcp/192.168.1.20:7447"]
"#,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC_TOML: &str = r#"
[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"
pid = "0x5740"
serial = "5A7A0555021"
"#;

    #[test]
    fn parse_basic_config() {
        let cfg = DaemonConfig::from_str(BASIC_TOML).unwrap();
        assert_eq!(cfg.arm.id, "arm_1");
        assert_eq!(cfg.arm.arm_type, "so101");
    }

    #[test]
    fn parse_so101_fields() {
        let cfg = DaemonConfig::from_str(BASIC_TOML).unwrap();
        let s = &cfg.arm.so101;
        assert_eq!(s.baud, 1_000_000);
        assert_eq!(s.ids, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(s.vid, "0x0483");
        assert_eq!(s.pid, "0x5740");
        assert_eq!(s.serial.as_deref(), Some("5A7A0555021"));
    }

    #[test]
    fn parse_without_serial() {
        let toml = r#"
[arm]
id = "arm_2"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"
pid = "0x5740"
"#;
        let cfg = DaemonConfig::from_str(toml).unwrap();
        assert!(cfg.arm.so101.serial.is_none());
    }

    #[test]
    fn parse_missing_arm_fails() {
        let toml = r#"
[foo]
bar = 1
"#;
        assert!(DaemonConfig::from_str(toml).is_err());
    }

    #[test]
    fn parse_missing_ids_fails() {
        let toml = r#"
[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
vid = "0x0483"
pid = "0x5740"
"#;
        assert!(DaemonConfig::from_str(toml).is_err());
    }

    #[test]
    fn parse_baud_zero() {
        let toml = r#"
[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 0
ids = [1, 2]
vid = "0x0483"
pid = "0x5740"
"#;
        let cfg = DaemonConfig::from_str(toml).unwrap();
        assert_eq!(cfg.arm.so101.baud, 0);
    }

    #[test]
    fn parse_custom_arm_type() {
        let toml = r#"
[arm]
id = "arm_3"
type = "ur5"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"
pid = "0x5740"
"#;
        let cfg = DaemonConfig::from_str(toml).unwrap();
        assert_eq!(cfg.arm.arm_type, "ur5");
        assert_eq!(cfg.arm.id, "arm_3");
    }
}
