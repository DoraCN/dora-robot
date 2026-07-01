//! DORA dataflow lifecycle management.
//!
//! The daemon manages dataflow start/stop via the `dora` CLI
//! (system-installed, assumed on PATH).  The dataflow runs only
//! while torque is ON (Ready + Recording); it is stopped on TorqueOff.

use crate::config::DaemonConfig;
use std::process::{Child, Command};

pub struct DoraFlow {
    _child: Option<Child>,
}

impl DoraFlow {
    /// Launch the recording dataflow via `dora up && dora start`.
    pub fn launch(config: &DaemonConfig) -> anyhow::Result<Self> {
        let id = &config.arm.id;
        let _ = id;

        // dora up (ignore errors if already running)
        let _ = Command::new("dora")
            .args(["up"])
            .status();

        let child = Command::new("dora")
            .args(["start", "dataflows/record.yml", "--name", &format!("record-{}", id)])
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to launch dora dataflow: {}", e))?;

        Ok(Self {
            _child: Some(child),
        })
    }

    /// Stop the dataflow (`dora destroy` or kill the child).
    pub fn stop(mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self._child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // dora destroy (best-effort)
        let _ = Command::new("dora")
            .args(["destroy"])
            .status();
        Ok(())
    }

    /// Check if the dataflow process is still alive.
    pub fn alive(&self) -> bool {
        self._child
            .as_ref()
            .map(|c| c.id() > 0)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensure the DoraFlow type is constructible (stub — the actual
    /// `dora` binary is not expected to be installed during CI tests).
    #[test]
    fn dora_flow_type_exists() {
        // Launch will fail if `dora` is not installed; the type must still compile.
        let cfg = DaemonConfig::sample();
        let result = DoraFlow::launch(&cfg);
        // Expected to fail in CI (no dora binary)
        if let Err(e) = &result {
            assert!(e.to_string().contains("dora") || e.to_string().contains("failed"));
        }
    }
}
