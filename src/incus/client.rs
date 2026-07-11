use std::{collections::BTreeMap, error::Error, fmt, path::Path, time::Duration};

use reqwest::{Client, StatusCode, header::IF_MATCH};
use serde_json::{Value, json};
use url::form_urlencoded::byte_serialize;

use tokio::sync::mpsc;

use super::{
    AccessEntry, ApiResponse, BackupSummary, CertificateSummary, ClusterGroupSummary,
    ClusterMemberSummary, CreateInstanceSpec, EventEntry, ExecResult, ImagePublishSpec,
    ImageSummary, ImageUpdateSpec, InstanceAction, InstanceDetail, InstanceSummary,
    MoveInstanceSpec, NetworkAclRule, NetworkAclSummary, NetworkAddressSetSummary,
    NetworkForwardPort, NetworkForwardSummary, NetworkForwardUpdateSpec, NetworkIntegrationSummary,
    NetworkLease, NetworkLoadBalancerBackend, NetworkLoadBalancerPort, NetworkLoadBalancerSpec,
    NetworkLoadBalancerSummary, NetworkLoadBalancerUpdateSpec, NetworkPeerSpec, NetworkPeerSummary,
    NetworkPeerUpdateSpec, NetworkSummary, NetworkZoneRecordEntry, NetworkZoneRecordSummary,
    NetworkZoneSummary, Operation, OperationGroups, ProfileSummary, ProjectSummary,
    ResourceDefinition, ResourceKind, ServerInfo, SnapshotSummary, StorageBucketKeySummary,
    StorageBucketSummary, StoragePoolSummary, StorageVolumeBitmap, StorageVolumeBitmapSpec,
    StorageVolumeSnapshotSummary, StorageVolumeSpec, StorageVolumeSummary, StorageVolumeUpdateSpec,
    Transport, WarningSummary, events,
};

const LOCAL_API_URL: &str = "http://incus/1.0";
const USER_AGENT: &str = concat!("virtty/", env!("CARGO_PKG_VERSION"));

/// Direct Incus REST API client.
#[derive(Clone)]
pub struct IncusClient {
    http: Client,
    api_url: String,
    transport: Transport,
}

impl IncusClient {
    pub fn from_transport(transport: Transport) -> Result<Self, IncusError> {
        match transport {
            Transport::LocalUnix { socket_path } => Self::local_socket(socket_path),
            Transport::Remote {
                base_url,
                client_certificate: Some(certificate),
                client_key: Some(key),
                server_ca,
            } => Self::remote_tls(base_url, certificate, key, server_ca),
            Transport::Remote { base_url, .. } => Self::remote(base_url),
        }
    }

    /// Connects through the standard local Incus Unix socket.
    pub fn local_default() -> Result<Self, IncusError> {
        Self::local_socket("/var/lib/incus/unix.socket")
    }

    /// Connects through an explicit local Incus Unix socket.
    pub fn local_socket(socket_path: impl AsRef<Path>) -> Result<Self, IncusError> {
        let transport = Transport::local(socket_path.as_ref());

        #[cfg(unix)]
        let http = Client::builder()
            .unix_socket(socket_path.as_ref())
            .timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build()
            .map_err(IncusError::Request)?;

        #[cfg(not(unix))]
        let http = return Err(IncusError::UnsupportedLocalSocket);

        Ok(Self {
            http,
            api_url: LOCAL_API_URL.to_owned(),
            transport,
        })
    }

    /// Connects to a remote Incus API endpoint.
    pub fn remote(base_url: impl Into<String>) -> Result<Self, IncusError> {
        let base_url = base_url.into();
        let api_url = api_url_for_remote(&base_url)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build()
            .map_err(IncusError::Request)?;

        Ok(Self {
            http,
            api_url,
            transport: Transport::remote(base_url),
        })
    }

    /// Connects to a remote with an explicit client TLS identity and optional private CA.
    pub fn remote_tls(
        base_url: impl Into<String>,
        certificate_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
        server_ca_path: Option<impl AsRef<Path>>,
    ) -> Result<Self, IncusError> {
        let base_url = base_url.into();
        let api_url = api_url_for_remote(&base_url)?;
        let certificate = std::fs::read(certificate_path.as_ref()).map_err(IncusError::Io)?;
        let key = std::fs::read(key_path.as_ref()).map_err(IncusError::Io)?;
        let mut identity_pem = certificate;
        identity_pem.extend_from_slice(b"\n");
        identity_pem.extend_from_slice(&key);
        let identity = reqwest::Identity::from_pem(&identity_pem).map_err(IncusError::Request)?;
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .identity(identity);
        let ca_path = server_ca_path
            .as_ref()
            .map(|path| path.as_ref().to_path_buf());
        if let Some(path) = &ca_path {
            let pem = std::fs::read(path).map_err(IncusError::Io)?;
            builder = builder.add_root_certificate(
                reqwest::Certificate::from_pem(&pem).map_err(IncusError::Request)?,
            );
        }
        let http = builder.build().map_err(IncusError::Request)?;
        Ok(Self {
            http,
            api_url,
            transport: Transport::remote_tls(
                base_url,
                certificate_path,
                key_path,
                ca_path.as_ref(),
            ),
        })
    }

    /// Returns the selected connection configuration.
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Subscribes to Incus lifecycle, operation, and logging events in the background.
    pub fn subscribe_events(&self) -> mpsc::Receiver<EventEntry> {
        events::subscribe(self.transport.clone())
    }

    /// Reads server and API metadata.
    pub async fn server_info(&self) -> Result<ServerInfo, IncusError> {
        self.get("").await
    }

    /// Reads host CPU, memory, disk, GPU, and network inventory/usage data.
    pub async fn resources(&self) -> Result<Value, IncusError> {
        self.get("/resources").await
    }

    /// Returns the Prometheus-format per-instance and host metrics exposed by Incus.
    pub async fn metrics(&self) -> Result<String, IncusError> {
        self.get_text("/metrics").await
    }

    pub async fn metadata_configuration(&self) -> Result<String, IncusError> {
        self.get("/metadata/configuration").await
    }

    /// Lists instances with their metadata in a single request.
    pub async fn instances(&self) -> Result<Vec<InstanceSummary>, IncusError> {
        self.get("/instances?recursion=2&all-projects=true").await
    }

    /// Retrieves one instance definition including config and devices.
    pub async fn instance_detail(
        &self,
        name: &str,
        project: &str,
    ) -> Result<InstanceDetail, IncusError> {
        self.get(&append_query(&instance_path(name, project), "recursion=1"))
            .await
    }

    /// Gets normalized runtime state, including guest-agent OS information when available.
    pub async fn instance_state(&self, name: &str, project: &str) -> Result<Value, IncusError> {
        self.get(&instance_subpath(name, project, "state")).await
    }

    pub async fn instance_access(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<AccessEntry>, IncusError> {
        self.get(&instance_subpath(name, project, "access")).await
    }

    /// Lists projects with their metadata in a single request.
    pub async fn projects(&self) -> Result<Vec<ProjectSummary>, IncusError> {
        self.get("/projects?recursion=1").await
    }

    pub async fn project_state(&self, name: &str) -> Result<Value, IncusError> {
        self.get(&format!("/projects/{name}/state")).await
    }

    pub async fn project_access(&self, name: &str) -> Result<Vec<AccessEntry>, IncusError> {
        self.get(&format!("/projects/{name}/access")).await
    }

    /// Lists storage pools with their summary metadata.
    pub async fn storage_pools(&self) -> Result<Vec<StoragePoolSummary>, IncusError> {
        self.get("/storage-pools?recursion=1").await
    }

    pub async fn storage_pool_resources(&self, pool: &str) -> Result<Value, IncusError> {
        self.get(&format!("/storage-pools/{pool}/resources")).await
    }

    pub async fn storage_volume_state(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
    ) -> Result<Value, IncusError> {
        self.get(&format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/state{}",
            project_query(project)
        ))
        .await
    }

    pub async fn storage_volume_bitmaps(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
    ) -> Result<Vec<StorageVolumeBitmap>, IncusError> {
        self.get(&append_query(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/bitmaps{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_storage_volume_bitmap(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        spec: &StorageVolumeBitmapSpec,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/bitmaps{}",
                project_query(project)
            ),
            spec,
        )
        .await
    }

    pub async fn delete_storage_volume_bitmap(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        bitmap: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/bitmaps/{}{}",
            encode_path_segment(bitmap),
            project_query(project)
        ))
        .await
    }

    /// Lists custom volumes in a storage pool across projects.
    pub async fn storage_volumes(
        &self,
        pool: &str,
    ) -> Result<Vec<StorageVolumeSummary>, IncusError> {
        self.get(&format!(
            "/storage-pools/{pool}/volumes/custom?recursion=1&all-projects=true"
        ))
        .await
    }

    /// Creates a custom storage volume.
    pub async fn create_storage_volume(
        &self,
        spec: &StorageVolumeSpec,
    ) -> Result<Value, IncusError> {
        if spec.name.trim().is_empty() {
            return Err(IncusError::InvalidResource(
                "storage volume name is required".to_owned(),
            ));
        }
        let mut config = BTreeMap::new();
        if !spec.size.trim().is_empty() {
            config.insert("size".to_owned(), spec.size.trim().to_owned());
        }
        for (key, value) in [
            ("initial.uid", spec.initial_uid.trim()),
            ("initial.gid", spec.initial_gid.trim()),
            ("initial.mode", spec.initial_mode.trim()),
        ] {
            if !value.is_empty() {
                config.insert(key.to_owned(), value.to_owned());
            }
        }
        if spec.shared {
            config.insert("security.shared".to_owned(), "true".to_owned());
        }
        self.post(
            &format!(
                "/storage-pools/{}/volumes/custom{}",
                spec.pool,
                project_query(&spec.project)
            ),
            &StorageVolumeCreateRequest {
                name: spec.name.trim(),
                volume_type: "custom",
                content_type: &spec.content_type,
                description: spec.description.trim(),
                config,
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn copy_storage_volume(
        &self,
        source_pool: &str,
        source_project: &str,
        source_name: &str,
        target_pool: &str,
        target_project: &str,
        target_name: &str,
        volume_only: bool,
        refresh: bool,
        live: bool,
        refresh_exclude_older: bool,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{target_pool}/volumes/custom{}",
                project_query(target_project)
            ),
            &json!({
                "name": target_name,
                "type": "custom",
                "source": {
                    "type": "copy",
                    "name": source_name,
                    "pool": source_pool,
                    "project": source_project,
                    "volume_only": volume_only,
                    "refresh": refresh,
                    "live": live,
                    "refresh_exclude_older": refresh_exclude_older,
                }
            }),
        )
        .await
    }

    pub async fn import_storage_volume(
        &self,
        pool: &str,
        project: &str,
        name: &str,
        import_type: &str,
        archive: Vec<u8>,
    ) -> Result<Operation, IncusError> {
        self.send(
            self.http
                .post(format!(
                    "{}/storage-pools/{pool}/volumes/custom{}",
                    self.api_url,
                    project_query(project)
                ))
                .header("Content-Type", "application/octet-stream")
                .header("X-Incus-name", name)
                .header("X-Incus-type", import_type)
                .body(archive),
        )
        .await
    }

    /// Deletes a custom storage volume.
    pub async fn delete_storage_volume(
        &self,
        pool: &str,
        project: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/volumes/custom/{name}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn rebuild_storage_volume(
        &self,
        pool: &str,
        project: &str,
        name: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{name}/rebuild{}",
                project_query(project)
            ),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn storage_volume_backups(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
    ) -> Result<Vec<BackupSummary>, IncusError> {
        self.get(&append_query(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/backups{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_storage_volume_backup(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        name: &str,
        volume_only: bool,
        optimized_storage: bool,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/backups{}",
                project_query(project)
            ),
            &StorageVolumeBackupRequest {
                name,
                volume_only,
                optimized_storage,
            },
        )
        .await
    }

    pub async fn rename_storage_volume_backup(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        backup: &str,
        name: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/backups/{backup}{}",
                project_query(project)
            ),
            &SnapshotRenameRequest { name },
        )
        .await
    }

    pub async fn delete_storage_volume_backup(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        backup: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/backups/{backup}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn export_storage_volume_backup(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        backup: &str,
    ) -> Result<Vec<u8>, IncusError> {
        self.get_bytes(&format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/backups/{backup}/export{}",
            project_query(project)
        ))
        .await
    }

    /// Updates a custom volume's description and driver configuration.
    pub async fn update_storage_volume(
        &self,
        spec: &StorageVolumeUpdateSpec,
    ) -> Result<Value, IncusError> {
        let mut config = spec.config.clone();
        if spec.size.trim().is_empty() {
            config.remove("size");
        } else {
            config.insert("size".to_owned(), spec.size.trim().to_owned());
        }
        self.put(
            &format!(
                "/storage-pools/{}/volumes/custom/{}{}",
                spec.pool,
                spec.name,
                project_query(&spec.project)
            ),
            &StorageVolumeUpdateRequest {
                description: spec.description.trim(),
                config,
            },
        )
        .await
    }

    pub async fn storage_volume_snapshots(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
    ) -> Result<Vec<StorageVolumeSnapshotSummary>, IncusError> {
        self.get(&append_query(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/snapshots{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_storage_volume_snapshot(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        name: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/snapshots{}",
                project_query(project)
            ),
            &StorageVolumeSnapshotRequest { name: name.trim() },
        )
        .await
    }

    pub async fn rename_storage_volume_snapshot(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        snapshot: &str,
        new_name: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}/snapshots/{snapshot}{}",
                project_query(project)
            ),
            &StorageVolumeSnapshotRequest {
                name: new_name.trim(),
            },
        )
        .await
    }

    pub async fn restore_storage_volume_snapshot(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        snapshot: &str,
    ) -> Result<Operation, IncusError> {
        self.put(
            &format!(
                "/storage-pools/{pool}/volumes/custom/{volume}{}",
                project_query(project)
            ),
            &StorageVolumeRestoreRequest { restore: snapshot },
        )
        .await
    }

    pub async fn delete_storage_volume_snapshot(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        snapshot: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/snapshots/{snapshot}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn storage_buckets(
        &self,
        pool: &str,
    ) -> Result<Vec<StorageBucketSummary>, IncusError> {
        self.get(&format!(
            "/storage-pools/{pool}/buckets?recursion=1&all-projects=true"
        ))
        .await
    }

    pub async fn create_storage_bucket(
        &self,
        pool: &str,
        project: &str,
        name: &str,
        description: &str,
        size: &str,
    ) -> Result<Value, IncusError> {
        let mut config = BTreeMap::new();
        if !size.trim().is_empty() {
            config.insert("size".to_owned(), size.trim().to_owned());
        }
        self.post(
            &format!("/storage-pools/{pool}/buckets{}", project_query(project)),
            &StorageBucketRequest {
                name: Some(name.trim()),
                description: description.trim(),
                config,
            },
        )
        .await
    }

    pub async fn update_storage_bucket(
        &self,
        pool: &str,
        project: &str,
        name: &str,
        description: &str,
        mut config: BTreeMap<String, String>,
        size: &str,
    ) -> Result<Value, IncusError> {
        if size.trim().is_empty() {
            config.remove("size");
        } else {
            config.insert("size".to_owned(), size.trim().to_owned());
        }
        self.put(
            &format!(
                "/storage-pools/{pool}/buckets/{name}{}",
                project_query(project)
            ),
            &StorageBucketRequest {
                name: None,
                description: description.trim(),
                config,
            },
        )
        .await
    }

    pub async fn delete_storage_bucket(
        &self,
        pool: &str,
        project: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/buckets/{name}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn storage_bucket_keys(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
    ) -> Result<Vec<StorageBucketKeySummary>, IncusError> {
        self.get(&append_query(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/keys{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn storage_bucket_backups(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
    ) -> Result<Vec<BackupSummary>, IncusError> {
        self.get(&append_query(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/backups{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_storage_bucket_backup(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        name: &str,
        compression: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/backups{}",
                project_query(project)
            ),
            &StorageBucketBackupRequest {
                name: Some(name),
                compression_algorithm: Some(compression),
            },
        )
        .await
    }

    pub async fn rename_storage_bucket_backup(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        backup: &str,
        name: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/backups/{backup}{}",
                project_query(project)
            ),
            &StorageBucketBackupRequest {
                name: Some(name),
                compression_algorithm: None,
            },
        )
        .await
    }

    pub async fn delete_storage_bucket_backup(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        backup: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/buckets/{bucket}/backups/{backup}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn export_storage_bucket_backup(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        backup: &str,
    ) -> Result<Vec<u8>, IncusError> {
        self.get_bytes(&format!(
            "/storage-pools/{pool}/buckets/{bucket}/backups/{backup}/export{}",
            project_query(project)
        ))
        .await
    }

    pub async fn import_storage_bucket(
        &self,
        pool: &str,
        project: &str,
        name: &str,
        archive: Vec<u8>,
    ) -> Result<Operation, IncusError> {
        self.send(
            self.http
                .post(format!(
                    "{}/storage-pools/{pool}/buckets{}",
                    self.api_url,
                    project_query(project)
                ))
                .header("Content-Type", "application/octet-stream")
                .header("X-Incus-name", name)
                .body(archive),
        )
        .await
    }

    pub async fn create_storage_bucket_key(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        name: &str,
        description: &str,
        role: &str,
    ) -> Result<StorageBucketKeySummary, IncusError> {
        self.post(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/keys{}",
                project_query(project)
            ),
            &StorageBucketKeyRequest {
                name: name.trim(),
                description: description.trim(),
                role,
            },
        )
        .await
    }

    pub async fn delete_storage_bucket_key(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/storage-pools/{pool}/buckets/{bucket}/keys/{name}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn update_storage_bucket_key(
        &self,
        pool: &str,
        project: &str,
        bucket: &str,
        name: &str,
        description: &str,
        role: &str,
    ) -> Result<StorageBucketKeySummary, IncusError> {
        self.put(
            &format!(
                "/storage-pools/{pool}/buckets/{bucket}/keys/{name}{}",
                project_query(project)
            ),
            &StorageBucketKeyRequest {
                name,
                description: description.trim(),
                role,
            },
        )
        .await
    }

    /// Lists networks across projects.
    pub async fn networks(&self) -> Result<Vec<NetworkSummary>, IncusError> {
        self.get("/networks?recursion=1&all-projects=true").await
    }

    /// Lists addresses and MACs allocated by instances and managed network services.
    pub async fn network_allocations(&self) -> Result<Vec<Value>, IncusError> {
        self.get("/network-allocations?all-projects=true").await
    }

    pub async fn network_integrations(&self) -> Result<Vec<NetworkIntegrationSummary>, IncusError> {
        self.get("/network-integrations?recursion=1").await
    }

    pub async fn create_network_integration(
        &self,
        name: &str,
        integration_type: &str,
        description: &str,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.post(
            "/network-integrations",
            &NetworkIntegrationRequest {
                name: Some(name),
                integration_type: Some(integration_type),
                description,
                config,
            },
        )
        .await
    }

    pub async fn update_network_integration(
        &self,
        name: &str,
        description: &str,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/network-integrations/{name}"),
            &NetworkIntegrationRequest {
                name: None,
                integration_type: None,
                description,
                config,
            },
        )
        .await
    }

    pub async fn delete_network_integration(&self, name: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/network-integrations/{name}")).await
    }

    pub async fn rename_network_integration(
        &self,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-integrations/{name}"),
            &json!({ "name": new_name }),
        )
        .await
    }

    pub async fn network_leases(
        &self,
        network: &str,
        project: &str,
    ) -> Result<Vec<NetworkLease>, IncusError> {
        self.get(&format!(
            "/networks/{network}/leases{}",
            project_query(project)
        ))
        .await
    }

    pub async fn network_state(&self, network: &str, project: &str) -> Result<Value, IncusError> {
        self.get(&format!(
            "/networks/{network}/state{}",
            project_query(project)
        ))
        .await
    }

    pub async fn network_forwards(
        &self,
        network: &str,
        project: &str,
    ) -> Result<Vec<NetworkForwardSummary>, IncusError> {
        self.get(&append_query(
            &format!("/networks/{network}/forwards{}", project_query(project)),
            "recursion=1",
        ))
        .await
    }

    pub async fn network_acls(&self, project: &str) -> Result<Vec<NetworkAclSummary>, IncusError> {
        self.get(&append_query(
            &format!("/network-acls{}", project_query(project)),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_network_forward(
        &self,
        network: &str,
        project: &str,
        listen_address: &str,
        description: &str,
        port: NetworkForwardPort,
    ) -> Result<Value, IncusError> {
        let ports = if port.listen_port.is_empty() && port.target_address.is_empty() {
            Vec::new()
        } else {
            vec![port]
        };
        self.post(
            &format!("/networks/{network}/forwards{}", project_query(project)),
            &NetworkForwardCreateRequest {
                listen_address,
                description,
                ports,
            },
        )
        .await
    }

    pub async fn delete_network_forward(
        &self,
        network: &str,
        project: &str,
        listen_address: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/networks/{network}/forwards/{listen_address}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn update_network_forward(
        &self,
        spec: &NetworkForwardUpdateSpec,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!(
                "/networks/{}/forwards/{}{}",
                spec.network,
                spec.listen_address,
                project_query(&spec.project)
            ),
            &NetworkForwardUpdateRequest {
                description: &spec.description,
                ports: spec.ports.clone(),
                config: spec.config.clone(),
            },
        )
        .await
    }

    pub async fn create_network_acl(
        &self,
        project: &str,
        name: &str,
        description: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-acls{}", project_query(project)),
            &NetworkAclCreateRequest {
                name,
                description,
                ingress: Vec::new(),
                egress: Vec::new(),
            },
        )
        .await
    }

    pub async fn delete_network_acl(&self, project: &str, name: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/network-acls/{name}{}", project_query(project)))
            .await
    }

    pub async fn rename_network_acl(
        &self,
        project: &str,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-acls/{name}{}", project_query(project)),
            &json!({ "name": new_name }),
        )
        .await
    }

    pub async fn network_acl_log(&self, project: &str, name: &str) -> Result<String, IncusError> {
        self.get_text(&format!(
            "/network-acls/{name}/log{}",
            project_query(project)
        ))
        .await
    }

    pub async fn update_network_acl(
        &self,
        project: &str,
        name: &str,
        description: &str,
        ingress: Vec<NetworkAclRule>,
        egress: Vec<NetworkAclRule>,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/network-acls/{name}{}", project_query(project)),
            &NetworkAclUpdateRequest {
                description,
                ingress,
                egress,
                config,
            },
        )
        .await
    }

    pub async fn network_address_sets(&self) -> Result<Vec<NetworkAddressSetSummary>, IncusError> {
        self.get("/network-address-sets?recursion=1&all-projects=true")
            .await
    }
    pub async fn create_network_address_set(
        &self,
        project: &str,
        name: &str,
        description: &str,
        addresses: Vec<String>,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-address-sets{}", project_query(project)),
            &NetworkAddressSetRequest {
                name: Some(name),
                description,
                addresses,
                config: BTreeMap::new(),
            },
        )
        .await
    }
    pub async fn update_network_address_set(
        &self,
        project: &str,
        name: &str,
        description: &str,
        addresses: Vec<String>,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/network-address-sets/{name}{}", project_query(project)),
            &NetworkAddressSetRequest {
                name: None,
                description,
                addresses,
                config,
            },
        )
        .await
    }
    pub async fn delete_network_address_set(
        &self,
        project: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/network-address-sets/{name}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn rename_network_address_set(
        &self,
        project: &str,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-address-sets/{name}{}", project_query(project)),
            &json!({ "name": new_name }),
        )
        .await
    }

    pub async fn network_load_balancers(
        &self,
        network: &str,
        project: &str,
    ) -> Result<Vec<NetworkLoadBalancerSummary>, IncusError> {
        self.get(&append_query(
            &format!(
                "/networks/{network}/load-balancers{}",
                project_query(project)
            ),
            "recursion=1",
        ))
        .await
    }

    pub async fn network_load_balancer_state(
        &self,
        network: &str,
        project: &str,
        address: &str,
    ) -> Result<Value, IncusError> {
        self.get(&format!(
            "/networks/{network}/load-balancers/{address}/state{}",
            project_query(project)
        ))
        .await
    }

    pub async fn create_network_load_balancer(
        &self,
        spec: &NetworkLoadBalancerSpec,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!(
                "/networks/{}/load-balancers{}",
                spec.network,
                project_query(&spec.project)
            ),
            &NetworkLoadBalancerCreateRequest {
                listen_address: &spec.listen_address,
                description: &spec.description,
                backends: vec![spec.backend.clone()],
                ports: vec![spec.port.clone()],
                config: spec.config.clone(),
            },
        )
        .await
    }

    pub async fn delete_network_load_balancer(
        &self,
        network: &str,
        project: &str,
        listen_address: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/networks/{network}/load-balancers/{listen_address}{}",
            project_query(project)
        ))
        .await
    }
    pub async fn update_network_load_balancer(
        &self,
        spec: &NetworkLoadBalancerUpdateSpec,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!(
                "/networks/{}/load-balancers/{}{}",
                spec.network,
                spec.listen_address,
                project_query(&spec.project)
            ),
            &NetworkLoadBalancerUpdateRequest {
                description: &spec.description,
                backends: spec.backends.clone(),
                ports: spec.ports.clone(),
                config: spec.config.clone(),
            },
        )
        .await
    }

    pub async fn network_peers(
        &self,
        network: &str,
        project: &str,
    ) -> Result<Vec<NetworkPeerSummary>, IncusError> {
        self.get(&append_query(
            &format!("/networks/{network}/peers{}", project_query(project)),
            "recursion=1",
        ))
        .await
    }

    pub async fn create_network_peer(&self, spec: &NetworkPeerSpec) -> Result<Value, IncusError> {
        self.post(
            &format!(
                "/networks/{}/peers{}",
                spec.network,
                project_query(&spec.project)
            ),
            &NetworkPeerCreateRequest {
                name: &spec.name,
                description: &spec.description,
                peer_type: &spec.peer_type,
                target_project: &spec.target_project,
                target_network: &spec.target_network,
                target_integration: &spec.target_integration,
            },
        )
        .await
    }

    pub async fn delete_network_peer(
        &self,
        network: &str,
        project: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/networks/{network}/peers/{name}{}",
            project_query(project)
        ))
        .await
    }
    pub async fn update_network_peer(
        &self,
        spec: &NetworkPeerUpdateSpec,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!(
                "/networks/{}/peers/{}{}",
                spec.network,
                spec.name,
                project_query(&spec.project)
            ),
            &NetworkPeerUpdateRequest {
                description: &spec.description,
                peer_type: &spec.peer_type,
                target_project: &spec.target_project,
                target_network: &spec.target_network,
                target_integration: &spec.target_integration,
                config: spec.config.clone(),
            },
        )
        .await
    }

    pub async fn network_zones(&self) -> Result<Vec<NetworkZoneSummary>, IncusError> {
        self.get("/network-zones?recursion=1&all-projects=true")
            .await
    }
    pub async fn create_network_zone(
        &self,
        project: &str,
        name: &str,
        description: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-zones{}", project_query(project)),
            &NetworkZoneRequest {
                name: Some(name),
                description,
                config: BTreeMap::new(),
            },
        )
        .await
    }
    pub async fn update_network_zone(
        &self,
        project: &str,
        name: &str,
        description: &str,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/network-zones/{name}{}", project_query(project)),
            &NetworkZoneRequest {
                name: None,
                description,
                config,
            },
        )
        .await
    }
    pub async fn delete_network_zone(
        &self,
        project: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!("/network-zones/{name}{}", project_query(project)))
            .await
    }

    pub async fn rename_network_zone(
        &self,
        project: &str,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-zones/{name}{}", project_query(project)),
            &json!({ "name": new_name }),
        )
        .await
    }
    pub async fn network_zone_records(
        &self,
        project: &str,
        zone: &str,
    ) -> Result<Vec<NetworkZoneRecordSummary>, IncusError> {
        self.get(&append_query(
            &format!("/network-zones/{zone}/records{}", project_query(project)),
            "recursion=1",
        ))
        .await
    }
    pub async fn create_network_zone_record(
        &self,
        project: &str,
        zone: &str,
        name: &str,
        description: &str,
        entry: NetworkZoneRecordEntry,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/network-zones/{zone}/records{}", project_query(project)),
            &NetworkZoneRecordRequest {
                name: Some(name),
                description,
                entries: vec![entry],
            },
        )
        .await
    }
    pub async fn update_network_zone_record(
        &self,
        project: &str,
        zone: &str,
        name: &str,
        description: &str,
        entries: Vec<NetworkZoneRecordEntry>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!(
                "/network-zones/{zone}/records/{name}{}",
                project_query(project)
            ),
            &NetworkZoneRecordRequest {
                name: None,
                description,
                entries,
            },
        )
        .await
    }
    pub async fn delete_network_zone_record(
        &self,
        project: &str,
        zone: &str,
        name: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/network-zones/{zone}/records/{name}{}",
            project_query(project)
        ))
        .await
    }

    /// Lists local cached images across projects.
    pub async fn images(&self) -> Result<Vec<ImageSummary>, IncusError> {
        self.get("/images?recursion=1&all-projects=true").await
    }

    pub async fn update_image(&self, spec: &ImageUpdateSpec) -> Result<Value, IncusError> {
        self.put(
            &format!(
                "/images/{}{}",
                spec.fingerprint,
                project_query(&spec.project)
            ),
            &ImageUpdateRequest {
                public: spec.public,
                auto_update: spec.auto_update,
                properties: spec.properties.clone(),
            },
        )
        .await
    }

    pub async fn export_image(
        &self,
        fingerprint: &str,
        project: &str,
    ) -> Result<Vec<u8>, IncusError> {
        let response = self
            .http
            .get(format!(
                "{}/images/{fingerprint}/export{}",
                self.api_url,
                project_query(project)
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        Ok(response
            .bytes()
            .await
            .map_err(IncusError::Request)?
            .to_vec())
    }

    /// Pushes a local image to an Incus image-receive endpoint.
    #[allow(clippy::too_many_arguments)]
    pub async fn push_image(
        &self,
        fingerprint: &str,
        project: &str,
        target: &str,
        target_project: &str,
        secret: &str,
        certificate: &str,
        aliases: Vec<String>,
        profiles: Vec<String>,
    ) -> Result<Operation, IncusError> {
        let aliases = aliases
            .into_iter()
            .map(|name| json!({ "name": name, "description": "" }))
            .collect::<Vec<_>>();
        self.post(
            &format!("/images/{fingerprint}/export{}", project_query(project)),
            &json!({
                "target": target.trim(),
                "project": target_project.trim(),
                "secret": secret.trim(),
                "certificate": certificate,
                "aliases": aliases,
                "profiles": profiles,
            }),
        )
        .await
    }

    pub async fn refresh_image(
        &self,
        fingerprint: &str,
        project: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!("/images/{fingerprint}/refresh{}", project_query(project)),
            &json!({}),
        )
        .await
    }

    pub async fn import_image(
        &self,
        project: &str,
        filename: &str,
        public: bool,
        aliases: &[String],
        archive: Vec<u8>,
    ) -> Result<Operation, IncusError> {
        let mut request = self
            .http
            .post(format!("{}/images{}", self.api_url, project_query(project)))
            .header("Content-Type", "application/octet-stream")
            .header("X-Incus-filename", filename)
            .header("X-Incus-public", public.to_string())
            .body(archive);
        if !aliases.is_empty() {
            let mut values = url::form_urlencoded::Serializer::new(String::new());
            for alias in aliases {
                values.append_pair("alias", alias);
            }
            request = request.header("X-Incus-aliases", values.finish());
        }
        self.send(request).await
    }

    pub async fn publish_instance_image(
        &self,
        spec: &ImagePublishSpec,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!("/images{}", project_query(&spec.project)),
            &ImagePublishRequest {
                source: ImagePublishSource {
                    source_type: "instance",
                    name: &spec.instance,
                },
                aliases: if spec.alias.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![ImagePublishAlias {
                        name: &spec.alias,
                        description: &spec.description,
                    }]
                },
                public: spec.public,
            },
        )
        .await
    }

    /// Adds an alias to a local image in a project.
    pub async fn create_image_alias(
        &self,
        alias: &str,
        fingerprint: &str,
        project: &str,
        description: &str,
    ) -> Result<Value, IncusError> {
        if alias.trim().is_empty() {
            return Err(IncusError::InvalidResource(
                "image alias name is required".to_owned(),
            ));
        }
        let request = ImageAliasRequest {
            name: alias.trim(),
            target: fingerprint,
            description: description.trim(),
        };
        self.post(
            &format!("/images/aliases{}", project_query(project)),
            &request,
        )
        .await
    }

    /// Deletes an image alias from a project.
    pub async fn delete_image_alias(
        &self,
        alias: &str,
        project: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "/images/aliases/{alias}{}",
            project_query(project)
        ))
        .await
    }

    pub async fn update_image_alias(
        &self,
        alias: &str,
        project: &str,
        new_name: &str,
        fingerprint: &str,
        description: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/images/aliases/{alias}{}", project_query(project)),
            &ImageAliasRequest {
                name: new_name,
                target: fingerprint,
                description,
            },
        )
        .await
    }

    /// Lists profiles across projects.
    pub async fn profiles(&self) -> Result<Vec<ProfileSummary>, IncusError> {
        self.get("/profiles?recursion=1&all-projects=true").await
    }

    /// Lists asynchronous operations, grouped by Incus lifecycle category.
    pub async fn operations(&self) -> Result<OperationGroups, IncusError> {
        self.get("/operations?recursion=1").await
    }

    /// Lists current and acknowledged server warnings.
    pub async fn warnings(&self) -> Result<Vec<WarningSummary>, IncusError> {
        self.get("/warnings?recursion=1").await
    }

    /// Acknowledges a server warning.
    pub async fn acknowledge_warning(&self, uuid: &str) -> Result<Value, IncusError> {
        self.put(
            &format!("/warnings/{uuid}"),
            &WarningStatusRequest {
                status: "acknowledged",
            },
        )
        .await
    }

    /// Permanently removes a warning record.
    pub async fn delete_warning(&self, uuid: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/warnings/{uuid}")).await
    }

    /// Lists trusted client certificates.
    pub async fn certificates(&self) -> Result<Vec<CertificateSummary>, IncusError> {
        self.get("/certificates?recursion=1").await
    }

    /// Removes a trusted client certificate.
    pub async fn delete_certificate(&self, fingerprint: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/certificates/{fingerprint}")).await
    }

    /// Creates a single-use trust token for a future client certificate.
    pub async fn generate_certificate_token(
        &self,
        name: &str,
        restricted: bool,
        projects: Vec<String>,
    ) -> Result<Operation, IncusError> {
        self.post(
            "/certificates",
            &CertificateCreateRequest {
                name,
                certificate_type: "client",
                restricted,
                projects,
                certificate: None,
                token: true,
            },
        )
        .await
    }

    /// Adds a PEM-encoded client certificate directly to the trust store.
    pub async fn add_certificate(
        &self,
        name: &str,
        restricted: bool,
        projects: Vec<String>,
        certificate: String,
    ) -> Result<Value, IncusError> {
        self.post(
            "/certificates",
            &CertificateCreateRequest {
                name,
                certificate_type: "client",
                restricted,
                projects,
                certificate: Some(certificate),
                token: false,
            },
        )
        .await
    }

    pub async fn update_certificate(
        &self,
        fingerprint: &str,
        name: &str,
        restricted: bool,
        projects: Vec<String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/certificates/{fingerprint}"),
            &CertificateUpdateRequest {
                name,
                certificate_type: "client",
                restricted,
                projects,
            },
        )
        .await
    }

    /// Lists cluster members. Call only when the server reports clustered mode.
    pub async fn cluster_members(&self) -> Result<Vec<ClusterMemberSummary>, IncusError> {
        self.get("/cluster/members?recursion=1").await
    }

    pub async fn cluster_configuration(&self) -> Result<Value, IncusError> {
        self.get("/cluster").await
    }

    pub async fn configure_cluster(&self, value: &Value) -> Result<Value, IncusError> {
        self.put("/cluster", value).await
    }

    pub async fn update_cluster_certificate(
        &self,
        certificate: &str,
        key: &str,
    ) -> Result<Value, IncusError> {
        self.put(
            "/cluster/certificate",
            &json!({
                "cluster_certificate": certificate,
                "cluster_certificate_key": key,
            }),
        )
        .await
    }

    pub async fn generate_cluster_join_token(
        &self,
        server_name: &str,
    ) -> Result<Operation, IncusError> {
        self.post("/cluster/members", &ClusterJoinTokenRequest { server_name })
            .await
    }

    pub async fn cluster_groups(&self) -> Result<Vec<ClusterGroupSummary>, IncusError> {
        self.get("/cluster/groups?recursion=1").await
    }

    pub async fn create_cluster_group(
        &self,
        name: &str,
        description: &str,
        members: Vec<String>,
    ) -> Result<Value, IncusError> {
        self.post(
            "/cluster/groups",
            &ClusterGroupRequest {
                name: Some(name),
                description,
                members,
            },
        )
        .await
    }

    pub async fn update_cluster_group(
        &self,
        name: &str,
        description: &str,
        members: Vec<String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/cluster/groups/{name}"),
            &ClusterGroupRequest {
                name: None,
                description,
                members,
            },
        )
        .await
    }

    pub async fn delete_cluster_group(&self, name: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/cluster/groups/{name}")).await
    }

    pub async fn rename_cluster_group(
        &self,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/cluster/groups/{name}"),
            &json!({ "name": new_name }),
        )
        .await
    }

    pub async fn update_cluster_member(
        &self,
        name: &str,
        description: &str,
        roles: Vec<String>,
        groups: Vec<String>,
        failure_domain: &str,
        config: BTreeMap<String, String>,
    ) -> Result<Value, IncusError> {
        self.put(
            &format!("/cluster/members/{name}"),
            &ClusterMemberUpdateRequest {
                description,
                roles,
                groups,
                failure_domain,
                config,
            },
        )
        .await
    }

    pub async fn rename_cluster_member(
        &self,
        name: &str,
        new_name: &str,
    ) -> Result<Value, IncusError> {
        self.post(
            &format!("/cluster/members/{name}"),
            &json!({ "server_name": new_name }),
        )
        .await
    }

    pub async fn cluster_member_state(&self, name: &str) -> Result<Value, IncusError> {
        self.get(&format!("/cluster/members/{name}/state")).await
    }

    pub async fn change_cluster_member_state(
        &self,
        name: &str,
        action: &str,
        mode: &str,
    ) -> Result<Operation, IncusError> {
        self.post(
            &format!("/cluster/members/{name}/state"),
            &ClusterMemberStateRequest { action, mode },
        )
        .await
    }

    pub async fn delete_cluster_member(&self, name: &str) -> Result<Value, IncusError> {
        self.delete(&format!("/cluster/members/{name}")).await
    }

    /// Retrieves a full mutable object and its ETag for safe configuration edits.
    pub async fn resource_definition(
        &self,
        kind: ResourceKind,
        name: &str,
        project: &str,
    ) -> Result<ResourceDefinition, IncusError> {
        let path = resource_path(kind, name, project)?;
        let response = self
            .http
            .get(format!("{}{}", self.api_url, path))
            .send()
            .await
            .map_err(IncusError::Request)?;
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let value: Value = decode_response(response).await?;
        let value = if kind == ResourceKind::Server {
            value
                .get("config")
                .cloned()
                .ok_or(IncusError::MissingMetadata)?
        } else {
            value
        };
        Ok(ResourceDefinition {
            kind,
            name: name.to_owned(),
            project: project.to_owned(),
            value,
            etag,
        })
    }

    /// Replaces a full Incus resource definition guarded by the revision read earlier.
    pub async fn update_resource(
        &self,
        definition: &ResourceDefinition,
        value: &Value,
    ) -> Result<Value, IncusError> {
        let etag = definition.etag.as_deref().ok_or(IncusError::MissingEtag)?;
        let path = resource_path(definition.kind, &definition.name, &definition.project)?;
        let body = if definition.kind == ResourceKind::Server {
            json!({ "config": value })
        } else {
            value.clone()
        };
        self.send(
            self.http
                .put(format!("{}{}", self.api_url, path))
                .header(IF_MATCH, etag)
                .json(&body),
        )
        .await
    }

    /// Creates a top-level mutable resource from a complete Incus definition.
    pub async fn create_resource(
        &self,
        kind: ResourceKind,
        project: &str,
        value: &Value,
    ) -> Result<Value, IncusError> {
        let path = resource_collection_path(kind, project)?;
        self.post(&path, value).await
    }

    /// Deletes a named top-level resource.
    pub async fn delete_resource(
        &self,
        kind: ResourceKind,
        name: &str,
        project: &str,
    ) -> Result<Value, IncusError> {
        let path = resource_path(kind, name, project)?;
        self.delete(&path).await
    }

    /// Applies an instance lifecycle state change and returns Incus's async operation.
    pub async fn change_instance_state(
        &self,
        name: &str,
        project: &str,
        action: InstanceAction,
    ) -> Result<Operation, IncusError> {
        debug_assert!(action != InstanceAction::Delete);
        let request = InstanceStateRequest {
            action: action.as_str(),
            timeout: -1,
            force: false,
            stateful: false,
        };
        self.put(&instance_subpath(name, project, "state"), &request)
            .await
    }

    /// Deletes a stopped instance and returns Incus's async operation.
    pub async fn delete_instance(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&instance_path(name, project)).await
    }

    /// Reinitializes an instance root disk while preserving its configuration.
    pub async fn rebuild_instance(
        &self,
        name: &str,
        project: &str,
        image: Option<&str>,
    ) -> Result<Operation, IncusError> {
        let source = match image.map(str::trim).filter(|value| !value.is_empty()) {
            Some(alias) => json!({
                "type": "image",
                "alias": alias,
                "mode": "pull",
                "protocol": "simplestreams",
                "server": "https://images.linuxcontainers.org"
            }),
            None => json!({ "type": "none" }),
        };
        self.post(
            &instance_subpath(name, project, "rebuild"),
            &json!({ "source": source }),
        )
        .await
    }

    /// Renames an instance and returns the asynchronous operation.
    pub async fn rename_instance(
        &self,
        name: &str,
        project: &str,
        new_name: &str,
    ) -> Result<Operation, IncusError> {
        if new_name.trim().is_empty() {
            return Err(IncusError::InvalidResource(
                "new instance name is required".to_owned(),
            ));
        }
        self.post(
            &instance_path(name, project),
            &RenameInstanceRequest {
                name: new_name.trim(),
            },
        )
        .await
    }

    pub async fn move_instance(&self, spec: &MoveInstanceSpec) -> Result<Operation, IncusError> {
        self.post(
            &instance_path(&spec.name, &spec.project),
            &MoveInstanceRequest {
                migration: true,
                name: &spec.new_name,
                project: &spec.target_project,
                target: &spec.target_member,
                pool: &spec.target_pool,
                live: spec.live,
            },
        )
        .await
    }

    /// Creates an instance from a local image or an `images:` simplestreams alias.
    pub async fn create_instance(
        &self,
        spec: &CreateInstanceSpec,
    ) -> Result<Operation, IncusError> {
        if spec.name.trim().is_empty() {
            return Err(IncusError::InvalidCreate(
                "instance name is required".to_owned(),
            ));
        }
        if spec.image.trim().is_empty() {
            return Err(IncusError::InvalidCreate("image is required".to_owned()));
        }
        if !matches!(spec.instance_type.as_str(), "container" | "virtual-machine") {
            return Err(IncusError::InvalidCreate(format!(
                "unsupported instance type: {}",
                spec.instance_type
            )));
        }

        let request = CreateInstanceRequest::from_spec(spec);
        let path = create_instance_path(spec);
        self.post(&path, &request).await
    }

    /// Runs a non-interactive shell command and captures stdout/stderr on completion.
    pub async fn execute_command(
        &self,
        name: &str,
        project: &str,
        command: &str,
    ) -> Result<ExecResult, IncusError> {
        if command.trim().is_empty() {
            return Err(IncusError::InvalidExec("command is required".to_owned()));
        }
        let request = ExecRequest {
            command: vec!["/bin/sh".to_owned(), "-lc".to_owned(), command.to_owned()],
            wait_for_websocket: false,
            interactive: false,
            record_output: true,
        };
        let operation: Operation = self
            .post(&instance_subpath(name, project, "exec"), &request)
            .await?;
        if operation.id.is_empty() {
            return Err(IncusError::MissingOperationId);
        }

        let operation = self.wait_operation(&operation.id, 30).await?;
        let stdout = self.operation_output(&operation, "1", project).await?;
        let stderr = self.operation_output(&operation, "2", project).await?;
        Ok(ExecResult {
            exit_status: operation
                .metadata
                .get("return")
                .and_then(serde_json::Value::as_i64),
            operation,
            stdout,
            stderr,
        })
    }

    pub async fn interactive_exec(
        &self,
        name: &str,
        project: &str,
        shell: &str,
    ) -> Result<Operation, IncusError> {
        let command = if shell.trim().is_empty() {
            "/bin/sh"
        } else {
            shell.trim()
        };
        let request = ExecRequest {
            command: vec![command.to_owned()],
            wait_for_websocket: true,
            interactive: true,
            record_output: false,
        };
        let operation: Operation = self
            .post(&instance_subpath(name, project, "exec"), &request)
            .await?;
        self.attach_operation_terminal(&operation).await?;
        Ok(operation)
    }

    pub async fn interactive_console(
        &self,
        name: &str,
        project: &str,
        force: bool,
    ) -> Result<Operation, IncusError> {
        let operation: Operation = self
            .post(
                &instance_subpath(name, project, "console"),
                &ConsoleRequest {
                    console_type: "console",
                    width: 0,
                    height: 0,
                    force,
                },
            )
            .await?;
        self.attach_operation_terminal(&operation).await?;
        Ok(operation)
    }

    pub async fn vga_console(
        &self,
        name: &str,
        project: &str,
        force: bool,
    ) -> Result<Operation, IncusError> {
        let operation: Operation = self
            .post(
                &instance_subpath(name, project, "console"),
                &ConsoleRequest {
                    console_type: "vga",
                    width: 0,
                    height: 0,
                    force,
                },
            )
            .await?;
        let path = self.operation_websocket_path(&operation)?;
        events::vga_session(&self.transport, &path)
            .await
            .map_err(|error| IncusError::Interactive(error.to_string()))?;
        Ok(operation)
    }

    pub async fn start_instance_protocol_proxy(
        &self,
        name: &str,
        project: &str,
        protocol: &str,
        port: u16,
    ) -> Result<std::net::SocketAddr, IncusError> {
        if !matches!(protocol, "sftp" | "nbd") {
            return Err(IncusError::Interactive(format!(
                "unsupported proxy protocol: {protocol}"
            )));
        }
        events::start_binary_proxy(
            self.transport.clone(),
            instance_subpath(name, project, protocol),
            port,
        )
        .await
        .map_err(|error| IncusError::Interactive(error.to_string()))
    }

    pub async fn start_storage_volume_protocol_proxy(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        protocol: &str,
        port: u16,
    ) -> Result<std::net::SocketAddr, IncusError> {
        if !matches!(protocol, "sftp" | "nbd") {
            return Err(IncusError::Interactive(format!(
                "unsupported proxy protocol: {protocol}"
            )));
        }
        let path = format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/{protocol}{}",
            project_query(project)
        );
        events::start_binary_proxy(self.transport.clone(), path, port)
            .await
            .map_err(|error| IncusError::Interactive(error.to_string()))
    }

    pub async fn read_storage_volume_file(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        path: &str,
    ) -> Result<String, IncusError> {
        self.get_text(&storage_volume_file_path(pool, project, volume, path))
            .await
    }

    pub async fn list_storage_volume_directory(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        path: &str,
    ) -> Result<Vec<String>, IncusError> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.api_url,
                storage_volume_file_path(pool, project, volume, path)
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        if response
            .headers()
            .get("X-Incus-type")
            .and_then(|value| value.to_str().ok())
            != Some("directory")
        {
            return Err(IncusError::InvalidResource(
                "selected custom-volume path is not a directory".into(),
            ));
        }
        let value: Value =
            serde_json::from_slice(&response.bytes().await.map_err(IncusError::Request)?).map_err(
                |error| IncusError::InvalidResource(format!("invalid directory listing: {error}")),
            )?;
        Ok(value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect())
    }

    pub async fn write_storage_volume_file(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<Value, IncusError> {
        self.send(
            self.http
                .post(format!(
                    "{}{}",
                    self.api_url,
                    storage_volume_file_path(pool, project, volume, path)
                ))
                .header("Content-Type", "application/octet-stream")
                .header("X-Incus-type", "file")
                .body(content),
        )
        .await
    }

    pub async fn delete_storage_volume_file(
        &self,
        pool: &str,
        project: &str,
        volume: &str,
        path: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&storage_volume_file_path(pool, project, volume, path))
            .await
    }

    async fn attach_operation_terminal(&self, operation: &Operation) -> Result<(), IncusError> {
        let path = self.operation_websocket_path(operation)?;
        events::interactive_session(&self.transport, &path)
            .await
            .map_err(|error| IncusError::Interactive(error.to_string()))
    }

    fn operation_websocket_path(&self, operation: &Operation) -> Result<String, IncusError> {
        let secret = operation
            .metadata
            .get("fds")
            .and_then(|fds| fds.get("0"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                IncusError::Interactive(
                    "operation did not return terminal websocket secret".to_owned(),
                )
            })?;
        let encoded: String = byte_serialize(secret.as_bytes()).collect();
        Ok(format!(
            "/1.0/operations/{}/websocket?secret={encoded}",
            operation.id
        ))
    }

    /// Lists snapshots belonging to one instance.
    pub async fn snapshots(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<SnapshotSummary>, IncusError> {
        self.get(&append_query(
            &instance_subpath(name, project, "snapshots"),
            "recursion=1",
        ))
        .await
    }

    /// Creates an instance snapshot as an asynchronous operation.
    pub async fn create_snapshot(
        &self,
        name: &str,
        project: &str,
        snapshot_name: &str,
        stateful: bool,
    ) -> Result<Operation, IncusError> {
        if snapshot_name.trim().is_empty() {
            return Err(IncusError::InvalidSnapshot(
                "snapshot name is required".to_owned(),
            ));
        }
        let request = SnapshotRequest {
            name: snapshot_name.trim(),
            stateful,
        };
        self.post(&instance_subpath(name, project, "snapshots"), &request)
            .await
    }

    /// Deletes a snapshot as an asynchronous operation.
    pub async fn delete_snapshot(
        &self,
        name: &str,
        project: &str,
        snapshot_name: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&instance_subpath(
            name,
            project,
            &format!("snapshots/{snapshot_name}"),
        ))
        .await
    }

    /// Renames an instance snapshot as an asynchronous operation.
    pub async fn rename_snapshot(
        &self,
        name: &str,
        project: &str,
        snapshot_name: &str,
        new_name: &str,
    ) -> Result<Operation, IncusError> {
        if new_name.trim().is_empty() {
            return Err(IncusError::InvalidSnapshot(
                "new snapshot name is required".to_owned(),
            ));
        }
        self.post(
            &instance_subpath(name, project, &format!("snapshots/{snapshot_name}")),
            &SnapshotRenameRequest {
                name: new_name.trim(),
            },
        )
        .await
    }

    /// Restores an instance to a selected snapshot.
    pub async fn restore_snapshot(
        &self,
        name: &str,
        project: &str,
        snapshot_name: &str,
        stateful: bool,
    ) -> Result<Operation, IncusError> {
        let request = RestoreSnapshotRequest {
            restore: snapshot_name,
            stateful,
        };
        self.put(&instance_path(name, project), &request).await
    }

    /// Reads a UTF-8 file from an instance.
    pub async fn read_file(
        &self,
        name: &str,
        project: &str,
        path: &str,
    ) -> Result<String, IncusError> {
        self.get_text(&file_path(name, project, path)).await
    }

    /// Lists the immediate children of a directory inside an instance.
    pub async fn list_directory(
        &self,
        name: &str,
        project: &str,
        path: &str,
    ) -> Result<Vec<String>, IncusError> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.api_url,
                file_path(name, project, path)
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        if response
            .headers()
            .get("X-Incus-type")
            .and_then(|value| value.to_str().ok())
            != Some("directory")
        {
            return Err(IncusError::InvalidResource(format!(
                "{path} is not a directory"
            )));
        }
        let mut entries: Vec<String> = response.json().await.map_err(IncusError::Request)?;
        entries.sort_unstable();
        Ok(entries)
    }

    pub async fn pull_file(
        &self,
        name: &str,
        project: &str,
        path: &str,
    ) -> Result<Vec<u8>, IncusError> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.api_url,
                file_path(name, project, path)
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        Ok(response
            .bytes()
            .await
            .map_err(IncusError::Request)?
            .to_vec())
    }

    pub async fn push_file(
        &self,
        name: &str,
        project: &str,
        path: &str,
        contents: Vec<u8>,
    ) -> Result<(), IncusError> {
        self.send_bytes(
            self.http
                .post(format!(
                    "{}{}",
                    self.api_url,
                    file_path(name, project, path)
                ))
                .header("X-Incus-type", "file")
                .header("X-Incus-write", "overwrite")
                .body(contents),
        )
        .await
    }

    /// Creates or replaces a UTF-8 file in an instance.
    pub async fn write_file(
        &self,
        name: &str,
        project: &str,
        path: &str,
        contents: String,
    ) -> Result<(), IncusError> {
        self.send_bytes(
            self.http
                .post(format!(
                    "{}{}",
                    self.api_url,
                    file_path(name, project, path)
                ))
                .header("X-Incus-type", "file")
                .header("X-Incus-write", "overwrite")
                .body(contents),
        )
        .await
    }

    /// Deletes a file or empty directory from an instance.
    pub async fn delete_file(
        &self,
        name: &str,
        project: &str,
        path: &str,
    ) -> Result<(), IncusError> {
        self.send_bytes(self.http.delete(format!(
            "{}{}",
            self.api_url,
            file_path(name, project, path)
        )))
        .await
    }

    /// Requests cancellation of an operation that Incus marks as cancellable.
    pub async fn cancel_operation(&self, id: &str) -> Result<(), IncusError> {
        if id.is_empty() {
            return Err(IncusError::MissingOperationId);
        }
        self.send_bytes(
            self.http
                .delete(format!("{}/operations/{id}", self.api_url)),
        )
        .await
    }

    /// Lists exportable backups belonging to one instance.
    pub async fn backups(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<BackupSummary>, IncusError> {
        self.get(&append_query(
            &instance_subpath(name, project, "backups"),
            "recursion=1",
        ))
        .await
    }

    /// Creates an exportable instance backup as an asynchronous operation.
    pub async fn create_backup(
        &self,
        name: &str,
        project: &str,
        backup_name: &str,
        instance_only: bool,
        optimized_storage: bool,
    ) -> Result<Operation, IncusError> {
        if backup_name.trim().is_empty() {
            return Err(IncusError::InvalidBackup(
                "backup name is required".to_owned(),
            ));
        }
        let request = BackupRequest {
            name: backup_name.trim(),
            instance_only,
            optimized_storage,
        };
        self.post(&instance_subpath(name, project, "backups"), &request)
            .await
    }

    /// Deletes an exportable instance backup.
    pub async fn delete_backup(
        &self,
        name: &str,
        project: &str,
        backup_name: &str,
    ) -> Result<Operation, IncusError> {
        self.delete(&instance_subpath(
            name,
            project,
            &format!("backups/{backup_name}"),
        ))
        .await
    }

    /// Downloads an instance backup archive without interpreting its binary payload.
    pub async fn export_backup(
        &self,
        name: &str,
        project: &str,
        backup_name: &str,
    ) -> Result<Vec<u8>, IncusError> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.api_url,
                instance_subpath(name, project, &format!("backups/{backup_name}/export"))
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        Ok(response
            .bytes()
            .await
            .map_err(IncusError::Request)?
            .to_vec())
    }

    /// Imports an Incus backup archive into a project.
    pub async fn import_backup(
        &self,
        project: &str,
        archive: Vec<u8>,
    ) -> Result<Operation, IncusError> {
        self.send(
            self.http
                .post(format!(
                    "{}/instances{}",
                    self.api_url,
                    project_query(project)
                ))
                .header("Content-Type", "application/octet-stream")
                .body(archive),
        )
        .await
    }

    async fn get<T>(&self, path: &str) -> Result<T, IncusError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.send(self.http.get(format!("{}{}", self.api_url, path)))
            .await
    }

    async fn put<T, Body>(&self, path: &str, body: &Body) -> Result<T, IncusError>
    where
        T: serde::de::DeserializeOwned,
        Body: serde::Serialize + ?Sized,
    {
        self.send(
            self.http
                .put(format!("{}{}", self.api_url, path))
                .json(body),
        )
        .await
    }

    /// Reads the serial console log retained for an instance.
    pub async fn console_log(&self, name: &str, project: &str) -> Result<String, IncusError> {
        self.get_text(&instance_subpath(name, project, "console"))
            .await
    }

    pub async fn console_screenshot(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<u8>, IncusError> {
        self.get_bytes(&append_query(
            &instance_subpath(name, project, "console"),
            "type=vga",
        ))
        .await
    }

    pub async fn clear_console_log(&self, name: &str, project: &str) -> Result<Value, IncusError> {
        self.delete(&instance_subpath(name, project, "console"))
            .await
    }

    pub async fn instance_logs(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<String>, IncusError> {
        self.get(&instance_subpath(name, project, "logs")).await
    }

    pub async fn instance_log(
        &self,
        name: &str,
        project: &str,
        filename: &str,
    ) -> Result<String, IncusError> {
        self.get_text(&format!(
            "{}/{}",
            instance_subpath(name, project, "logs"),
            encode_path_segment(filename)
        ))
        .await
    }

    pub async fn delete_instance_log(
        &self,
        name: &str,
        project: &str,
        filename: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "{}/{}",
            instance_subpath(name, project, "logs"),
            encode_path_segment(filename)
        ))
        .await
    }

    pub async fn instance_exec_output_logs(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<String>, IncusError> {
        self.get(&instance_subpath(name, project, "logs/exec-output"))
            .await
    }

    pub async fn instance_exec_output_log(
        &self,
        name: &str,
        project: &str,
        filename: &str,
    ) -> Result<String, IncusError> {
        self.get_text(&format!(
            "{}/{}",
            instance_subpath(name, project, "logs/exec-output"),
            encode_path_segment(filename)
        ))
        .await
    }

    pub async fn delete_instance_exec_output_log(
        &self,
        name: &str,
        project: &str,
        filename: &str,
    ) -> Result<Value, IncusError> {
        self.delete(&format!(
            "{}/{}",
            instance_subpath(name, project, "logs/exec-output"),
            encode_path_segment(filename)
        ))
        .await
    }

    pub async fn instance_metadata(
        &self,
        name: &str,
        project: &str,
    ) -> Result<(Value, Option<String>), IncusError> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.api_url,
                instance_subpath(name, project, "metadata")
            ))
            .send()
            .await
            .map_err(IncusError::Request)?;
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let value = decode_response(response).await?;
        Ok((value, etag))
    }

    pub async fn update_instance_metadata(
        &self,
        name: &str,
        project: &str,
        metadata: &Value,
        etag: &str,
    ) -> Result<Value, IncusError> {
        self.send(
            self.http
                .put(format!(
                    "{}{}",
                    self.api_url,
                    instance_subpath(name, project, "metadata")
                ))
                .header(IF_MATCH, etag)
                .json(metadata),
        )
        .await
    }

    pub async fn instance_template_names(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<String>, IncusError> {
        self.get(&instance_subpath(name, project, "metadata/templates"))
            .await
    }

    pub async fn instance_template(
        &self,
        name: &str,
        project: &str,
        template: &str,
    ) -> Result<String, IncusError> {
        let encoded = encode_path_segment(template);
        self.get_text(&append_query(
            &instance_subpath(name, project, "metadata/templates"),
            &format!("path={encoded}"),
        ))
        .await
    }

    pub async fn write_instance_template(
        &self,
        name: &str,
        project: &str,
        template: &str,
        content: String,
    ) -> Result<Value, IncusError> {
        let encoded = encode_path_segment(template);
        self.send(
            self.http
                .post(format!(
                    "{}{}",
                    self.api_url,
                    append_query(
                        &instance_subpath(name, project, "metadata/templates"),
                        &format!("path={encoded}"),
                    )
                ))
                .header("Content-Type", "application/octet-stream")
                .body(content),
        )
        .await
    }

    pub async fn delete_instance_template(
        &self,
        name: &str,
        project: &str,
        template: &str,
    ) -> Result<Value, IncusError> {
        let encoded = encode_path_segment(template);
        self.delete(&append_query(
            &instance_subpath(name, project, "metadata/templates"),
            &format!("path={encoded}"),
        ))
        .await
    }

    pub async fn instance_memory_dump(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Vec<u8>, IncusError> {
        self.get_bytes(&instance_subpath(name, project, "debug/memory"))
            .await
    }

    pub async fn repair_instance(
        &self,
        name: &str,
        project: &str,
    ) -> Result<Operation, IncusError> {
        self.get(&instance_subpath(name, project, "debug/repair"))
            .await
    }

    pub async fn create_instance_bitmaps(
        &self,
        name: &str,
        project: &str,
        spec: &StorageVolumeBitmapSpec,
    ) -> Result<Operation, IncusError> {
        self.post(&instance_subpath(name, project, "bitmaps"), spec)
            .await
    }

    async fn post<T, Body>(&self, path: &str, body: &Body) -> Result<T, IncusError>
    where
        T: serde::de::DeserializeOwned,
        Body: serde::Serialize + ?Sized,
    {
        self.send(
            self.http
                .post(format!("{}{}", self.api_url, path))
                .json(body),
        )
        .await
    }

    async fn wait_operation(
        &self,
        id: &str,
        timeout_seconds: u32,
    ) -> Result<Operation, IncusError> {
        self.get(&format!("/operations/{id}/wait?timeout={timeout_seconds}"))
            .await
    }

    async fn operation_output(
        &self,
        operation: &Operation,
        stream: &str,
        project: &str,
    ) -> Result<String, IncusError> {
        let Some(path) = operation
            .metadata
            .get("output")
            .and_then(|output| output.get(stream))
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(String::new());
        };
        self.get_text(&add_project_to_path(path, project)).await
    }

    async fn delete<T>(&self, path: &str) -> Result<T, IncusError>
    where
        T: serde::de::DeserializeOwned,
    {
        self.send(self.http.delete(format!("{}{}", self.api_url, path)))
            .await
    }

    async fn get_text(&self, path: &str) -> Result<String, IncusError> {
        let response = self
            .http
            .get(format!("{}{}", self.api_url, path))
            .send()
            .await
            .map_err(IncusError::Request)?;
        let status = response.status();
        if !status.is_success() {
            return Err(IncusError::HttpStatus(status));
        }
        response.text().await.map_err(IncusError::Request)
    }

    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>, IncusError> {
        let response = self
            .http
            .get(format!("{}{}", self.api_url, path))
            .send()
            .await
            .map_err(IncusError::Request)?;
        if !response.status().is_success() {
            return Err(IncusError::HttpStatus(response.status()));
        }
        Ok(response
            .bytes()
            .await
            .map_err(IncusError::Request)?
            .to_vec())
    }

    async fn send_bytes(&self, request: reqwest::RequestBuilder) -> Result<(), IncusError> {
        let response = request.send().await.map_err(IncusError::Request)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(IncusError::HttpStatus(response.status()))
        }
    }

    async fn send<T>(&self, request: reqwest::RequestBuilder) -> Result<T, IncusError>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = request.send().await.map_err(IncusError::Request)?;
        decode_response(response).await
    }
}

async fn decode_response<T>(response: reqwest::Response) -> Result<T, IncusError>
where
    T: serde::de::DeserializeOwned,
{
    let http_status = response.status();
    let response = response
        .json::<ApiResponse<T>>()
        .await
        .map_err(IncusError::Request)?;

    if !http_status.is_success() || !response.is_success() {
        return Err(IncusError::Api {
            http_status,
            status_code: response.status_code,
            message: response.error.if_empty_then(&response.status).to_owned(),
        });
    }

    response.metadata.ok_or(IncusError::MissingMetadata)
}

#[derive(serde::Serialize)]
struct InstanceStateRequest<'a> {
    action: &'a str,
    timeout: i32,
    force: bool,
    stateful: bool,
}

#[derive(serde::Serialize)]
struct RenameInstanceRequest<'a> {
    name: &'a str,
}

#[derive(serde::Serialize)]
struct MoveInstanceRequest<'a> {
    migration: bool,
    name: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    project: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    target: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pool: &'a str,
    live: bool,
}

#[derive(serde::Serialize)]
struct CreateInstanceRequest {
    name: String,
    #[serde(rename = "type")]
    instance_type: String,
    source: ImageSource,
    profiles: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    config: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    devices: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "is_false")]
    start: bool,
}

#[derive(serde::Serialize)]
struct ImageSource {
    #[serde(rename = "type")]
    source_type: &'static str,
    alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<&'static str>,
}

#[derive(serde::Serialize)]
struct ExecRequest {
    command: Vec<String>,
    #[serde(rename = "wait-for-websocket")]
    wait_for_websocket: bool,
    interactive: bool,
    #[serde(rename = "record-output")]
    record_output: bool,
}

#[derive(serde::Serialize)]
struct ConsoleRequest<'a> {
    #[serde(rename = "type")]
    console_type: &'a str,
    width: u16,
    height: u16,
    force: bool,
}

#[derive(serde::Serialize)]
struct SnapshotRequest<'a> {
    name: &'a str,
    stateful: bool,
}

#[derive(serde::Serialize)]
struct SnapshotRenameRequest<'a> {
    name: &'a str,
}

#[derive(serde::Serialize)]
struct RestoreSnapshotRequest<'a> {
    restore: &'a str,
    stateful: bool,
}

#[derive(serde::Serialize)]
struct BackupRequest<'a> {
    name: &'a str,
    instance_only: bool,
    optimized_storage: bool,
}

#[derive(serde::Serialize)]
struct StorageVolumeBackupRequest<'a> {
    name: &'a str,
    volume_only: bool,
    optimized_storage: bool,
}

#[derive(serde::Serialize)]
struct StorageBucketBackupRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compression_algorithm: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct ImageAliasRequest<'a> {
    name: &'a str,
    target: &'a str,
    description: &'a str,
}

#[derive(serde::Serialize)]
struct ImageUpdateRequest {
    public: bool,
    auto_update: bool,
    properties: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct ImagePublishRequest<'a> {
    source: ImagePublishSource<'a>,
    aliases: Vec<ImagePublishAlias<'a>>,
    public: bool,
}

#[derive(serde::Serialize)]
struct ImagePublishSource<'a> {
    #[serde(rename = "type")]
    source_type: &'a str,
    name: &'a str,
}

#[derive(serde::Serialize)]
struct ImagePublishAlias<'a> {
    name: &'a str,
    description: &'a str,
}

#[derive(serde::Serialize)]
struct WarningStatusRequest<'a> {
    status: &'a str,
}

#[derive(serde::Serialize)]
struct CertificateCreateRequest<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    certificate_type: &'a str,
    restricted: bool,
    projects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    certificate: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    token: bool,
}

#[derive(serde::Serialize)]
struct CertificateUpdateRequest<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    certificate_type: &'a str,
    restricted: bool,
    projects: Vec<String>,
}

#[derive(serde::Serialize)]
struct ClusterMemberUpdateRequest<'a> {
    description: &'a str,
    roles: Vec<String>,
    groups: Vec<String>,
    failure_domain: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct ClusterGroupRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    description: &'a str,
    members: Vec<String>,
}

#[derive(serde::Serialize)]
struct ClusterJoinTokenRequest<'a> {
    server_name: &'a str,
}

#[derive(serde::Serialize)]
struct ClusterMemberStateRequest<'a> {
    action: &'a str,
    mode: &'a str,
}

#[derive(serde::Serialize)]
struct StorageVolumeCreateRequest<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    volume_type: &'a str,
    content_type: &'a str,
    description: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct StorageVolumeUpdateRequest<'a> {
    description: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct StorageVolumeSnapshotRequest<'a> {
    name: &'a str,
}

#[derive(serde::Serialize)]
struct StorageVolumeRestoreRequest<'a> {
    restore: &'a str,
}

#[derive(serde::Serialize)]
struct StorageBucketRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    description: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct StorageBucketKeyRequest<'a> {
    name: &'a str,
    description: &'a str,
    role: &'a str,
}

#[derive(serde::Serialize)]
struct NetworkForwardCreateRequest<'a> {
    listen_address: &'a str,
    description: &'a str,
    ports: Vec<NetworkForwardPort>,
}

#[derive(serde::Serialize)]
struct NetworkIntegrationRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    integration_type: Option<&'a str>,
    description: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkForwardUpdateRequest<'a> {
    description: &'a str,
    ports: Vec<NetworkForwardPort>,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkAclCreateRequest<'a> {
    name: &'a str,
    description: &'a str,
    ingress: Vec<NetworkAclRule>,
    egress: Vec<NetworkAclRule>,
}

#[derive(serde::Serialize)]
struct NetworkAclUpdateRequest<'a> {
    description: &'a str,
    ingress: Vec<NetworkAclRule>,
    egress: Vec<NetworkAclRule>,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkAddressSetRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    description: &'a str,
    addresses: Vec<String>,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkLoadBalancerCreateRequest<'a> {
    listen_address: &'a str,
    description: &'a str,
    backends: Vec<NetworkLoadBalancerBackend>,
    ports: Vec<NetworkLoadBalancerPort>,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkLoadBalancerUpdateRequest<'a> {
    description: &'a str,
    backends: Vec<NetworkLoadBalancerBackend>,
    ports: Vec<NetworkLoadBalancerPort>,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkPeerCreateRequest<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(rename = "type")]
    peer_type: &'a str,
    target_project: &'a str,
    target_network: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    target_integration: &'a str,
}

#[derive(serde::Serialize)]
struct NetworkPeerUpdateRequest<'a> {
    description: &'a str,
    #[serde(rename = "type")]
    peer_type: &'a str,
    target_project: &'a str,
    target_network: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    target_integration: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkZoneRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    description: &'a str,
    config: BTreeMap<String, String>,
}

#[derive(serde::Serialize)]
struct NetworkZoneRecordRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    description: &'a str,
    entries: Vec<NetworkZoneRecordEntry>,
}

impl CreateInstanceRequest {
    fn from_spec(spec: &CreateInstanceSpec) -> Self {
        let mut config = BTreeMap::new();
        if !spec.cpu_limit.trim().is_empty() {
            config.insert("limits.cpu".to_owned(), spec.cpu_limit.trim().to_owned());
        }
        if !spec.memory_limit.trim().is_empty() {
            config.insert(
                "limits.memory".to_owned(),
                spec.memory_limit.trim().to_owned(),
            );
        }
        if !spec.cloud_init_user_data.trim().is_empty() {
            config.insert(
                "cloud-init.user-data".to_owned(),
                spec.cloud_init_user_data.clone(),
            );
        }
        if spec.privileged {
            config.insert("security.privileged".to_owned(), "true".to_owned());
        }
        if spec.nesting {
            config.insert("security.nesting".to_owned(), "true".to_owned());
        }
        if spec.instance_type == "virtual-machine" && !spec.secure_boot {
            config.insert("security.secureboot".to_owned(), "false".to_owned());
        }
        if spec.autostart {
            config.insert("boot.autostart".to_owned(), "true".to_owned());
        }

        let mut devices = BTreeMap::new();
        if !spec.storage_pool.trim().is_empty() || !spec.root_disk_size.trim().is_empty() {
            let mut root = BTreeMap::from([
                ("type".to_owned(), "disk".to_owned()),
                ("path".to_owned(), "/".to_owned()),
            ]);
            if !spec.storage_pool.trim().is_empty() {
                root.insert("pool".to_owned(), spec.storage_pool.trim().to_owned());
            }
            if !spec.root_disk_size.trim().is_empty() {
                root.insert("size".to_owned(), spec.root_disk_size.trim().to_owned());
            }
            devices.insert("root".to_owned(), root);
        }
        if !spec.network.trim().is_empty() {
            devices.insert(
                "eth0".to_owned(),
                BTreeMap::from([
                    ("type".to_owned(), "nic".to_owned()),
                    ("name".to_owned(), "eth0".to_owned()),
                    ("network".to_owned(), spec.network.trim().to_owned()),
                ]),
            );
        }

        let image = spec.image.trim();
        let source = if let Some(alias) = image.strip_prefix("images:") {
            ImageSource {
                source_type: "image",
                alias: alias.to_owned(),
                mode: Some("pull"),
                server: Some("https://images.linuxcontainers.org"),
                protocol: Some("simplestreams"),
            }
        } else {
            ImageSource {
                source_type: "image",
                alias: image.to_owned(),
                mode: None,
                server: None,
                protocol: None,
            }
        };

        Self {
            name: spec.name.trim().to_owned(),
            instance_type: spec.instance_type.clone(),
            source,
            profiles: spec.profiles.clone(),
            config,
            devices,
            start: spec.start_after_create,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn instance_path(name: &str, project: &str) -> String {
    format!("/instances/{name}{}", project_query(project))
}

fn create_instance_path(spec: &CreateInstanceSpec) -> String {
    let mut path = format!("/instances{}", project_query(&spec.project));
    if !spec.target_member.trim().is_empty() {
        path = append_query(&path, &format!("target={}", spec.target_member.trim()));
    }
    path
}

fn instance_subpath(name: &str, project: &str, subpath: &str) -> String {
    format!("/instances/{name}/{subpath}{}", project_query(project))
}

fn project_query(project: &str) -> String {
    if project.is_empty() || project == "default" {
        String::new()
    } else {
        format!("?project={project}")
    }
}

fn add_project_to_path(path: &str, project: &str) -> String {
    if project.is_empty() || project == "default" || path.contains("project=") {
        path.to_owned()
    } else if path.contains('?') {
        format!("{path}&project={project}")
    } else {
        format!("{path}?project={project}")
    }
}

fn append_query(path: &str, query: &str) -> String {
    if path.contains('?') {
        format!("{path}&{query}")
    } else {
        format!("{path}?{query}")
    }
}

fn file_path(name: &str, project: &str, path: &str) -> String {
    let encoded_path: String = byte_serialize(path.as_bytes()).collect();
    append_query(
        &instance_subpath(name, project, "files"),
        &format!("path={encoded_path}"),
    )
}

fn storage_volume_file_path(pool: &str, project: &str, volume: &str, path: &str) -> String {
    let encoded_path: String = byte_serialize(path.as_bytes()).collect();
    append_query(
        &format!(
            "/storage-pools/{pool}/volumes/custom/{volume}/files{}",
            project_query(project)
        ),
        &format!("path={encoded_path}"),
    )
}

fn encode_path_segment(value: &str) -> String {
    byte_serialize(value.as_bytes()).collect()
}

fn resource_collection_path(kind: ResourceKind, project: &str) -> Result<String, IncusError> {
    match kind {
        ResourceKind::Instance => Ok(format!("/instances{}", project_query(project))),
        ResourceKind::Network => Ok(format!("/networks{}", project_query(project))),
        ResourceKind::StoragePool => Ok("/storage-pools".to_owned()),
        ResourceKind::Project => Ok("/projects".to_owned()),
        ResourceKind::Profile => Ok(format!("/profiles{}", project_query(project))),
        ResourceKind::Image => Ok(format!("/images{}", project_query(project))),
        ResourceKind::Server => Err(IncusError::InvalidResource(
            "the server object is updated at /1.0 and cannot be created".to_owned(),
        )),
    }
}

fn resource_path(kind: ResourceKind, name: &str, project: &str) -> Result<String, IncusError> {
    match kind {
        ResourceKind::Instance => Ok(instance_path(name, project)),
        ResourceKind::Network => Ok(format!("/networks/{name}{}", project_query(project))),
        ResourceKind::StoragePool => Ok(format!("/storage-pools/{name}")),
        ResourceKind::Project => Ok(format!("/projects/{name}")),
        ResourceKind::Profile => Ok(format!("/profiles/{name}{}", project_query(project))),
        ResourceKind::Image => Ok(format!("/images/{name}{}", project_query(project))),
        ResourceKind::Server if name.is_empty() => Ok(String::new()),
        ResourceKind::Server => Err(IncusError::InvalidResource(
            "the server object does not have a name".to_owned(),
        )),
    }
}

fn api_url_for_remote(base_url: &str) -> Result<String, IncusError> {
    let base_url = base_url.trim_end_matches('/');
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(IncusError::InvalidRemoteUrl(base_url.to_owned()));
    }

    if base_url.ends_with("/1.0") {
        Ok(base_url.to_owned())
    } else {
        Ok(format!("{base_url}/1.0"))
    }
}

trait IfEmptyThen {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmptyThen for str {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

/// Failures produced while configuring or querying an Incus connection.
#[derive(Debug)]
pub enum IncusError {
    Request(reqwest::Error),
    Io(std::io::Error),
    InvalidRemoteUrl(String),
    InvalidCreate(String),
    InvalidExec(String),
    Interactive(String),
    InvalidSnapshot(String),
    InvalidBackup(String),
    InvalidResource(String),
    MissingMetadata,
    MissingEtag,
    MissingOperationId,
    UnsupportedLocalSocket,
    HttpStatus(StatusCode),
    Api {
        http_status: StatusCode,
        status_code: u16,
        message: String,
    },
}

impl fmt::Display for IncusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => write!(formatter, "Incus request failed: {error}"),
            Self::Io(error) => write!(formatter, "file transfer failed: {error}"),
            Self::InvalidRemoteUrl(url) => write!(
                formatter,
                "remote URL must start with http:// or https://: {url}"
            ),
            Self::InvalidCreate(message) => {
                write!(formatter, "invalid instance creation: {message}")
            }
            Self::InvalidExec(message) => write!(formatter, "invalid command execution: {message}"),
            Self::Interactive(message) => {
                write!(formatter, "interactive session failed: {message}")
            }
            Self::InvalidSnapshot(message) => write!(formatter, "invalid snapshot: {message}"),
            Self::InvalidBackup(message) => write!(formatter, "invalid backup: {message}"),
            Self::InvalidResource(message) => {
                write!(formatter, "invalid resource request: {message}")
            }
            Self::MissingMetadata => write!(formatter, "Incus response did not include metadata"),
            Self::MissingEtag => write!(
                formatter,
                "Incus did not return an ETag for this editable object"
            ),
            Self::MissingOperationId => write!(formatter, "Incus operation did not include an ID"),
            Self::UnsupportedLocalSocket => {
                write!(
                    formatter,
                    "local Incus Unix-socket connections require a Unix host"
                )
            }
            Self::HttpStatus(status) => write!(formatter, "Incus returned HTTP {status}"),
            Self::Api {
                http_status,
                status_code,
                message,
            } => write!(
                formatter,
                "Incus API error (HTTP {http_status}, status {status_code}): {message}"
            ),
        }
    }
}

impl Error for IncusError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackupRequest, ConsoleRequest, CreateInstanceRequest, ExecRequest, IncusClient,
        InstanceStateRequest, MoveInstanceRequest, SnapshotRequest, add_project_to_path,
        api_url_for_remote, append_query, create_instance_path, file_path, instance_path,
        instance_subpath, resource_collection_path, resource_path,
    };
    use crate::incus::{CreateInstanceSpec, InstanceAction, ResourceKind};

    #[test]
    fn normalizes_a_remote_endpoint_to_the_incus_api_root() {
        assert_eq!(
            api_url_for_remote("https://incus.example.test:8443/").expect("valid URL"),
            "https://incus.example.test:8443/1.0"
        );
        assert_eq!(
            api_url_for_remote("http://localhost:8443/1.0").expect("valid URL"),
            "http://localhost:8443/1.0"
        );
    }

    #[test]
    fn builds_project_scoped_instance_paths_and_state_requests() {
        assert_eq!(instance_path("demo", "default"), "/instances/demo");
        assert_eq!(instance_path("demo", "ops"), "/instances/demo?project=ops");
        assert_eq!(
            instance_subpath("demo", "ops", "state"),
            "/instances/demo/state?project=ops"
        );
        assert_eq!(
            append_query("/instances/demo/snapshots", "recursion=1"),
            "/instances/demo/snapshots?recursion=1"
        );
        assert_eq!(
            file_path("demo", "ops", "/etc/a file"),
            "/instances/demo/files?project=ops&path=%2Fetc%2Fa+file"
        );

        let body = serde_json::to_value(InstanceStateRequest {
            action: InstanceAction::Freeze.as_str(),
            timeout: -1,
            force: false,
            stateful: false,
        })
        .expect("serializes request");
        assert_eq!(body["action"], "freeze");
        assert_eq!(body["timeout"], -1);
        assert_eq!(body["force"], false);
    }

    #[test]
    fn builds_mutable_resource_paths_for_every_core_control_plane_object() {
        assert_eq!(
            resource_path(ResourceKind::Instance, "vm-01", "ops").expect("instance path"),
            "/instances/vm-01?project=ops"
        );
        assert_eq!(
            resource_path(ResourceKind::Network, "br0", "ops").expect("network path"),
            "/networks/br0?project=ops"
        );
        assert_eq!(
            resource_path(ResourceKind::StoragePool, "fast", "").expect("pool path"),
            "/storage-pools/fast"
        );
        assert_eq!(
            resource_path(ResourceKind::Project, "ops", "").expect("project path"),
            "/projects/ops"
        );
        assert_eq!(
            resource_path(ResourceKind::Profile, "gpu", "ops").expect("profile path"),
            "/profiles/gpu?project=ops"
        );
        assert_eq!(
            resource_collection_path(ResourceKind::Network, "ops").expect("network collection"),
            "/networks?project=ops"
        );
        assert!(resource_collection_path(ResourceKind::Server, "").is_err());
    }

    #[test]
    fn builds_a_vm_create_request_from_the_images_remote() {
        let spec = CreateInstanceSpec {
            name: "debian-vm".to_owned(),
            project: "ops".to_owned(),
            instance_type: "virtual-machine".to_owned(),
            cpu_limit: "2".to_owned(),
            memory_limit: "2GiB".to_owned(),
            storage_pool: "fast".to_owned(),
            root_disk_size: "20GiB".to_owned(),
            network: "incusbr0".to_owned(),
            cloud_init_user_data: "#cloud-config".to_owned(),
            target_member: "node-2".to_owned(),
            start_after_create: true,
            ..Default::default()
        };
        let request = CreateInstanceRequest::from_spec(&spec);
        let body = serde_json::to_value(request).expect("serializes create request");

        assert_eq!(body["name"], "debian-vm");
        assert_eq!(body["type"], "virtual-machine");
        assert_eq!(body["source"]["alias"], "debian/12");
        assert_eq!(body["source"]["protocol"], "simplestreams");
        assert_eq!(body["config"]["limits.cpu"], "2");
        assert_eq!(body["devices"]["root"]["pool"], "fast");
        assert_eq!(body["devices"]["root"]["size"], "20GiB");
        assert_eq!(body["devices"]["eth0"]["network"], "incusbr0");
        assert_eq!(body["config"]["cloud-init.user-data"], "#cloud-config");
        assert_eq!(body["start"], true);
        assert_eq!(
            create_instance_path(&spec),
            "/instances?project=ops&target=node-2"
        );
    }

    #[test]
    fn builds_non_interactive_exec_requests_and_preserves_project_paths() {
        let body = serde_json::to_value(ExecRequest {
            command: vec!["/bin/sh".into(), "-lc".into(), "id".into()],
            wait_for_websocket: false,
            interactive: false,
            record_output: true,
        })
        .expect("serializes exec request");

        assert_eq!(body["command"], serde_json::json!(["/bin/sh", "-lc", "id"]));
        assert_eq!(body["wait-for-websocket"], false);
        assert_eq!(body["record-output"], true);
        assert_eq!(
            add_project_to_path("/1.0/instances/demo/logs/exec_1.stdout", "ops"),
            "/1.0/instances/demo/logs/exec_1.stdout?project=ops"
        );
    }

    #[test]
    fn builds_interactive_exec_and_serial_console_requests() {
        let exec = serde_json::to_value(ExecRequest {
            command: vec!["/bin/sh".into()],
            wait_for_websocket: true,
            interactive: true,
            record_output: false,
        })
        .expect("exec request");
        assert_eq!(exec["wait-for-websocket"], true);
        assert_eq!(exec["interactive"], true);
        assert_eq!(exec["record-output"], false);

        let console = serde_json::to_value(ConsoleRequest {
            console_type: "console",
            width: 0,
            height: 0,
            force: false,
        })
        .expect("console request");
        assert_eq!(console["type"], "console");
        let vga = serde_json::to_value(ConsoleRequest {
            console_type: "vga",
            width: 0,
            height: 0,
            force: false,
        })
        .expect("VGA request");
        assert_eq!(vga["type"], "vga");
    }

    #[test]
    fn builds_structured_live_instance_migration_requests() {
        let body = serde_json::to_value(MoveInstanceRequest {
            migration: true,
            name: "demo-new",
            project: "ops",
            target: "node-b",
            pool: "fast",
            live: true,
        })
        .expect("move request");
        assert_eq!(body["migration"], true);
        assert_eq!(body["target"], "node-b");
        assert_eq!(body["pool"], "fast");
        assert_eq!(body["live"], true);
    }

    #[test]
    fn builds_stateful_snapshot_requests() {
        let body = serde_json::to_value(SnapshotRequest {
            name: "before-upgrade",
            stateful: true,
        })
        .expect("serializes snapshot request");
        assert_eq!(body["name"], "before-upgrade");
        assert_eq!(body["stateful"], true);
    }

    #[test]
    fn builds_optimized_instance_backup_requests() {
        let body = serde_json::to_value(BackupRequest {
            name: "before-upgrade",
            instance_only: false,
            optimized_storage: true,
        })
        .expect("serializes backup request");
        assert_eq!(body["name"], "before-upgrade");
        assert_eq!(body["instance_only"], false);
        assert_eq!(body["optimized_storage"], true);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn queries_an_incus_envelope_over_a_unix_socket() {
        use std::{
            io::{Read, Write},
            os::unix::net::UnixListener,
            thread,
        };

        let socket_path = std::env::temp_dir().join(format!(
            "virtty-incus-client-test-{}-{}.socket",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos()
        ));
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            // The development command sandbox intentionally blocks Unix-socket creation.
            // Keep the test useful on normal hosts without making `make test` environment-bound.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("bind test socket: {error}"),
        };
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client request");
            let mut request = [0_u8; 1024];
            let received = stream.read(&mut request).expect("read request");
            let request = std::str::from_utf8(&request[..received]).expect("HTTP request");
            assert!(request.starts_with("GET /1.0 HTTP/1.1"));

            let body = r#"{"type":"sync","status":"Success","status_code":200,"metadata":{"api_version":"1.0","environment":{"server_name":"test-incus"}}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write response");
        });

        let client = IncusClient::local_socket(&socket_path).expect("configure client");
        let server_info = client.server_info().await.expect("read server info");
        server.join().expect("server thread succeeds");
        std::fs::remove_file(socket_path).expect("remove test socket");

        assert_eq!(server_info.environment.server_name, "test-incus");
    }
}
