//! UI-oriented, normalized state derived from Incus resources.

use std::time::SystemTime;

use crate::incus::{
    CertificateSummary, ClusterMemberSummary, EventEntry, ImageSummary, InstanceSummary,
    NetworkSummary, Operation, ProfileSummary, ProjectSummary, ServerInfo, StoragePoolSummary,
    WarningSummary,
};

/// Data loaded for the current TUI view.
#[derive(Clone, Debug, Default)]
pub struct AppModel {
    pub server: Option<ServerInfo>,
    pub host_resources: Option<serde_json::Value>,
    pub metrics: Option<String>,
    pub network_allocations: Vec<serde_json::Value>,
    pub instances: Vec<InstanceSummary>,
    pub projects: Vec<ProjectSummary>,
    pub operations: Vec<Operation>,
    pub events: Vec<EventEntry>,
    pub storage_pools: Vec<StoragePoolSummary>,
    pub networks: Vec<NetworkSummary>,
    pub images: Vec<ImageSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub warnings: Vec<WarningSummary>,
    pub certificates: Vec<CertificateSummary>,
    pub cluster_members: Vec<ClusterMemberSummary>,
    pub last_operation: Option<Operation>,
    pub refreshed_at: Option<SystemTime>,
    pub last_error: Option<String>,
}

impl AppModel {
    /// Count instances by their current Incus lifecycle status.
    pub fn instance_counts(&self) -> InstanceCounts {
        let mut counts = InstanceCounts::default();
        for instance in &self.instances {
            match instance.status.as_str() {
                "Running" => counts.running += 1,
                "Stopped" => counts.stopped += 1,
                "Frozen" => counts.frozen += 1,
                _ => counts.other += 1,
            }
            if instance.instance_type == "container" {
                counts.containers += 1;
            } else if instance.instance_type == "virtual-machine" {
                counts.virtual_machines += 1;
            }
        }
        counts
    }
}

/// Compact dashboard aggregate for instance state and type.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstanceCounts {
    pub running: usize,
    pub stopped: usize,
    pub frozen: usize,
    pub other: usize,
    pub containers: usize,
    pub virtual_machines: usize,
}

#[cfg(test)]
mod tests {
    use crate::incus::InstanceSummary;

    use super::AppModel;

    #[test]
    fn summarizes_lifecycle_and_machine_types() {
        let model = AppModel {
            instances: vec![
                InstanceSummary {
                    status: "Running".into(),
                    instance_type: "container".into(),
                    ..Default::default()
                },
                InstanceSummary {
                    status: "Stopped".into(),
                    instance_type: "virtual-machine".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(model.instance_counts().running, 1);
        assert_eq!(model.instance_counts().stopped, 1);
        assert_eq!(model.instance_counts().containers, 1);
        assert_eq!(model.instance_counts().virtual_machines, 1);
    }
}
