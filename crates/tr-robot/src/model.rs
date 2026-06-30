//! Robot description. In production load this from URDF (`urdf-rs`).

#[derive(Debug, Clone, Copy)]
pub struct JointLimit {
    pub lower: f64,
    pub upper: f64,
    pub max_velocity: f64,
}

impl JointLimit {
    pub fn clamp(&self, q: f64) -> f64 {
        q.clamp(self.lower, self.upper)
    }
}

#[derive(Debug, Clone)]
pub struct RobotModel {
    pub name: String,
    pub dof: u32,
    pub joint_names: Vec<String>,
    pub limits: Vec<JointLimit>,
    pub end_effectors: Vec<String>,
}

impl RobotModel {
    /// A generic n-DoF arm with symmetric limits — placeholder for a URDF load.
    pub fn generic_arm(name: impl Into<String>, dof: u32) -> Self {
        let dof_us = dof as usize;
        Self {
            name: name.into(),
            dof,
            joint_names: (0..dof_us).map(|i| format!("joint_{i}")).collect(),
            limits: vec![
                JointLimit {
                    lower: -std::f64::consts::PI,
                    upper: std::f64::consts::PI,
                    max_velocity: 3.14,
                };
                dof_us
            ],
            end_effectors: vec!["tcp".into()],
        }
    }

    /// Clamp a joint vector to the configured position limits.
    pub fn clamp_positions(&self, q: &mut [f64]) {
        for (i, v) in q.iter_mut().enumerate() {
            if let Some(limit) = self.limits.get(i) {
                *v = limit.clamp(*v);
            }
        }
    }
}
