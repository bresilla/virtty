use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A mutable Incus API object retrieved together with its ETag revision.
///
/// Editing must send this ETag back in `If-Match`; otherwise a stale TUI can
/// silently overwrite a change made by another administrator.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceDefinition {
    pub kind: ResourceKind,
    pub name: String,
    pub project: String,
    pub value: Value,
    pub etag: Option<String>,
}

/// Top-level resource families supported by the generic configuration editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    Instance,
    Network,
    StoragePool,
    Project,
    Profile,
    Image,
    Server,
}

impl ResourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Instance => "instance",
            Self::Network => "network",
            Self::StoragePool => "storage pool",
            Self::Project => "project",
            Self::Profile => "profile",
            Self::Image => "image",
            Self::Server => "server",
        }
    }
}

/// Metadata returned by `GET /1.0`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub api_extensions: Vec<String>,
    #[serde(default)]
    pub api_status: String,
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub environment: ServerEnvironment,
}

/// The useful, stable server fields from an Incus server response.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ServerEnvironment {
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub server_clustered: bool,
    #[serde(default)]
    pub kernel_version: String,
}

/// An instance returned by `GET /1.0/instances?recursion=2`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct InstanceSummary {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub instance_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: i64,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub profiles: Vec<String>,
}

/// Full instance definition used by the details pane and later edit flows.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct InstanceDetail {
    #[serde(flatten)]
    pub summary: InstanceSummary,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub devices: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub expanded_config: BTreeMap<String, String>,
    #[serde(default)]
    pub expanded_devices: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default)]
    pub stateful: bool,
}

impl InstanceSummary {
    /// Formats the Incus resource type for the compact table.
    pub fn type_label(&self) -> &str {
        match self.instance_type.as_str() {
            "virtual-machine" => "VM",
            "container" => "CT",
            "" => "?",
            other => other,
        }
    }
}

/// A supported lifecycle target for an instance state request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceAction {
    Start,
    Stop,
    Restart,
    Freeze,
    Unfreeze,
    Delete,
}

impl InstanceAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Freeze => "freeze",
            Self::Unfreeze => "unfreeze",
            Self::Delete => "delete",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::Freeze => "Freeze",
            Self::Unfreeze => "Unfreeze",
            Self::Delete => "Delete",
        }
    }

    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Stop | Self::Restart | Self::Delete)
    }
}

/// An asynchronous Incus operation.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Operation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: i64,
    #[serde(default)]
    pub may_cancel: bool,
    #[serde(default)]
    pub err: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub resources: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub metadata: Value,
}

/// Captured output from a non-interactive instance command.
#[derive(Clone, Debug, Default)]
pub struct ExecResult {
    pub operation: Operation,
    pub exit_status: Option<i64>,
    pub stdout: String,
    pub stderr: String,
}

/// Operations grouped by Incus's lifecycle category.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct OperationGroups {
    #[serde(default)]
    pub running: Vec<Operation>,
    #[serde(default)]
    pub failure: Vec<Operation>,
    #[serde(default)]
    pub success: Vec<Operation>,
}

impl OperationGroups {
    /// Flattens the groups with running work first, suitable for a compact TUI table.
    pub fn into_recent(mut self) -> Vec<Operation> {
        self.running.append(&mut self.failure);
        self.running.append(&mut self.success);
        self.running
    }
}

/// A project returned by `GET /1.0/projects?recursion=1`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProjectSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct AccessEntry {
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub provider: String,
}

/// A storage pool returned by `GET /1.0/storage-pools?recursion=1`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StoragePoolSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub locations: Vec<String>,
}

/// A custom storage volume inside an Incus pool.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StorageVolumeSummary {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub volume_type: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub used_by: Vec<String>,
}

/// Structured request for a custom storage volume.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StorageVolumeSpec {
    pub pool: String,
    pub project: String,
    pub name: String,
    pub content_type: String,
    pub description: String,
    pub size: String,
    pub initial_uid: String,
    pub initial_gid: String,
    pub initial_mode: String,
    pub shared: bool,
}

/// Structured mutable properties for an existing custom storage volume.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StorageVolumeUpdateSpec {
    pub pool: String,
    pub project: String,
    pub name: String,
    pub description: String,
    pub size: String,
    pub config: BTreeMap<String, String>,
}

/// Snapshot of a custom storage volume.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StorageVolumeSnapshotSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StorageVolumeBitmap {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub recording: bool,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub inconsistent: bool,
    #[serde(default)]
    pub granularity: i64,
    #[serde(default)]
    pub count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct StorageVolumeBitmapSpec {
    pub name: String,
    pub persistent: bool,
    pub disabled: bool,
    pub granularity: i64,
}

/// S3-compatible bucket managed by an Incus storage pool.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StorageBucketSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

/// Access credential associated with a storage bucket.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StorageBucketKeySummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
}

/// A managed or unmanaged Incus network.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkSummary {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub network_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkIntegrationSummary {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub integration_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

/// A DHCP or gateway lease on a managed network.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkLease {
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub hwaddr: String,
    #[serde(rename = "type", default)]
    pub lease_type: String,
    #[serde(default)]
    pub location: String,
}

/// A port mapping within an Incus network forward.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct NetworkForwardPort {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub listen_port: String,
    #[serde(default)]
    pub target_address: String,
    #[serde(default)]
    pub target_port: String,
}

/// A listening address and its forwarded ports.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkForwardSummary {
    #[serde(default)]
    pub listen_address: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub ports: Vec<NetworkForwardPort>,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkForwardSpec {
    pub network: String,
    pub project: String,
    pub listen_address: String,
    pub description: String,
    pub port: NetworkForwardPort,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkForwardUpdateSpec {
    pub network: String,
    pub project: String,
    pub listen_address: String,
    pub description: String,
    pub ports: Vec<NetworkForwardPort>,
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct NetworkLoadBalancerBackend {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub target_address: String,
    #[serde(default)]
    pub target_port: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct NetworkLoadBalancerPort {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub listen_port: String,
    #[serde(default)]
    pub target_backend: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkLoadBalancerSummary {
    #[serde(default)]
    pub listen_address: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub backends: Vec<NetworkLoadBalancerBackend>,
    #[serde(default)]
    pub ports: Vec<NetworkLoadBalancerPort>,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkLoadBalancerSpec {
    pub network: String,
    pub project: String,
    pub listen_address: String,
    pub description: String,
    pub backend: NetworkLoadBalancerBackend,
    pub port: NetworkLoadBalancerPort,
    pub config: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkLoadBalancerUpdateSpec {
    pub network: String,
    pub project: String,
    pub listen_address: String,
    pub description: String,
    pub backends: Vec<NetworkLoadBalancerBackend>,
    pub ports: Vec<NetworkLoadBalancerPort>,
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkPeerSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "type")]
    pub peer_type: String,
    #[serde(default)]
    pub target_project: String,
    #[serde(default)]
    pub target_network: String,
    #[serde(default)]
    pub target_integration: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkPeerSpec {
    pub network: String,
    pub project: String,
    pub name: String,
    pub description: String,
    pub peer_type: String,
    pub target_project: String,
    pub target_network: String,
    pub target_integration: String,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkPeerUpdateSpec {
    pub network: String,
    pub project: String,
    pub name: String,
    pub description: String,
    pub peer_type: String,
    pub target_project: String,
    pub target_network: String,
    pub target_integration: String,
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkZoneSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
    #[serde(default)]
    pub used_by: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct NetworkZoneRecordEntry {
    #[serde(rename = "type", default)]
    pub entry_type: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub ttl: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkZoneRecordSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub entries: Vec<NetworkZoneRecordEntry>,
}

/// A network ACL and its ingress/egress rule counts.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct NetworkAclSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub ingress: Vec<NetworkAclRule>,
    #[serde(default)]
    pub egress: Vec<NetworkAclRule>,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub used_by: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct NetworkAclRule {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub source_port: String,
    #[serde(default)]
    pub destination_port: String,
    #[serde(default)]
    pub icmp_type: String,
    #[serde(default)]
    pub icmp_code: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct NetworkAddressSetSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub used_by: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkAclSpec {
    pub project: String,
    pub name: String,
    pub description: String,
}

/// An image alias carried by an Incus image.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ImageAlias {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// An image returned by `GET /1.0/images?recursion=1`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ImageSummary {
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub aliases: Vec<ImageAlias>,
    #[serde(default)]
    pub architecture: String,
    #[serde(rename = "type", default)]
    pub image_type: String,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub auto_update: bool,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub uploaded_at: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImageUpdateSpec {
    pub fingerprint: String,
    pub project: String,
    pub public: bool,
    pub auto_update: bool,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImagePublishSpec {
    pub instance: String,
    pub project: String,
    pub alias: String,
    pub description: String,
    pub public: bool,
}

impl ImageSummary {
    /// The first alias if present, otherwise a short fingerprint.
    pub fn display_name(&self) -> String {
        self.aliases
            .first()
            .map(|alias| alias.name.clone())
            .filter(|alias| !alias.is_empty())
            .unwrap_or_else(|| self.fingerprint.chars().take(12).collect())
    }
}

/// An instance profile returned by `GET /1.0/profiles?recursion=1`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project: String,
}

/// User-entered configuration for a new Incus container or VM.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateInstanceSpec {
    pub name: String,
    pub project: String,
    pub instance_type: String,
    pub image: String,
    pub profiles: Vec<String>,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub storage_pool: String,
    pub root_disk_size: String,
    pub network: String,
    pub cloud_init_user_data: String,
    pub target_member: String,
    pub privileged: bool,
    pub nesting: bool,
    pub secure_boot: bool,
    pub autostart: bool,
    pub start_after_create: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MoveInstanceSpec {
    pub name: String,
    pub project: String,
    pub new_name: String,
    pub target_project: String,
    pub target_member: String,
    pub target_pool: String,
    pub live: bool,
}

impl Default for CreateInstanceSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            project: "default".to_owned(),
            instance_type: "container".to_owned(),
            image: "images:debian/12".to_owned(),
            profiles: vec!["default".to_owned()],
            cpu_limit: String::new(),
            memory_limit: String::new(),
            storage_pool: String::new(),
            root_disk_size: String::new(),
            network: String::new(),
            cloud_init_user_data: String::new(),
            target_member: String::new(),
            privileged: false,
            nesting: false,
            secure_boot: true,
            autostart: false,
            start_after_create: false,
        }
    }
}

/// A point-in-time instance snapshot.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SnapshotSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stateful: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: String,
}

/// An exportable instance backup.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BackupSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub instance_only: bool,
    #[serde(default)]
    pub optimized_storage: bool,
}

/// Server warning surfaced by the Incus warning API.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WarningSummary {
    #[serde(default)]
    pub uuid: String,
    #[serde(rename = "type", default)]
    pub warning_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub last_message: String,
    #[serde(default)]
    pub first_seen_at: String,
    #[serde(default)]
    pub last_seen_at: String,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub entity_url: String,
}

/// Trusted TLS certificate and its project restrictions.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CertificateSummary {
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub certificate_type: String,
    #[serde(default)]
    pub restricted: bool,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub description: String,
}

/// One member of an Incus cluster.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ClusterMemberSummary {
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub failure_domain: String,
    #[serde(default)]
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ClusterGroupSummary {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub members: Vec<String>,
}
