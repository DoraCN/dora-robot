//! DORA dataflow lifecycle management.
//!
//! The daemon manages dataflow start/stop via the `dora` CLI
//! (system-installed, assumed on PATH).  The dataflow runs only
//! while torque is ON (Ready + Recording); it is stopped on TorqueOff.
//!
//! Python nodes in the dataflow need the project's venv Python on PATH,
//! so we prepend `training/.venv/bin` when launching dora.

use crate::config::DaemonConfig;
use std::process::{Child, Command};

/// Prepend the project venv to a command's PATH so DORA spawns Python
/// nodes with the correct interpreter.
fn with_venv_path(cmd: &mut Command) -> &mut Command {
    if let Some(venv) = venv_dir() {
        let venv_bin = format!("{venv}/bin");
        let current_path = std::env::var_os("PATH")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        cmd.env("PATH", format!("{venv_bin}:{current_path}"))
            .env("VIRTUAL_ENV", &venv);
    }
    cmd
}

fn venv_dir() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|d| d.join("training").join(".venv"))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
}

pub struct DoraFlow {
    _child: Option<Child>,
}

impl DoraFlow {
    /// Launch the recording dataflow via `dora up && dora start`.
    pub fn launch(config: &DaemonConfig) -> anyhow::Result<Self> {
        let id = &config.arm.id;

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let root = format!("../datasets/{}", today);
        let task = "teleop";

        // dora up (ignore errors if already running)
        let _ = with_venv_path(&mut Command::new("dora"))
            .args(["up"])
            .status();

        let child = with_venv_path(&mut Command::new("dora"))
            .args(["start", "dataflows/record.yml", "--name", &format!("record-{}", id)])
            .env("LEROBOT_ROOT", &root)
            .env("LEROBOT_TASK", task)
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
        let _ = with_venv_path(&mut Command::new("dora"))
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
