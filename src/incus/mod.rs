//! Typed access to the Incus REST API.

mod client;
mod envelope;
mod events;
mod transport;
mod types;

pub use client::{IncusClient, IncusError};
pub use envelope::ApiResponse;
pub use events::EventEntry;
pub use transport::Transport;
pub use types::{
    AccessEntry, BackupSummary, CertificateSummary, ClusterGroupSummary, ClusterMemberSummary,
    CreateInstanceSpec, ExecResult, ImageAlias, ImagePublishSpec, ImageSummary, ImageUpdateSpec,
    InstanceAction, InstanceDetail, InstanceSummary, MoveInstanceSpec, NetworkAclRule,
    NetworkAclSpec, NetworkAclSummary, NetworkAddressSetSummary, NetworkForwardPort,
    NetworkForwardSpec, NetworkForwardSummary, NetworkForwardUpdateSpec, NetworkIntegrationSummary,
    NetworkLease, NetworkLoadBalancerBackend, NetworkLoadBalancerPort, NetworkLoadBalancerSpec,
    NetworkLoadBalancerSummary, NetworkLoadBalancerUpdateSpec, NetworkPeerSpec, NetworkPeerSummary,
    NetworkPeerUpdateSpec, NetworkSummary, NetworkZoneRecordEntry, NetworkZoneRecordSummary,
    NetworkZoneSummary, Operation, OperationGroups, ProfileSummary, ProjectSummary,
    ResourceDefinition, ResourceKind, ServerEnvironment, ServerInfo, SnapshotSummary,
    StorageBucketKeySummary, StorageBucketSummary, StoragePoolSummary, StorageVolumeBitmap,
    StorageVolumeBitmapSpec, StorageVolumeSnapshotSummary, StorageVolumeSpec, StorageVolumeSummary,
    StorageVolumeUpdateSpec, WarningSummary,
};
