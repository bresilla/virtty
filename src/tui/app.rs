use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    incus::{
        AccessEntry, BackupSummary, ClusterGroupSummary, CreateInstanceSpec, EventEntry,
        ExecResult, ImagePublishSpec, ImageUpdateSpec, IncusClient, IncusError, InstanceAction,
        InstanceDetail, MoveInstanceSpec, NetworkAclRule, NetworkAclSpec, NetworkAclSummary,
        NetworkAddressSetSummary, NetworkForwardPort, NetworkForwardSpec, NetworkForwardSummary,
        NetworkForwardUpdateSpec, NetworkIntegrationSummary, NetworkLease,
        NetworkLoadBalancerBackend, NetworkLoadBalancerPort, NetworkLoadBalancerSpec,
        NetworkLoadBalancerSummary, NetworkLoadBalancerUpdateSpec, NetworkPeerSpec,
        NetworkPeerSummary, NetworkPeerUpdateSpec, NetworkZoneRecordEntry,
        NetworkZoneRecordSummary, NetworkZoneSummary, ResourceDefinition, ResourceKind,
        SnapshotSummary, StorageBucketKeySummary, StorageBucketSummary, StorageVolumeBitmap,
        StorageVolumeBitmapSpec, StorageVolumeSnapshotSummary, StorageVolumeSpec,
        StorageVolumeSummary, StorageVolumeUpdateSpec, Transport,
    },
    journal::ActionEntry,
    model::AppModel,
    openfga::TupleKey,
    remotes::SavedRemote,
    schedules::BackupSchedule,
    views::SavedView,
};

/// TUI sections, ordered as they appear in the tab bar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Page {
    Dashboard,
    Instances,
    Operations,
    Events,
    Storage,
    Networks,
    Images,
    Projects,
    Settings,
}

impl Page {
    pub const ALL: [Self; 9] = [
        Self::Dashboard,
        Self::Instances,
        Self::Operations,
        Self::Events,
        Self::Storage,
        Self::Networks,
        Self::Images,
        Self::Projects,
        Self::Settings,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Instances => "Instances",
            Self::Operations => "Operations",
            Self::Events => "Events",
            Self::Storage => "Storage",
            Self::Networks => "Networks",
            Self::Images => "Images",
            Self::Projects => "Projects",
            Self::Settings => "Settings",
        }
    }

    pub(super) const GROUPS: [&'static str; 5] = [
        "Dashboard",
        "Workloads",
        "Activity",
        "Infrastructure",
        "Governance",
    ];

    pub(super) const GROUPS_COMPACT: [&'static str; 5] =
        ["Dash", "Work", "Activity", "Infra", "Gov"];

    pub(super) const fn group(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Instances | Self::Images => 1,
            Self::Operations | Self::Events => 2,
            Self::Storage | Self::Networks => 3,
            Self::Projects | Self::Settings => 4,
        }
    }

    pub(super) const fn group_pages(group: usize) -> &'static [Self] {
        match group {
            1 => &[Self::Instances, Self::Images],
            2 => &[Self::Operations, Self::Events],
            3 => &[Self::Storage, Self::Networks],
            4 => &[Self::Projects, Self::Settings],
            _ => &[Self::Dashboard],
        }
    }

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Instances => 1,
            Self::Operations => 2,
            Self::Events => 3,
            Self::Storage => 4,
            Self::Networks => 5,
            Self::Images => 6,
            Self::Projects => 7,
            Self::Settings => 8,
        }
    }

    fn next_view(self) -> Self {
        let pages = Self::group_pages(self.group());
        let index = pages.iter().position(|page| *page == self).unwrap_or(0);
        pages[(index + 1) % pages.len()]
    }

    fn previous_view(self) -> Self {
        let pages = Self::group_pages(self.group());
        let index = pages.iter().position(|page| *page == self).unwrap_or(0);
        pages[(index + pages.len() - 1) % pages.len()]
    }

    fn next_group(self) -> Self {
        Self::group_pages((self.group() + 1) % Self::GROUPS.len())[0]
    }

    fn previous_group(self) -> Self {
        Self::group_pages((self.group() + Self::GROUPS.len() - 1) % Self::GROUPS.len())[0]
    }
}

/// Stateful application controller, kept independent from terminal rendering.
#[derive(Debug)]
pub struct App {
    target: String,
    page: Page,
    selected_instance: usize,
    selected_operation: usize,
    selected_storage_pool: usize,
    selected_network: usize,
    selected_project: usize,
    active_project: String,
    selected_profile: usize,
    selected_image: usize,
    filter_query: String,
    filter_form: Option<String>,
    action_menu: Option<ActionMenu>,
    connection_form: Option<ConnectionForm>,
    saved_remotes: Vec<SavedRemote>,
    saved_views: Vec<SavedView>,
    view_manager: Option<SavedViewManager>,
    audit_export: Option<AuditExportForm>,
    action_journal: Vec<ActionEntry>,
    journal_viewer: Option<JournalViewer>,
    fleet_manager: Option<FleetManager>,
    placement_inspector: Option<PlacementInspector>,
    backup_schedules: Vec<BackupSchedule>,
    backup_schedule_form: Option<BackupScheduleForm>,
    openfga_manager: Option<OpenFgaManager>,
    image_retention_preview: Option<ImageRetentionPreview>,
    cluster_group_manager: Option<ClusterGroupManager>,
    move_form: Option<MoveForm>,
    project_usage: Option<ProjectUsageForm>,
    project_access: Option<ProjectAccessForm>,
    instance_access: Option<InstanceAccessForm>,
    should_quit: bool,
    confirmation: Option<Confirmation>,
    create_form: Option<CreateForm>,
    exec_form: Option<ExecForm>,
    snapshot_form: Option<SnapshotForm>,
    backup_form: Option<BackupForm>,
    console_form: Option<ConsoleForm>,
    detail_form: Option<DetailForm>,
    config_form: Option<ConfigForm>,
    focus_config_key: Option<&'static str>,
    resource_create_form: Option<ResourceCreateForm>,
    resource_delete_confirmation: Option<ResourceDeleteConfirmation>,
    image_alias_form: Option<ImageAliasForm>,
    image_alias_manager: Option<ImageAliasManager>,
    image_form: Option<ImageForm>,
    artifacts_form: Option<ArtifactsForm>,
    operation_detail: Option<OperationDetailForm>,
    rename_form: Option<RenameForm>,
    rebuild_form: Option<RebuildForm>,
    log_manager: Option<InstanceLogManager>,
    allocation_explorer: Option<NetworkAllocationExplorer>,
    metrics_explorer: Option<MetricsExplorer>,
    maintenance_manager: Option<InstanceMaintenanceManager>,
    storage_resource_detail: Option<String>,
    settings_manager: Option<SettingsManager>,
    logging_target_manager: Option<LoggingTargetManager>,
    scriptlet_manager: Option<ScriptletManager>,
    preseed_manager: Option<PreseedManager>,
    pool_safety_manager: Option<PoolSafetyManager>,
    qmp_manager: Option<QmpManager>,
    bulk_manager: Option<BulkManager>,
    bulk_progress: Option<tokio::sync::mpsc::UnboundedReceiver<BulkProgress>>,
    bulk_cancel: Option<Arc<AtomicBool>>,
    device_manager: Option<DeviceManager>,
    volume_manager: Option<VolumeManager>,
    bucket_manager: Option<BucketManager>,
    network_services: Option<NetworkServicesManager>,
    zone_manager: Option<ZoneManager>,
    bgp_form: Option<BgpForm>,
    address_set_manager: Option<AddressSetManager>,
    integration_manager: Option<IntegrationManager>,
    file_form: Option<FileForm>,
    operation_confirmation: Option<OperationConfirmation>,
    help_visible: bool,
    pub model: AppModel,
}

/// The explicit phrase required before a lifecycle change is submitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confirmation {
    action: InstanceAction,
    instance_name: String,
    project: String,
    input: String,
}

/// Keyboard form used to create an Incus container or VM.
#[derive(Clone, Debug)]
pub struct CreateForm {
    spec: CreateInstanceSpec,
    profiles: String,
    field: usize,
}

/// A one-shot command capture session for a selected instance.
#[derive(Clone, Debug)]
pub struct ExecForm {
    instance_name: String,
    project: String,
    instance_type: String,
    command: String,
    result: Option<ExecResult>,
}

/// Small form for creating a named snapshot of the selected instance.
#[derive(Clone, Debug)]
pub struct SnapshotForm {
    instance_name: String,
    project: String,
    name: String,
    stateful: bool,
}

/// Form for an exportable Incus backup.
#[derive(Clone, Debug)]
pub struct BackupForm {
    instance_name: String,
    project: String,
    name: String,
    instance_only: bool,
    optimized_storage: bool,
}

#[derive(Clone, Debug)]
pub struct BackupScheduleForm {
    schedule: BackupSchedule,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct OpenFgaManager {
    url: String,
    store: String,
    token: String,
    tuples: Vec<TupleKey>,
    selected: usize,
    fields: [String; 5],
    field: usize,
    operation: usize,
    confirmation: String,
    confirming: bool,
    result: String,
}

#[derive(Clone, Debug)]
pub struct ImageRetentionPreview {
    expiry_hours: u64,
    rows: Vec<(String, String, bool)>,
    selected: usize,
}

impl ImageRetentionPreview {
    pub const fn expiry_hours(&self) -> u64 {
        self.expiry_hours
    }
    pub fn rows(&self) -> &[(String, String, bool)] {
        &self.rows
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
}

impl OpenFgaManager {
    pub fn tuples(&self) -> &[TupleKey] {
        &self.tuples
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
    pub fn fields(&self) -> [(&'static str, &str, bool); 5] {
        [
            ("Subject type", self.fields[0].as_str(), self.field == 0),
            ("Subject ID", self.fields[1].as_str(), self.field == 1),
            ("Relation", self.fields[2].as_str(), self.field == 2),
            ("Object type", self.fields[3].as_str(), self.field == 3),
            ("Object ID", self.fields[4].as_str(), self.field == 4),
        ]
    }
    pub fn operation(&self) -> &'static str {
        ["check", "grant", "revoke"][self.operation]
    }
    pub fn result(&self) -> &str {
        &self.result
    }
    pub fn confirmation(&self) -> &str {
        &self.confirmation
    }
    pub const fn confirming(&self) -> bool {
        self.confirming
    }
}

impl BackupScheduleForm {
    pub fn fields(&self) -> [(&'static str, String, bool, bool); 6] {
        [
            (
                "Enabled",
                if self.schedule.enabled { "on" } else { "off" }.into(),
                true,
                self.field == 0,
            ),
            (
                "Interval",
                format_schedule_interval(self.schedule.interval_seconds).into(),
                true,
                self.field == 1,
            ),
            (
                "Name pattern",
                self.schedule.name_pattern.clone(),
                false,
                self.field == 2,
            ),
            (
                "Instance only",
                if self.schedule.instance_only {
                    "on"
                } else {
                    "off"
                }
                .into(),
                true,
                self.field == 3,
            ),
            (
                "Optimized storage",
                if self.schedule.optimized_storage {
                    "on"
                } else {
                    "off"
                }
                .into(),
                true,
                self.field == 4,
            ),
            (
                "Last result",
                if self.schedule.last_result.is_empty() {
                    "never run".into()
                } else {
                    self.schedule.last_result.clone()
                },
                true,
                self.field == 5,
            ),
        ]
    }
    pub fn instance(&self) -> &str {
        &self.schedule.instance
    }
    pub fn project(&self) -> &str {
        &self.schedule.project
    }
}

/// Read-only serial console log for a selected instance.
#[derive(Clone, Debug)]
pub struct ConsoleForm {
    instance_name: String,
    project: String,
    instance_type: String,
    output: String,
}

/// Read-only full instance definition for inspection before future edit flows.
#[derive(Clone, Debug)]
pub struct DetailForm {
    name: String,
    project: String,
    detail: Option<InstanceDetail>,
    state: Option<serde_json::Value>,
    expanded_diff: bool,
}

/// Structured, resource-specific configuration form backed by an Incus ETag.
#[derive(Clone, Debug)]
pub struct ConfigForm {
    definition: ResourceDefinition,
    fields: Vec<ConfigField>,
    field: usize,
    omitted_capabilities: Vec<&'static str>,
}

/// Structured creation form for top-level Incus resources.
#[derive(Clone, Debug)]
pub struct ResourceCreateForm {
    kind: ResourceKind,
    fields: Vec<CreateField>,
    field: usize,
    wipe_confirmation: Option<String>,
}

#[derive(Clone, Debug)]
struct CreateField {
    label: &'static str,
    key: CreateKey,
    kind: CreateFieldKind,
    value: String,
}

#[derive(Clone, Copy, Debug)]
enum CreateKey {
    Name,
    Project,
    Type,
    Driver,
    Description,
    Config(&'static str),
}

#[derive(Clone, Copy, Debug)]
enum CreateFieldKind {
    Text,
    Toggle,
    Choice(&'static [&'static str]),
}

/// Type-to-confirm deletion for a non-instance resource.
#[derive(Clone, Debug)]
pub struct ResourceDeleteConfirmation {
    kind: ResourceKind,
    name: String,
    project: String,
    input: String,
}

/// Structured form for adding an alias to the selected image.
#[derive(Clone, Debug)]
pub struct ImageAliasForm {
    fingerprint: String,
    project: String,
    alias: String,
    description: String,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct ImageAliasManager {
    fingerprint: String,
    project: String,
    aliases: Vec<crate::incus::ImageAlias>,
    selected: usize,
    editor: Option<ImageAliasEditor>,
    delete_input: Option<String>,
}

#[derive(Clone, Debug)]
struct ImageAliasEditor {
    original: String,
    name: String,
    description: String,
    field: usize,
}

#[derive(Clone, Debug)]
pub enum ImageForm {
    Edit {
        spec: ImageUpdateSpec,
        fields: [String; 9],
        field: usize,
    },
    Import {
        project: String,
        path: String,
        aliases: String,
        public: bool,
        field: usize,
    },
    Export {
        fingerprint: String,
        project: String,
        path: String,
    },
    Publish {
        spec: ImagePublishSpec,
        field: usize,
    },
    Push {
        fingerprint: String,
        project: String,
        fields: [String; 6],
        field: usize,
    },
    CopyProject {
        fingerprint: String,
        source_project: String,
        target_project: String,
        public: bool,
        field: usize,
    },
}

#[derive(Clone, Debug)]
pub struct ActionMenu {
    title: String,
    entries: Vec<ActionMenuEntry>,
    selected: usize,
}

#[derive(Clone, Debug)]
pub struct SavedViewManager {
    views: Vec<SavedView>,
    selected: usize,
    name_input: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuditExportForm {
    path: String,
    filter: String,
    include_events: bool,
    include_operations: bool,
    redact: bool,
    field: usize,
}

#[derive(Clone, Debug, Default)]
pub struct JournalViewer {
    selected: usize,
    filter: String,
    filtering: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FleetManager {
    rows: Vec<FleetRow>,
    selected: usize,
    loading: bool,
}

#[derive(Clone, Debug)]
pub struct FleetRow {
    pub name: String,
    pub endpoint: String,
    pub version: String,
    pub instances: usize,
    pub running: usize,
    pub warnings: usize,
    pub drift_keys: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PlacementInspector {
    instance: crate::incus::InstanceSummary,
    members: Vec<crate::incus::ClusterMemberSummary>,
    selected: usize,
}

impl PlacementInspector {
    pub fn instance(&self) -> &crate::incus::InstanceSummary {
        &self.instance
    }
    pub fn members(&self) -> &[crate::incus::ClusterMemberSummary] {
        &self.members
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
    pub fn eligibility(&self, member: &crate::incus::ClusterMemberSummary) -> (bool, String) {
        if !member.status.eq_ignore_ascii_case("online") {
            return (false, format!("member status is {}", member.status));
        }
        if !self.instance.architecture.is_empty()
            && !member.architecture.is_empty()
            && self.instance.architecture != member.architecture
        {
            return (
                false,
                format!(
                    "architecture {} does not match {}",
                    member.architecture, self.instance.architecture
                ),
            );
        }
        (
            true,
            "passes observable status and architecture checks".into(),
        )
    }
}

impl FleetManager {
    pub fn rows(&self) -> &[FleetRow] {
        &self.rows
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
    pub const fn loading(&self) -> bool {
        self.loading
    }
}

impl JournalViewer {
    pub fn filter(&self) -> &str {
        &self.filter
    }
    pub const fn filtering(&self) -> bool {
        self.filtering
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
}

impl AuditExportForm {
    pub fn fields(&self) -> [(&'static str, String, bool, bool); 5] {
        [
            ("Destination", self.path.clone(), false, self.field == 0),
            ("Filter", self.filter.clone(), false, self.field == 1),
            (
                "Events",
                if self.include_events { "on" } else { "off" }.into(),
                true,
                self.field == 2,
            ),
            (
                "Operations",
                if self.include_operations { "on" } else { "off" }.into(),
                true,
                self.field == 3,
            ),
            (
                "Redact secrets",
                if self.redact { "on" } else { "off" }.into(),
                true,
                self.field == 4,
            ),
        ]
    }
    pub fn preview_counts(&self, app: &App) -> (usize, usize) {
        let filter = self.filter.to_ascii_lowercase();
        let events = app
            .model
            .events
            .iter()
            .filter(|event| {
                filter.is_empty()
                    || event.summary().to_ascii_lowercase().contains(&filter)
                    || event.project.to_ascii_lowercase().contains(&filter)
            })
            .count();
        let operations = app
            .model
            .operations
            .iter()
            .filter(|operation| {
                filter.is_empty()
                    || operation.id.to_ascii_lowercase().contains(&filter)
                    || operation.description.to_ascii_lowercase().contains(&filter)
                    || operation.status.to_ascii_lowercase().contains(&filter)
            })
            .count();
        (
            if self.include_events { events } else { 0 },
            if self.include_operations {
                operations
            } else {
                0
            },
        )
    }
}

impl SavedViewManager {
    pub fn views(&self) -> &[SavedView] {
        &self.views
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn name_input(&self) -> Option<&str> {
        self.name_input.as_deref()
    }
}

#[derive(Clone, Debug)]
pub struct ActionMenuEntry {
    label: &'static str,
    key: KeyCode,
}

#[derive(Clone, Debug)]
pub struct ConnectionForm {
    remote: bool,
    fields: [String; 5],
    field: usize,
    saved: Vec<SavedRemote>,
    selected_saved: usize,
}

#[derive(Clone, Debug)]
pub struct ClusterGroupManager {
    groups: Vec<ClusterGroupSummary>,
    selected: usize,
    editor: Option<ClusterGroupEditor>,
    delete_input: Option<String>,
    rename_input: Option<String>,
}

#[derive(Clone, Debug)]
struct LoadBalancerHealthSample {
    timestamp: u64,
    address: String,
    summary: String,
}

#[derive(Clone, Debug)]
pub struct MoveForm {
    spec: MoveInstanceSpec,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct ProjectUsageForm {
    project: String,
    state: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct ProjectAccessForm {
    project: String,
    entries: Vec<AccessEntry>,
}

#[derive(Clone, Debug)]
pub struct InstanceAccessForm {
    name: String,
    project: String,
    entries: Vec<AccessEntry>,
}

#[derive(Clone, Debug)]
struct ClusterGroupEditor {
    editing: bool,
    fields: [String; 3],
    field: usize,
}

impl ActionMenu {
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn entries(&self) -> impl Iterator<Item = (&str, bool)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.label, index == self.selected))
    }
}

impl ConnectionForm {
    fn new(saved: Vec<SavedRemote>) -> Self {
        Self {
            remote: false,
            fields: [
                "/var/lib/incus/unix.socket".to_owned(),
                String::new(),
                String::new(),
                String::new(),
                "local".to_owned(),
            ],
            field: 0,
            saved,
            selected_saved: 0,
        }
    }
    pub fn remote(&self) -> bool {
        self.remote
    }
    pub fn fields(&self) -> [(&'static str, &str, bool); 5] {
        [
            (
                if self.remote {
                    "Remote URL"
                } else {
                    "Unix socket"
                },
                self.fields[0].as_str(),
                self.field == 0,
            ),
            (
                "Client certificate",
                self.fields[1].as_str(),
                self.field == 1,
            ),
            ("Client key", self.fields[2].as_str(), self.field == 2),
            ("Server CA", self.fields[3].as_str(), self.field == 3),
            ("Saved name", self.fields[4].as_str(), self.field == 4),
        ]
    }
    pub fn saved(&self) -> &[SavedRemote] {
        &self.saved
    }
    pub fn selected_saved(&self) -> usize {
        self.selected_saved
    }

    fn transport(&self) -> Transport {
        if self.remote {
            if self.fields[1].is_empty() {
                Transport::remote(self.fields[0].trim())
            } else {
                Transport::remote_tls(
                    self.fields[0].trim(),
                    self.fields[1].trim(),
                    self.fields[2].trim(),
                    (!self.fields[3].trim().is_empty()).then(|| self.fields[3].trim()),
                )
            }
        } else {
            Transport::local(self.fields[0].trim())
        }
    }

    fn load_selected(&mut self) {
        let Some(saved) = self.saved.get(self.selected_saved) else {
            return;
        };
        self.fields[4] = saved.name.clone();
        match &saved.transport {
            Transport::LocalUnix { socket_path } => {
                self.remote = false;
                self.fields[0] = socket_path.display().to_string();
                self.fields[1..4].fill(String::new());
            }
            Transport::Remote {
                base_url,
                client_certificate,
                client_key,
                server_ca,
            } => {
                self.remote = true;
                self.fields[0] = base_url.clone();
                self.fields[1] = client_certificate
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                self.fields[2] = client_key
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                self.fields[3] = server_ca
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
            }
        }
    }
}

impl ClusterGroupManager {
    fn new() -> Self {
        Self {
            groups: Vec::new(),
            selected: 0,
            editor: None,
            delete_input: None,
            rename_input: None,
        }
    }
    pub fn groups(&self) -> &[ClusterGroupSummary] {
        &self.groups
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 3]> {
        self.editor.as_ref().map(|editor| {
            [
                ("Name", editor.fields[0].as_str(), editor.field == 0),
                ("Description", editor.fields[1].as_str(), editor.field == 1),
                ("Members", editor.fields[2].as_str(), editor.field == 2),
            ]
        })
    }
    pub fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.editing)
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete cluster group {}",
            self.groups
                .get(self.selected)
                .map(|v| v.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl MoveForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            spec: MoveInstanceSpec {
                name: instance.name.clone(),
                project: instance.project.clone(),
                new_name: instance.name.clone(),
                target_project: instance.project.clone(),
                target_member: String::new(),
                target_pool: String::new(),
                live: instance.status == "Running",
            },
            field: 0,
        }
    }
    pub fn fields(&self) -> [(&'static str, &str, bool, bool); 5] {
        [
            ("New name", &self.spec.new_name, false, self.field == 0),
            (
                "Target project",
                &self.spec.target_project,
                false,
                self.field == 1,
            ),
            (
                "Target member",
                &self.spec.target_member,
                false,
                self.field == 2,
            ),
            (
                "Target pool",
                &self.spec.target_pool,
                false,
                self.field == 3,
            ),
            (
                "Live migration",
                if self.spec.live { "true" } else { "false" },
                true,
                self.field == 4,
            ),
        ]
    }
    pub fn source(&self) -> (&str, &str) {
        (&self.spec.name, &self.spec.project)
    }
}

impl ProjectUsageForm {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn state(&self) -> Option<&serde_json::Value> {
        self.state.as_ref()
    }
}

impl ProjectAccessForm {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn entries(&self) -> &[AccessEntry] {
        &self.entries
    }
}

impl InstanceAccessForm {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn entries(&self) -> &[AccessEntry] {
        &self.entries
    }
}

/// Snapshot and backup manager for one instance.
#[derive(Clone, Debug)]
pub struct ArtifactsForm {
    instance_name: String,
    project: String,
    instance_type: String,
    snapshots: Vec<SnapshotSummary>,
    backups: Vec<BackupSummary>,
    tab: ArtifactTab,
    selected: usize,
    delete_input: Option<String>,
    restoring: bool,
    rename_input: Option<String>,
    transfer: Option<(bool, String)>,
    retention_preview: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactTab {
    Snapshots,
    Backups,
}

/// Full structured details for the selected asynchronous operation.
#[derive(Clone, Debug)]
pub struct OperationDetailForm {
    operation: crate::incus::Operation,
}

/// Structured instance rename form.
#[derive(Clone, Debug)]
pub struct RenameForm {
    current_name: String,
    project: String,
    new_name: String,
}

/// Guarded instance root-disk rebuild form.
#[derive(Clone, Debug)]
pub struct RebuildForm {
    name: String,
    project: String,
    image: String,
    confirmation: String,
    field: usize,
}

/// Selectable instance and exec-output log browser.
#[derive(Clone, Debug)]
pub struct InstanceLogManager {
    name: String,
    project: String,
    logs: Vec<String>,
    selected: usize,
    content: Option<String>,
    delete_input: Option<String>,
    clear_console_input: Option<String>,
    cleanup_exec_input: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NetworkAllocationExplorer {
    selected: usize,
    filter: String,
    filtering: bool,
}

#[derive(Clone, Debug)]
pub struct MetricsExplorer {
    instance: String,
    project: String,
    selected: usize,
    filter: String,
    filtering: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaintenanceTab {
    Metadata,
    Templates,
    Diagnostics,
}

#[derive(Clone, Debug)]
pub struct InstanceMaintenanceManager {
    name: String,
    project: String,
    tab: MaintenanceTab,
    metadata: serde_json::Value,
    metadata_etag: Option<String>,
    templates: Vec<String>,
    selected: usize,
    metadata_editor: Option<(String, String, usize)>,
    template_editor: Option<(String, String, usize)>,
    template_permissions: Option<(String, String, String, String, usize)>,
    delete_input: Option<String>,
    dump_path: Option<String>,
    screenshot_path: Option<String>,
    repair_input: Option<String>,
    protocol_form: Option<(String, String, usize)>,
    proxy_address: Option<String>,
    bitmap_form: Option<(String, String, bool, bool, usize)>,
}

/// Warning, trust-certificate, and cluster-member management surface.
#[derive(Clone, Debug)]
pub struct SettingsManager {
    tab: SettingsTab,
    selected: usize,
    delete_input: Option<String>,
    certificate_form: Option<CertificateForm>,
    certificate_edit: Option<CertificateEditForm>,
    trust_token: Option<String>,
    cluster_form: Option<ClusterMemberForm>,
    cluster_action: Option<(String, String)>,
    cluster_join_name: Option<String>,
    cluster_rename_input: Option<String>,
    cluster_detail: Option<(String, String)>,
    cluster_config: Option<ClusterConfigForm>,
    cluster_certificate: Option<ClusterCertificateForm>,
    capability_filter: Option<String>,
    capability_selected: usize,
}

/// Structured editor for dynamically named remote server logging targets.
#[derive(Clone, Debug)]
pub struct LoggingTargetManager {
    definition: ResourceDefinition,
    targets: Vec<String>,
    selected: usize,
    editor: Option<LoggingTargetForm>,
    delete_input: Option<String>,
    supports_webhook: bool,
}

pub type LoggingFieldView<'a> = (&'static str, &'a str, bool, bool, bool);

/// Staged editor for server-wide placement and authorization Starlark scriptlets.
#[derive(Clone, Debug)]
pub struct ScriptletManager {
    definition: ResourceDefinition,
    kind: ScriptletKind,
    original: String,
    draft: String,
    cursor: usize,
    validation: Vec<String>,
    activation_input: Option<String>,
    rollback: bool,
    supports_placement: bool,
    supports_authorization: bool,
}

/// Typed bootstrap/preseed workflow; files are only an import/export transport.
#[derive(Clone, Debug)]
pub struct PreseedManager {
    spec: PreseedSpec,
    field: usize,
    preview: bool,
    transfer: Option<(bool, String)>,
    apply_input: Option<String>,
    results: Vec<String>,
}

/// Dedicated loop-pool growth and driver health workflow.
#[derive(Clone, Debug)]
pub struct PoolSafetyManager {
    definition: ResourceDefinition,
    resources: serde_json::Value,
    driver: String,
    tab: PoolSafetyTab,
    new_size: String,
    confirmation: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolSafetyTab {
    Growth,
    Health,
}

/// Structured, allow-listed QMP hook command manager for virtual machines.
#[derive(Clone, Debug)]
pub struct QmpManager {
    definition: ResourceDefinition,
    stage: String,
    command: String,
    argument_name: String,
    argument_value: String,
    field: usize,
    commands: Vec<serde_json::Value>,
    selected: usize,
    confirmation: Option<String>,
}

/// Multi-instance dry-run and bounded-concurrency action workflow.
#[derive(Clone, Debug)]
pub struct BulkManager {
    targets: Vec<BulkTarget>,
    selected: usize,
    action: InstanceAction,
    concurrency: usize,
    dry_run: bool,
    confirmation: Option<String>,
    results: Vec<String>,
    failed: Vec<(String, String)>,
    running: bool,
    completed: usize,
    total: usize,
    cancelled: bool,
}

enum BulkProgress {
    Outcome {
        target: BulkTarget,
        result: Box<Result<Option<crate::incus::Operation>, String>>,
    },
    Finished,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BulkTarget {
    name: String,
    project: String,
    status: String,
    selected: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PreseedSpec {
    pub server_enabled: bool,
    pub https_address: String,
    pub metrics_address: String,
    pub cluster_enabled: bool,
    pub cluster_server_name: String,
    pub cluster_server_address: String,
    pub cluster_address: String,
    pub cluster_token: String,
    pub cluster_certificate_path: String,
    pub cluster_password: String,
    pub cluster_group: String,
    pub storage_enabled: bool,
    pub pool_name: String,
    pub pool_driver: String,
    pub pool_source: String,
    pub network_enabled: bool,
    pub network_name: String,
    pub network_type: String,
    pub ipv4_address: String,
    pub ipv6_address: String,
    pub profile_enabled: bool,
    pub profile_name: String,
    pub profile_cpu: String,
    pub profile_memory: String,
    pub profile_pool: String,
    pub profile_network: String,
    #[serde(default)]
    pub additional_pools: Vec<PreseedPoolSpec>,
    #[serde(default)]
    pub additional_networks: Vec<PreseedNetworkSpec>,
    #[serde(default)]
    pub additional_profiles: Vec<PreseedProfileSpec>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PreseedPoolSpec {
    pub name: String,
    pub driver: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PreseedNetworkSpec {
    pub name: String,
    pub network_type: String,
    pub ipv4_address: String,
    pub ipv6_address: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PreseedProfileSpec {
    pub name: String,
    pub cpu: String,
    pub memory: String,
    pub pool: String,
    pub network: String,
}

impl Default for PreseedSpec {
    fn default() -> Self {
        Self {
            server_enabled: true,
            https_address: String::new(),
            metrics_address: String::new(),
            cluster_enabled: false,
            cluster_server_name: String::new(),
            cluster_server_address: String::new(),
            cluster_address: String::new(),
            cluster_token: String::new(),
            cluster_certificate_path: String::new(),
            cluster_password: String::new(),
            cluster_group: String::new(),
            storage_enabled: true,
            pool_name: "default".into(),
            pool_driver: "dir".into(),
            pool_source: String::new(),
            network_enabled: true,
            network_name: "incusbr0".into(),
            network_type: "bridge".into(),
            ipv4_address: "auto".into(),
            ipv6_address: "auto".into(),
            profile_enabled: true,
            profile_name: "default".into(),
            profile_cpu: String::new(),
            profile_memory: String::new(),
            profile_pool: String::new(),
            profile_network: String::new(),
            additional_pools: Vec::new(),
            additional_networks: Vec::new(),
            additional_profiles: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScriptletKind {
    Placement,
    Authorization,
    Qemu,
}

impl ScriptletKind {
    const fn key(self) -> &'static str {
        match self {
            Self::Placement => "instances.placement.scriptlet",
            Self::Authorization => "authorization.scriptlet",
            Self::Qemu => "raw.qemu.scriptlet",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Authorization => "authorization",
            Self::Qemu => "QEMU",
        }
    }

    const fn required_function(self) -> &'static str {
        match self {
            Self::Placement => "def instance_placement(request, candidate_members):",
            Self::Authorization => "def authorize(details, object, entitlement):",
            Self::Qemu => "def qemu_hook(instance, stage):",
        }
    }
}

#[derive(Clone, Debug)]
struct LoggingTargetForm {
    original_name: Option<String>,
    values: [String; 13],
    field: usize,
}

#[derive(Clone, Debug)]
struct ClusterConfigForm {
    fields: [String; 6],
    enabled: bool,
    field: usize,
}

#[derive(Clone, Debug)]
struct ClusterCertificateForm {
    certificate_path: String,
    key_path: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct CertificateForm {
    mode: String,
    name: String,
    restricted: bool,
    projects: String,
    certificate_path: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct CertificateEditForm {
    fingerprint: String,
    name: String,
    restricted: bool,
    projects: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct ClusterMemberForm {
    description: String,
    roles: String,
    groups: String,
    failure_domain: String,
    field: usize,
}

/// Structured local-device manager for an instance or profile definition.
#[derive(Clone, Debug)]
pub struct DeviceManager {
    definition: ResourceDefinition,
    devices: Vec<(String, std::collections::BTreeMap<String, String>)>,
    selected: usize,
    editor: Option<DeviceForm>,
    delete_input: Option<String>,
}

#[derive(Clone, Debug)]
struct DeviceForm {
    original_name: Option<String>,
    fields: Vec<DeviceField>,
    field: usize,
}

#[derive(Clone, Debug)]
struct DeviceField {
    label: &'static str,
    key: &'static str,
    value: String,
    choices: &'static [&'static str],
}

/// Custom storage-volume manager for one pool.
#[derive(Clone, Debug)]
pub struct VolumeManager {
    pool: String,
    volumes: Vec<StorageVolumeSummary>,
    selected: usize,
    create_form: Option<VolumeCreateForm>,
    edit_form: Option<VolumeEditForm>,
    attach_form: Option<VolumeAttachForm>,
    detach_form: Option<VolumeDetachForm>,
    snapshots: Option<VolumeSnapshotsForm>,
    backups: Option<VolumeBackupsForm>,
    delete_input: Option<String>,
    rebuild_input: Option<String>,
    import_form: Option<VolumeImportForm>,
    copy_form: Option<VolumeCopyForm>,
    protocol_form: Option<(String, String, usize)>,
    proxy_address: Option<String>,
    file_form: Option<VolumeFileForm>,
    detail: Option<(String, String)>,
    bitmaps: Option<VolumeBitmapsForm>,
}

#[derive(Clone, Debug)]
pub struct VolumeBitmapsForm {
    volume: StorageVolumeSummary,
    bitmaps: Vec<StorageVolumeBitmap>,
    selected: usize,
    create: Option<(String, String, bool, bool, usize)>,
    delete_input: Option<String>,
}

#[derive(Clone, Debug)]
struct VolumeCreateForm {
    name: String,
    project: String,
    content_type: String,
    description: String,
    size: String,
    initial_uid: String,
    initial_gid: String,
    initial_mode: String,
    shared: bool,
    field: usize,
}

#[derive(Clone, Debug)]
struct VolumeEditForm {
    description: String,
    size: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct VolumeImportForm {
    local_path: String,
    project: String,
    name: String,
    import_type: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct VolumeCopyForm {
    target_pool: String,
    target_project: String,
    target_name: String,
    volume_only: bool,
    refresh: bool,
    live: bool,
    refresh_exclude_older: bool,
    supports_live: bool,
    supports_refresh_exclude: bool,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct VolumeFileForm {
    volume: StorageVolumeSummary,
    path: String,
    contents: String,
    entries: Vec<String>,
    selected: usize,
    browsing: bool,
    transfer: Option<(bool, String)>,
    delete_input: Option<String>,
    field: usize,
}

impl VolumeFileForm {
    pub fn volume(&self) -> &StorageVolumeSummary {
        &self.volume
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn contents(&self) -> &str {
        &self.contents
    }
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn browsing(&self) -> bool {
        self.browsing
    }
    pub fn transfer(&self) -> Option<(bool, &str)> {
        self.transfer
            .as_ref()
            .map(|(upload, path)| (*upload, path.as_str()))
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
}

#[derive(Clone, Debug)]
struct VolumeAttachForm {
    instances: Vec<(String, String)>,
    instance: usize,
    device_name: String,
    path: String,
    field: usize,
    target_kind: ResourceKind,
}

#[derive(Clone, Debug)]
struct VolumeDetachForm {
    instances: Vec<(String, String)>,
    instance: usize,
    device_name: String,
    field: usize,
    target_kind: ResourceKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeAttachmentSpec {
    pool: String,
    volume_project: String,
    volume_name: String,
    instance_name: String,
    instance_project: String,
    device_name: String,
    path: String,
    target_kind: ResourceKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeDetachmentSpec {
    pool: String,
    volume_name: String,
    instance_name: String,
    instance_project: String,
    device_name: String,
    target_kind: ResourceKind,
}

#[derive(Clone, Debug)]
pub struct VolumeSnapshotsForm {
    volume: StorageVolumeSummary,
    snapshots: Vec<StorageVolumeSnapshotSummary>,
    selected: usize,
    name_input: Option<(bool, String)>,
    confirmation: Option<(bool, String)>,
}

#[derive(Clone, Debug)]
pub struct VolumeBackupsForm {
    volume: StorageVolumeSummary,
    backups: Vec<BackupSummary>,
    selected: usize,
    name_input: Option<(bool, String)>,
    create_options: (bool, bool),
    delete_input: Option<String>,
    export_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BucketManager {
    pool: String,
    buckets: Vec<StorageBucketSummary>,
    selected: usize,
    editor: Option<BucketEditor>,
    delete_input: Option<String>,
    keys: Option<BucketKeysForm>,
    backups: Option<BucketBackupsForm>,
    import_form: Option<BucketImportForm>,
}

#[derive(Clone, Debug)]
struct BucketEditor {
    editing: bool,
    name: String,
    project: String,
    description: String,
    size: String,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct BucketKeysForm {
    bucket: StorageBucketSummary,
    keys: Vec<StorageBucketKeySummary>,
    selected: usize,
    editor: Option<BucketKeyEditor>,
    delete_input: Option<String>,
    credentials: Option<StorageBucketKeySummary>,
}

#[derive(Clone, Debug)]
pub struct BucketBackupsForm {
    bucket: StorageBucketSummary,
    backups: Vec<BackupSummary>,
    selected: usize,
    name_input: Option<(bool, String)>,
    compression: String,
    delete_input: Option<String>,
    export_path: Option<String>,
}

#[derive(Clone, Debug)]
struct BucketImportForm {
    path: String,
    project: String,
    name: String,
    field: usize,
}

#[derive(Clone, Debug)]
struct BucketKeyEditor {
    original_name: Option<String>,
    name: String,
    description: String,
    role: String,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct NetworkServicesManager {
    network: String,
    project: String,
    leases: Vec<NetworkLease>,
    forwards: Vec<NetworkForwardSummary>,
    acls: Vec<NetworkAclSummary>,
    load_balancers: Vec<NetworkLoadBalancerSummary>,
    peers: Vec<NetworkPeerSummary>,
    tab: NetworkServicesTab,
    selected: usize,
    form: Option<NetworkServiceForm>,
    delete_input: Option<String>,
    acl_rules: Option<AclRulesForm>,
    detail: Option<(String, String)>,
    rename_input: Option<String>,
    load_balancer_history: Vec<LoadBalancerHealthSample>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkServicesTab {
    Leases,
    Forwards,
    Acls,
    LoadBalancers,
    Peers,
}

#[derive(Clone, Debug)]
enum NetworkServiceForm {
    Forward {
        fields: [String; 6],
        field: usize,
        editing: bool,
    },
    Acl {
        name: String,
        description: String,
        field: usize,
    },
    LoadBalancer {
        fields: [String; 12],
        field: usize,
        editing: bool,
    },
    Peer {
        fields: [String; 6],
        field: usize,
        editing: bool,
    },
}

#[derive(Clone, Debug)]
pub struct AclRulesForm {
    acl: NetworkAclSummary,
    ingress: bool,
    selected: usize,
    editor: Option<AclRuleEditor>,
    delete_input: Option<String>,
}
#[derive(Clone, Debug)]
struct AclRuleEditor {
    original: Option<usize>,
    fields: [String; 10],
    field: usize,
}

#[derive(Clone, Debug)]
pub struct ZoneManager {
    zones: Vec<NetworkZoneSummary>,
    selected: usize,
    editor: Option<ZoneEditor>,
    delete_input: Option<String>,
    records: Option<ZoneRecordsForm>,
    rename_input: Option<String>,
}
#[derive(Clone, Debug)]
struct ZoneEditor {
    editing: bool,
    name: String,
    project: String,
    description: String,
    field: usize,
}
#[derive(Clone, Debug)]
pub struct ZoneRecordsForm {
    zone: NetworkZoneSummary,
    records: Vec<NetworkZoneRecordSummary>,
    selected: usize,
    editor: Option<ZoneRecordEditor>,
    delete_input: Option<String>,
}
#[derive(Clone, Debug)]
struct ZoneRecordEditor {
    original: Option<usize>,
    name: String,
    description: String,
    entry_type: String,
    value: String,
    ttl: String,
    field: usize,
}

#[derive(Clone, Debug)]
pub struct BgpForm {
    definition: ResourceDefinition,
    fields: [String; 10],
    field: usize,
}

#[derive(Clone, Debug)]
pub struct AddressSetManager {
    sets: Vec<NetworkAddressSetSummary>,
    selected: usize,
    editor: Option<AddressSetEditor>,
    delete_input: Option<String>,
    rename_input: Option<String>,
}

#[derive(Clone, Debug)]
pub struct IntegrationManager {
    integrations: Vec<NetworkIntegrationSummary>,
    selected: usize,
    editor: Option<IntegrationEditor>,
    delete_input: Option<String>,
    rename_input: Option<String>,
}

#[derive(Clone, Debug)]
struct IntegrationEditor {
    editing: bool,
    fields: [String; 9],
    field: usize,
}
#[derive(Clone, Debug)]
struct AddressSetEditor {
    editing: bool,
    name: String,
    project: String,
    description: String,
    addresses: String,
    field: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsTab {
    Warnings,
    Certificates,
    Cluster,
}

impl SettingsManager {
    pub fn tab(&self) -> SettingsTab {
        self.tab
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn certificate_fields(&self) -> Option<Vec<(&'static str, String, bool, bool)>> {
        self.certificate_form.as_ref().map(|f| {
            let mut fields = vec![
                ("Mode", f.mode.clone(), true, f.field == 0),
                ("Name", f.name.clone(), false, f.field == 1),
                (
                    "Restricted",
                    if f.restricted { "yes" } else { "no" }.to_owned(),
                    true,
                    f.field == 2,
                ),
                ("Projects", f.projects.clone(), false, f.field == 3),
            ];
            if f.mode == "certificate-file" {
                fields.push(("PEM path", f.certificate_path.clone(), false, f.field == 4));
            }
            fields
        })
    }
    pub fn certificate_edit_fields(&self) -> Option<[(&'static str, String, bool, bool); 3]> {
        self.certificate_edit.as_ref().map(|f| {
            [
                ("Name", f.name.clone(), false, f.field == 0),
                (
                    "Restricted",
                    if f.restricted { "yes" } else { "no" }.to_owned(),
                    true,
                    f.field == 1,
                ),
                ("Projects", f.projects.clone(), false, f.field == 2),
            ]
        })
    }
    pub fn trust_token(&self) -> Option<&str> {
        self.trust_token.as_deref()
    }
    pub fn cluster_fields(&self) -> Option<[(&'static str, &str, bool); 4]> {
        self.cluster_form.as_ref().map(|f| {
            [
                ("Description", f.description.as_str(), f.field == 0),
                ("Roles", f.roles.as_str(), f.field == 1),
                ("Groups", f.groups.as_str(), f.field == 2),
                ("Failure domain", f.failure_domain.as_str(), f.field == 3),
            ]
        })
    }
    pub fn cluster_action(&self) -> Option<(&str, &str)> {
        self.cluster_action
            .as_ref()
            .map(|(action, input)| (action.as_str(), input.as_str()))
    }
    pub fn cluster_join_name(&self) -> Option<&str> {
        self.cluster_join_name.as_deref()
    }
    pub fn cluster_rename_input(&self) -> Option<&str> {
        self.cluster_rename_input.as_deref()
    }
    pub fn cluster_detail(&self) -> Option<(&str, &str)> {
        self.cluster_detail
            .as_ref()
            .map(|(title, content)| (title.as_str(), content.as_str()))
    }
    pub fn cluster_config_fields(&self) -> Option<Vec<(&'static str, &str, bool, bool)>> {
        self.cluster_config.as_ref().map(|form| {
            [
                "Server name",
                "Server address",
                "Cluster address",
                "Cluster token",
                "Cluster certificate",
                "Cluster password",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                (
                    label,
                    form.fields[index].as_str(),
                    false,
                    form.field == index,
                )
            })
            .collect()
        })
    }
    pub fn cluster_config_enabled(&self) -> Option<bool> {
        self.cluster_config.as_ref().map(|f| f.enabled)
    }
    pub fn cluster_certificate_fields(&self) -> Option<[(&'static str, &str, bool); 2]> {
        self.cluster_certificate.as_ref().map(|form| {
            [
                (
                    "Certificate PEM",
                    form.certificate_path.as_str(),
                    form.field == 0,
                ),
                ("Private key PEM", form.key_path.as_str(), form.field == 1),
            ]
        })
    }
    pub fn capability_filter(&self) -> Option<&str> {
        self.capability_filter.as_deref()
    }
    pub fn capability_selected(&self) -> usize {
        self.capability_selected
    }
}

impl LoggingTargetManager {
    fn new(definition: ResourceDefinition, supports_webhook: bool) -> Self {
        let mut targets = definition
            .value
            .as_object()
            .into_iter()
            .flat_map(|config| config.keys())
            .filter_map(|key| {
                key.strip_prefix("logging.")
                    .and_then(|rest| rest.split('.').next())
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>();
        targets.sort();
        targets.dedup();
        Self {
            definition,
            targets,
            selected: 0,
            editor: None,
            delete_input: None,
            supports_webhook,
        }
    }

    pub fn targets(&self) -> &[String] {
        &self.targets
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }

    pub fn editor_fields(&self) -> Option<Vec<LoggingFieldView<'_>>> {
        const LABELS: [&str; 13] = [
            "Name",
            "Target type",
            "Address",
            "Username",
            "New password",
            "CA certificate PEM",
            "Syslog facility",
            "Loki instance",
            "Loki labels",
            "Delivery retries",
            "Event types",
            "Minimum log level",
            "Lifecycle projects/types",
        ];
        self.editor.as_ref().map(|form| {
            LABELS
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    (
                        label,
                        form.values[index].as_str(),
                        matches!(index, 1 | 11),
                        index == form.field,
                        index == 4,
                    )
                })
                .collect()
        })
    }

    fn value_for(definition: &ResourceDefinition, form: &LoggingTargetForm) -> serde_json::Value {
        let mut value = definition.value.clone();
        let config = value
            .as_object_mut()
            .expect("server definition is a configuration object");
        if let Some(original) = &form.original_name {
            let prefix = format!("logging.{original}.");
            config.retain(|key, _| !key.starts_with(&prefix));
        }
        let name = form.values[0].trim();
        let keys = [
            "target.type",
            "target.address",
            "target.username",
            "target.password",
            "target.ca_cert",
            "target.facility",
            "target.instance",
            "target.labels",
            "target.retry",
            "types",
            "logging.level",
        ];
        for (key, field) in keys.into_iter().zip(1..=11) {
            let content = form.values[field].trim();
            if !content.is_empty() {
                config.insert(
                    format!("logging.{name}.{key}"),
                    serde_json::Value::String(content.to_owned()),
                );
            } else if key == "target.password"
                && let Some(original) = &form.original_name
                && let Some(password) = definition
                    .value
                    .get(format!("logging.{original}.target.password"))
            {
                config.insert(format!("logging.{name}.{key}"), password.clone());
            }
        }
        let (projects, lifecycle_types) = form.values[12]
            .split_once(';')
            .unwrap_or((form.values[12].as_str(), ""));
        for (key, content) in [
            ("lifecycle.projects", projects.trim()),
            ("lifecycle.types", lifecycle_types.trim()),
        ] {
            if !content.is_empty() {
                config.insert(
                    format!("logging.{name}.{key}"),
                    serde_json::Value::String(content.to_owned()),
                );
            }
        }
        value
    }
}

impl LoggingTargetForm {
    fn new(manager: &LoggingTargetManager, name: Option<&str>) -> Self {
        let get = |suffix: &str| {
            name.and_then(|name| {
                manager
                    .definition
                    .value
                    .get(format!("logging.{name}.{suffix}"))
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or_default()
            .to_owned()
        };
        let projects = get("lifecycle.projects");
        let lifecycle_types = get("lifecycle.types");
        Self {
            original_name: name.map(ToOwned::to_owned),
            values: [
                name.unwrap_or_default().to_owned(),
                get("target.type"),
                get("target.address"),
                get("target.username"),
                String::new(),
                get("target.ca_cert"),
                get("target.facility"),
                get("target.instance"),
                get("target.labels"),
                get("target.retry"),
                get("types"),
                get("logging.level"),
                format!("{projects};{lifecycle_types}"),
            ],
            field: 0,
        }
    }
}

impl ScriptletManager {
    fn new(
        definition: ResourceDefinition,
        supports_placement: bool,
        supports_authorization: bool,
    ) -> Self {
        let kind = if supports_placement {
            ScriptletKind::Placement
        } else {
            ScriptletKind::Authorization
        };
        let original = config_value(&definition, kind.key());
        let cursor = original.len();
        Self {
            definition,
            kind,
            draft: original.clone(),
            original,
            cursor,
            validation: Vec::new(),
            activation_input: None,
            rollback: false,
            supports_placement,
            supports_authorization,
        }
    }

    fn new_qemu(definition: ResourceDefinition) -> Self {
        let kind = ScriptletKind::Qemu;
        let original = config_value(&definition, kind.key());
        let cursor = original.len();
        Self {
            definition,
            kind,
            draft: original.clone(),
            original,
            cursor,
            validation: Vec::new(),
            activation_input: None,
            rollback: false,
            supports_placement: false,
            supports_authorization: false,
        }
    }

    pub fn kind(&self) -> &'static str {
        self.kind.label()
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn changed(&self) -> bool {
        self.draft != self.original
    }

    pub fn validation(&self) -> &[String] {
        &self.validation
    }

    pub fn activation_input(&self) -> Option<&str> {
        self.activation_input.as_deref()
    }

    pub fn activation_action(&self) -> &'static str {
        if self.rollback {
            "rollback"
        } else {
            "activate"
        }
    }

    fn switch_kind(&mut self) {
        self.kind = match self.kind {
            ScriptletKind::Placement if self.supports_authorization => ScriptletKind::Authorization,
            ScriptletKind::Authorization if self.supports_placement => ScriptletKind::Placement,
            ScriptletKind::Qemu => ScriptletKind::Qemu,
            current => current,
        };
        self.original = config_value(&self.definition, self.kind.key());
        self.draft = self.original.clone();
        self.cursor = self.draft.len();
        self.validation.clear();
        self.activation_input = None;
        self.rollback = false;
    }

    fn template(&mut self) {
        self.draft = match self.kind {
            ScriptletKind::Placement => concat!(
                "def instance_placement(request, candidate_members):\n",
                "    if len(candidate_members) == 0:\n",
                "        fail(\"No candidate cluster members\")\n",
                "    set_target(candidate_members[0].server_name)\n",
                "    return\n"
            ),
            ScriptletKind::Authorization => concat!(
                "def authorize(details, object, entitlement):\n",
                "    # Start deny-by-default; add explicit policy above this return.\n",
                "    return False\n"
            ),
            ScriptletKind::Qemu => concat!(
                "def qemu_hook(instance, stage):\n",
                "    # Supported stages: config, early, pre-start, post-start.\n",
                "    log_info(\"QEMU hook stage: \", stage)\n",
                "    return\n"
            ),
        }
        .to_owned();
        self.cursor = self.draft.len();
        self.validation.clear();
    }

    fn validate(&mut self) -> bool {
        self.validation.clear();
        if self.draft.trim().is_empty() {
            self.validation.push("ERROR: scriptlet is empty".into());
        }
        if !self.draft.contains(self.kind.required_function()) {
            self.validation.push(format!(
                "ERROR: required entry point is `{}`",
                self.kind.required_function()
            ));
        }
        if self.draft.contains('\t') {
            self.validation
                .push("ERROR: tabs are forbidden; use spaces for Starlark indentation".into());
        }
        for (open, close, name) in [
            ('(', ')', "parentheses"),
            ('[', ']', "brackets"),
            ('{', '}', "braces"),
        ] {
            let mut depth = 0_i32;
            for character in self.draft.chars() {
                if character == open {
                    depth += 1;
                } else if character == close {
                    depth -= 1;
                }
                if depth < 0 {
                    break;
                }
            }
            if depth != 0 {
                self.validation.push(format!("ERROR: unbalanced {name}"));
            }
        }
        if !self
            .validation
            .iter()
            .any(|line| line.starts_with("ERROR:"))
        {
            self.validation.push(
                "PASS: local structure checks passed; Incus performs authoritative validation during activation"
                    .into(),
            );
            true
        } else {
            false
        }
    }

    fn value(&self, source: &str) -> serde_json::Value {
        let mut value = self.definition.value.clone();
        let config = if self.definition.kind == ResourceKind::Server {
            value.as_object_mut().expect("server config")
        } else {
            value["config"]
                .as_object_mut()
                .expect("instance or profile config")
        };
        if source.is_empty() {
            config.remove(self.kind.key());
        } else {
            config.insert(
                self.kind.key().to_owned(),
                serde_json::Value::String(source.to_owned()),
            );
        }
        value
    }
}

pub type PreseedFieldView = (&'static str, String, bool, bool);

impl PreseedManager {
    const FIELD_COUNT: usize = 26;

    fn new() -> Self {
        Self {
            spec: PreseedSpec::default(),
            field: 0,
            preview: false,
            transfer: None,
            apply_input: None,
            results: Vec::new(),
        }
    }

    pub fn fields(&self) -> Vec<PreseedFieldView> {
        let enabled = |value| if value { "yes" } else { "no" }.to_owned();
        vec![
            (
                "Apply server section",
                enabled(self.spec.server_enabled),
                true,
                self.field == 0,
            ),
            (
                "HTTPS address",
                self.spec.https_address.clone(),
                false,
                self.field == 1,
            ),
            (
                "Metrics address",
                self.spec.metrics_address.clone(),
                false,
                self.field == 2,
            ),
            (
                "Apply storage section",
                enabled(self.spec.storage_enabled),
                true,
                self.field == 3,
            ),
            (
                "Pool name",
                self.spec.pool_name.clone(),
                false,
                self.field == 4,
            ),
            (
                "Pool driver",
                self.spec.pool_driver.clone(),
                true,
                self.field == 5,
            ),
            (
                "Pool source",
                self.spec.pool_source.clone(),
                false,
                self.field == 6,
            ),
            (
                "Apply network section",
                enabled(self.spec.network_enabled),
                true,
                self.field == 7,
            ),
            (
                "Network name",
                self.spec.network_name.clone(),
                false,
                self.field == 8,
            ),
            (
                "Network type",
                self.spec.network_type.clone(),
                true,
                self.field == 9,
            ),
            (
                "IPv4 address",
                self.spec.ipv4_address.clone(),
                false,
                self.field == 10,
            ),
            (
                "IPv6 address",
                self.spec.ipv6_address.clone(),
                false,
                self.field == 11,
            ),
            (
                "Apply profile section",
                enabled(self.spec.profile_enabled),
                true,
                self.field == 12,
            ),
            (
                "Profile name",
                self.spec.profile_name.clone(),
                false,
                self.field == 13,
            ),
            (
                "Profile CPU",
                self.spec.profile_cpu.clone(),
                false,
                self.field == 14,
            ),
            (
                "Profile memory",
                self.spec.profile_memory.clone(),
                false,
                self.field == 15,
            ),
            (
                "Join/configure cluster",
                enabled(self.spec.cluster_enabled),
                true,
                self.field == 16,
            ),
            (
                "Cluster server name",
                self.spec.cluster_server_name.clone(),
                false,
                self.field == 17,
            ),
            (
                "Cluster server address",
                self.spec.cluster_server_address.clone(),
                false,
                self.field == 18,
            ),
            (
                "Cluster address",
                self.spec.cluster_address.clone(),
                false,
                self.field == 19,
            ),
            (
                "Cluster token",
                self.spec.cluster_token.clone(),
                false,
                self.field == 20,
            ),
            (
                "Cluster certificate path",
                self.spec.cluster_certificate_path.clone(),
                false,
                self.field == 21,
            ),
            (
                "Legacy cluster password",
                self.spec.cluster_password.clone(),
                false,
                self.field == 22,
            ),
            (
                "Cluster group after join",
                self.spec.cluster_group.clone(),
                false,
                self.field == 23,
            ),
            (
                "Profile root pool",
                self.spec.profile_pool.clone(),
                true,
                self.field == 24,
            ),
            (
                "Profile NIC network",
                self.spec.profile_network.clone(),
                true,
                self.field == 25,
            ),
        ]
    }

    pub fn preview(&self) -> bool {
        self.preview
    }

    pub fn results(&self) -> &[String] {
        &self.results
    }

    pub fn transfer(&self) -> Option<(bool, &str)> {
        self.transfer
            .as_ref()
            .map(|(import, path)| (*import, path.as_str()))
    }
    pub fn apply_input(&self) -> Option<&str> {
        self.apply_input.as_deref()
    }

    pub fn staged_entries(&self) -> Vec<String> {
        let mut rows = self
            .spec
            .additional_pools
            .iter()
            .map(|item| format!("pool     {:<18} {}", item.name, item.driver))
            .collect::<Vec<_>>();
        rows.extend(
            self.spec
                .additional_networks
                .iter()
                .map(|item| format!("network  {:<18} {}", item.name, item.network_type)),
        );
        rows.extend(self.spec.additional_profiles.iter().map(|item| {
            format!(
                "profile  {:<18} pool={} network={}",
                item.name, item.pool, item.network
            )
        }));
        rows
    }

    fn stage_current(&mut self) -> Result<String, String> {
        match self.field {
            3..=6 => {
                if self.spec.pool_name.trim().is_empty() {
                    return Err("pool name is required before staging".into());
                }
                let item = PreseedPoolSpec {
                    name: self.spec.pool_name.trim().into(),
                    driver: self.spec.pool_driver.clone(),
                    source: self.spec.pool_source.trim().into(),
                };
                self.spec
                    .additional_pools
                    .retain(|entry| entry.name != item.name);
                self.spec.additional_pools.push(item.clone());
                self.spec.pool_name.clear();
                self.spec.pool_source.clear();
                Ok(format!("staged pool {}", item.name))
            }
            7..=11 => {
                if self.spec.network_name.trim().is_empty() {
                    return Err("network name is required before staging".into());
                }
                let item = PreseedNetworkSpec {
                    name: self.spec.network_name.trim().into(),
                    network_type: self.spec.network_type.clone(),
                    ipv4_address: self.spec.ipv4_address.clone(),
                    ipv6_address: self.spec.ipv6_address.clone(),
                };
                self.spec
                    .additional_networks
                    .retain(|entry| entry.name != item.name);
                self.spec.additional_networks.push(item.clone());
                self.spec.network_name.clear();
                Ok(format!("staged network {}", item.name))
            }
            12..=15 | 24 | 25 => {
                if self.spec.profile_name.trim().is_empty() {
                    return Err("profile name is required before staging".into());
                }
                let item = PreseedProfileSpec {
                    name: self.spec.profile_name.trim().into(),
                    cpu: self.spec.profile_cpu.trim().into(),
                    memory: self.spec.profile_memory.trim().into(),
                    pool: if !self.spec.profile_pool.trim().is_empty() {
                        self.spec.profile_pool.trim().into()
                    } else if self.spec.pool_name.trim().is_empty() {
                        self.spec
                            .additional_pools
                            .last()
                            .map(|item| item.name.clone())
                            .unwrap_or_default()
                    } else {
                        self.spec.pool_name.trim().into()
                    },
                    network: if !self.spec.profile_network.trim().is_empty() {
                        self.spec.profile_network.trim().into()
                    } else if self.spec.network_name.trim().is_empty() {
                        self.spec
                            .additional_networks
                            .last()
                            .map(|item| item.name.clone())
                            .unwrap_or_default()
                    } else {
                        self.spec.network_name.trim().into()
                    },
                };
                self.spec
                    .additional_profiles
                    .retain(|entry| entry.name != item.name);
                self.spec.additional_profiles.push(item.clone());
                self.spec.profile_name.clear();
                self.spec.profile_cpu.clear();
                self.spec.profile_memory.clear();
                Ok(format!("staged profile {}", item.name))
            }
            _ => Err("focus a pool, network, or profile field before staging".into()),
        }
    }

    fn remove_last_staged(&mut self) -> Result<String, String> {
        let removed = match self.field {
            3..=6 => self
                .spec
                .additional_pools
                .pop()
                .map(|item| format!("pool {}", item.name)),
            7..=11 => self
                .spec
                .additional_networks
                .pop()
                .map(|item| format!("network {}", item.name)),
            12..=15 | 24 | 25 => self
                .spec
                .additional_profiles
                .pop()
                .map(|item| format!("profile {}", item.name)),
            _ => None,
        };
        removed
            .map(|item| format!("removed staged {item}"))
            .ok_or_else(|| "no staged entry in the focused section".into())
    }

    fn validate(&self) -> Result<(), String> {
        if !self.spec.server_enabled
            && !self.spec.storage_enabled
            && !self.spec.network_enabled
            && !self.spec.profile_enabled
            && !self.spec.cluster_enabled
        {
            return Err("select at least one preseed section".into());
        }
        if self.spec.storage_enabled
            && self.spec.pool_name.trim().is_empty()
            && self.spec.additional_pools.is_empty()
        {
            return Err("storage pool name is required".into());
        }
        if self.spec.network_enabled
            && self.spec.network_name.trim().is_empty()
            && self.spec.additional_networks.is_empty()
        {
            return Err("network name is required".into());
        }
        if self.spec.profile_enabled
            && self.spec.profile_name.trim().is_empty()
            && self.spec.additional_profiles.is_empty()
        {
            return Err("profile name is required".into());
        }
        if self.spec.cluster_enabled
            && (self.spec.cluster_server_name.trim().is_empty()
                || self.spec.cluster_address.trim().is_empty())
        {
            return Err("cluster server name and cluster address are required".into());
        }
        Ok(())
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.field {
            1 => Some(&mut self.spec.https_address),
            2 => Some(&mut self.spec.metrics_address),
            4 => Some(&mut self.spec.pool_name),
            6 => Some(&mut self.spec.pool_source),
            8 => Some(&mut self.spec.network_name),
            10 => Some(&mut self.spec.ipv4_address),
            11 => Some(&mut self.spec.ipv6_address),
            13 => Some(&mut self.spec.profile_name),
            14 => Some(&mut self.spec.profile_cpu),
            15 => Some(&mut self.spec.profile_memory),
            17 => Some(&mut self.spec.cluster_server_name),
            18 => Some(&mut self.spec.cluster_server_address),
            19 => Some(&mut self.spec.cluster_address),
            20 => Some(&mut self.spec.cluster_token),
            21 => Some(&mut self.spec.cluster_certificate_path),
            22 => Some(&mut self.spec.cluster_password),
            23 => Some(&mut self.spec.cluster_group),
            _ => None,
        }
    }

    fn cycle(&mut self, backwards: bool) {
        match self.field {
            0 => self.spec.server_enabled = !self.spec.server_enabled,
            3 => self.spec.storage_enabled = !self.spec.storage_enabled,
            5 => {
                let choices = ["dir", "zfs", "btrfs", "lvm", "ceph", "linstor"];
                cycle_string(&mut self.spec.pool_driver, &choices, backwards);
            }
            7 => self.spec.network_enabled = !self.spec.network_enabled,
            9 => {
                let choices = ["bridge", "ovn", "physical", "macvlan", "sriov"];
                cycle_string(&mut self.spec.network_type, &choices, backwards);
            }
            12 => self.spec.profile_enabled = !self.spec.profile_enabled,
            16 => self.spec.cluster_enabled = !self.spec.cluster_enabled,
            24 => {
                let mut choices = self
                    .spec
                    .additional_pools
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>();
                if !self.spec.pool_name.trim().is_empty() {
                    choices.insert(0, self.spec.pool_name.trim());
                }
                if !choices.is_empty() {
                    cycle_string(&mut self.spec.profile_pool, &choices, backwards);
                }
            }
            25 => {
                let mut choices = self
                    .spec
                    .additional_networks
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>();
                if !self.spec.network_name.trim().is_empty() {
                    choices.insert(0, self.spec.network_name.trim());
                }
                if !choices.is_empty() {
                    cycle_string(&mut self.spec.profile_network, &choices, backwards);
                }
            }
            _ => {}
        }
    }
}

fn cycle_string(value: &mut String, choices: &[&str], backwards: bool) {
    let current = choices.iter().position(|choice| *choice == value);
    let next = match (current, backwards) {
        (None, false) => 0,
        (None, true) => choices.len() - 1,
        (Some(current), true) => (current + choices.len() - 1) % choices.len(),
        (Some(current), false) => (current + 1) % choices.len(),
    };
    *value = choices[next].to_owned();
}

impl PoolSafetyManager {
    fn new(definition: ResourceDefinition, resources: serde_json::Value, driver: String) -> Self {
        let new_size = config_value(&definition, "size");
        Self {
            definition,
            resources,
            driver,
            tab: PoolSafetyTab::Growth,
            new_size,
            confirmation: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn driver(&self) -> &str {
        &self.driver
    }

    pub fn tab(&self) -> PoolSafetyTab {
        self.tab
    }

    pub fn old_size(&self) -> String {
        config_value(&self.definition, "size")
    }

    pub fn source(&self) -> String {
        config_value(&self.definition, "source")
    }

    pub fn new_size(&self) -> &str {
        &self.new_size
    }

    pub fn confirmation(&self) -> Option<&str> {
        self.confirmation.as_deref()
    }

    pub fn health_rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            ("Driver".into(), self.driver.clone()),
            ("Source".into(), self.source()),
            ("Configured size".into(), self.old_size()),
        ];
        for (label, pointers) in [
            ("Space total", &["/space/total", "/space/total_bytes"][..]),
            ("Space used", &["/space/used", "/space/used_bytes"][..]),
            ("Inodes total", &["/inodes/total"][..]),
            ("Inodes used", &["/inodes/used"][..]),
        ] {
            if let Some(value) = pointers
                .iter()
                .find_map(|pointer| self.resources.pointer(pointer))
            {
                rows.push((label.into(), display_json_scalar(value)));
            }
        }
        let driver_keys: &[&str] = match self.driver.as_str() {
            "zfs" => &["zfs.pool_name", "zfs.vdev", "zfs.clone_copy"],
            "btrfs" => &["btrfs.mount_options"],
            "lvm" => &["lvm.vg_name", "lvm.thinpool_name", "lvm.metadata_size"],
            "ceph" | "cephfs" => &["ceph.cluster_name", "ceph.user.name", "ceph.osd.pool_name"],
            "linstor" => &["linstor.resource_group"],
            _ => &[],
        };
        rows.extend(driver_keys.iter().filter_map(|key| {
            let value = config_value(&self.definition, key);
            (!value.is_empty()).then(|| ((*key).to_owned(), value))
        }));
        rows
    }

    fn growth_value(&self) -> Result<serde_json::Value, String> {
        let old = parse_storage_size(&self.old_size()).ok_or_else(|| {
            "current pool size is unavailable or not a sized loop pool".to_owned()
        })?;
        let new = parse_storage_size(self.new_size.trim())
            .ok_or_else(|| "new size must use bytes or a KiB/MiB/GiB/TiB suffix".to_owned())?;
        if new <= old {
            return Err(format!(
                "new size must exceed current size ({})",
                self.old_size()
            ));
        }
        let mut value = self.definition.value.clone();
        value["config"]["size"] = serde_json::Value::String(self.new_size.trim().into());
        Ok(value)
    }
}

impl QmpManager {
    const STAGES: [&'static str; 3] = ["early", "pre-start", "post-start"];
    const COMMANDS: [&'static str; 9] = [
        "query-status",
        "query-block",
        "query-pci",
        "query-cpus-fast",
        "query-memory-size-summary",
        "stop",
        "cont",
        "system_reset",
        "system_powerdown",
    ];

    fn new(definition: ResourceDefinition) -> Self {
        let stage = "pre-start".to_owned();
        let commands = qmp_commands(&definition, &stage);
        Self {
            definition,
            stage,
            command: "query-status".into(),
            argument_name: String::new(),
            argument_value: String::new(),
            field: 0,
            commands,
            selected: 0,
            confirmation: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }
    pub fn stage(&self) -> &str {
        &self.stage
    }
    pub fn command(&self) -> &str {
        &self.command
    }
    pub fn argument_name(&self) -> &str {
        &self.argument_name
    }
    pub fn argument_value(&self) -> &str {
        &self.argument_value
    }
    pub fn field(&self) -> usize {
        self.field
    }
    pub fn commands(&self) -> &[serde_json::Value] {
        &self.commands
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn confirmation(&self) -> Option<&str> {
        self.confirmation.as_deref()
    }

    fn cycle_stage(&mut self, backwards: bool) {
        cycle_string(&mut self.stage, &Self::STAGES, backwards);
        self.commands = qmp_commands(&self.definition, &self.stage);
        self.selected = 0;
    }

    fn cycle_command(&mut self, backwards: bool) {
        cycle_string(&mut self.command, &Self::COMMANDS, backwards);
    }

    fn add_command(&mut self) -> Result<(), String> {
        if !Self::COMMANDS.contains(&self.command.as_str()) {
            return Err("QMP command is not in the virtty allow-list".into());
        }
        let mut command = serde_json::json!({"execute":self.command});
        if !self.argument_name.trim().is_empty() {
            let scalar = serde_json::from_str(self.argument_value.trim())
                .unwrap_or_else(|_| serde_json::Value::String(self.argument_value.trim().into()));
            let mut arguments = serde_json::Map::new();
            arguments.insert(self.argument_name.trim().into(), scalar);
            command["arguments"] = serde_json::Value::Object(arguments);
        }
        self.commands.push(command);
        self.selected = self.commands.len().saturating_sub(1);
        self.argument_name.clear();
        self.argument_value.clear();
        Ok(())
    }

    fn value(&self) -> serde_json::Value {
        let mut value = self.definition.value.clone();
        let key = format!("raw.qemu.qmp.{}", self.stage);
        let config = value["config"].as_object_mut().expect("instance config");
        if self.commands.is_empty() {
            config.remove(&key);
        } else {
            config.insert(
                key,
                serde_json::Value::String(
                    serde_json::to_string_pretty(&self.commands).expect("QMP commands serialize"),
                ),
            );
        }
        value
    }
}

fn qmp_commands(definition: &ResourceDefinition, stage: &str) -> Vec<serde_json::Value> {
    let key = format!("raw.qemu.qmp.{stage}");
    config_value(definition, &key)
        .parse::<serde_json::Value>()
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

impl BulkManager {
    fn new(instances: &[crate::incus::InstanceSummary]) -> Self {
        Self {
            targets: instances
                .iter()
                .map(|instance| BulkTarget {
                    name: instance.name.clone(),
                    project: instance.project.clone(),
                    status: instance.status.clone(),
                    selected: false,
                })
                .collect(),
            selected: 0,
            action: InstanceAction::Start,
            concurrency: 4,
            dry_run: true,
            confirmation: None,
            results: Vec::new(),
            failed: Vec::new(),
            running: false,
            completed: 0,
            total: 0,
            cancelled: false,
        }
    }

    pub fn targets(&self) -> &[BulkTarget] {
        &self.targets
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn action(&self) -> InstanceAction {
        self.action
    }
    pub fn concurrency(&self) -> usize {
        self.concurrency
    }
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    pub fn confirmation(&self) -> Option<&str> {
        self.confirmation.as_deref()
    }
    pub fn results(&self) -> &[String] {
        &self.results
    }
    pub fn running(&self) -> bool {
        self.running
    }
    pub fn completed(&self) -> usize {
        self.completed
    }
    pub fn total(&self) -> usize {
        self.total
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn selected_count(&self) -> usize {
        self.targets.iter().filter(|target| target.selected).count()
    }
    pub fn eligible_count(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| target.selected && action_is_allowed(&target.status, self.action))
            .count()
    }
    fn selected_targets(&self) -> Vec<BulkTarget> {
        self.targets
            .iter()
            .filter(|target| target.selected)
            .cloned()
            .collect()
    }
    fn cycle_action(&mut self, backwards: bool) {
        let actions = [
            InstanceAction::Start,
            InstanceAction::Stop,
            InstanceAction::Restart,
            InstanceAction::Freeze,
            InstanceAction::Unfreeze,
            InstanceAction::Delete,
        ];
        let current = actions
            .iter()
            .position(|action| *action == self.action)
            .unwrap_or(0);
        let next = if backwards {
            (current + actions.len() - 1) % actions.len()
        } else {
            (current + 1) % actions.len()
        };
        self.action = actions[next];
    }
}

impl BulkTarget {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn selected(&self) -> bool {
        self.selected
    }
}

fn parse_storage_size(value: &str) -> Option<u128> {
    let trimmed = value.trim();
    let split = trimmed
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let number = trimmed[..split].parse::<u128>().ok()?;
    let multiplier = match trimmed[split..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kib" => 1024,
        "mib" => 1024_u128.pow(2),
        "gib" => 1024_u128.pow(3),
        "tib" => 1024_u128.pow(4),
        "kb" => 1000,
        "mb" => 1000_u128.pow(2),
        "gb" => 1000_u128.pow(3),
        "tb" => 1000_u128.pow(4),
        _ => return None,
    };
    number.checked_mul(multiplier)
}

fn parse_rfc3339_timestamp(value: &str) -> Option<i64> {
    if value.len() < 20 {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value
            .as_bytes()
            .get(10)
            .is_none_or(|separator| !matches!(separator, b'T' | b't' | b' '))
        || value.as_bytes().get(13) != Some(&b':')
        || value.as_bytes().get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut zone = 19;
    if value.as_bytes().get(zone) == Some(&b'.') {
        zone += 1;
        while value.as_bytes().get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
    }
    let offset = match value.as_bytes().get(zone).copied()? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let hours = value.get(zone + 1..zone + 3)?.parse::<i64>().ok()?;
            let minutes = value.get(zone + 4..zone + 6)?.parse::<i64>().ok()?;
            if value.as_bytes().get(zone + 3) != Some(&b':') || hours > 23 || minutes > 59 {
                return None;
            }
            (hours * 3600 + minutes * 60) * if sign == b'+' { 1 } else { -1 }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    Some(days * 86400 + hour * 3600 + minute * 60 + second - offset)
}

fn redact_sensitive(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                let key = key.to_ascii_lowercase();
                if ["password", "secret", "token", "private_key", "certificate"]
                    .iter()
                    .any(|needle| key.contains(needle))
                {
                    *value = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_sensitive(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_sensitive),
        _ => {}
    }
}

fn build_audit_export(model: &AppModel, form: &AuditExportForm) -> serde_json::Value {
    let filter = form.filter.to_ascii_lowercase();
    let events = if form.include_events {
        model.events.iter().filter(|event| filter.is_empty() || event.summary().to_ascii_lowercase().contains(&filter) || event.project.to_ascii_lowercase().contains(&filter)).map(|event| serde_json::json!({"type":event.event_type,"timestamp":event.timestamp,"project":event.project,"location":event.location,"summary":event.summary(),"metadata":event.metadata})).collect()
    } else {
        Vec::<serde_json::Value>::new()
    };
    let operations = if form.include_operations {
        model.operations.iter().filter(|operation| filter.is_empty() || operation.id.to_ascii_lowercase().contains(&filter) || operation.description.to_ascii_lowercase().contains(&filter) || operation.status.to_ascii_lowercase().contains(&filter)).map(|operation| serde_json::json!({"id":operation.id,"class":operation.class,"description":operation.description,"created_at":operation.created_at,"updated_at":operation.updated_at,"status":operation.status,"error":operation.err,"location":operation.location,"resources":operation.resources,"metadata":operation.metadata})).collect()
    } else {
        Vec::<serde_json::Value>::new()
    };
    let mut value = serde_json::json!({"schema":"virtty-audit-v1","filter":form.filter,"redacted":form.redact,"events":events,"operations":operations});
    if form.redact {
        redact_sensitive(&mut value);
    }
    value
}

fn display_json_scalar(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn format_schedule_interval(seconds: u64) -> &'static str {
    match seconds {
        3600 => "hourly",
        21600 => "every 6 hours",
        86400 => "daily",
        604800 => "weekly",
        _ => "daily",
    }
}

fn format_load_balancer_state(state: &serde_json::Value) -> String {
    let backends = state
        .get("backends")
        .or_else(|| state.get("backend_state"))
        .and_then(serde_json::Value::as_array);
    let Some(backends) = backends else {
        let summary = ["status", "health", "message"]
            .into_iter()
            .filter_map(|key| {
                state
                    .get(key)
                    .map(|value| format!("{key}: {}", display_json_scalar(value)))
            })
            .collect::<Vec<_>>()
            .join("\n");
        return if summary.is_empty() {
            "No backend health reported".into()
        } else {
            summary
        };
    };
    if backends.is_empty() {
        return "No backends reported".into();
    }
    backends
        .iter()
        .enumerate()
        .map(|(index, backend)| {
            let value = |keys: &[&str]| {
                keys.iter()
                    .find_map(|key| backend.get(*key))
                    .map(display_json_scalar)
                    .unwrap_or_default()
            };
            let name = value(&["name", "backend", "address"]);
            let status = value(&["status", "health", "state"]);
            let message = value(&["message", "reason", "last_error"]);
            format!(
                "{:<3} {:<24} {:<12} {}",
                index + 1,
                if name.is_empty() { "unnamed" } else { &name },
                if status.is_empty() {
                    "unknown"
                } else {
                    &status
                },
                message
            )
            .trim_end()
            .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_network_state(state: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    for (label, pointers) in [
        ("State", &["/state"][..]),
        ("Type", &["/type"][..]),
        ("MAC address", &["/hwaddr", "/hardware_address"][..]),
        ("MTU", &["/mtu"][..]),
    ] {
        if let Some(value) = pointers.iter().find_map(|pointer| state.pointer(pointer)) {
            lines.push(format!("{label:<20} {}", display_json_scalar(value)));
        }
    }
    if let Some(addresses) = state.get("addresses").and_then(serde_json::Value::as_array) {
        lines.push(String::new());
        lines.push("Addresses".into());
        for address in addresses {
            let family = address
                .get("family")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let value = address
                .get("address")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let netmask = address
                .get("netmask")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let scope = address
                .get("scope")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            lines.push(format!("  {family:<6} {value}/{netmask}  {scope}"));
        }
    }
    if let Some(counters) = state.get("counters").and_then(serde_json::Value::as_object) {
        lines.push(String::new());
        lines.push("Counters".into());
        for key in [
            "bytes_received",
            "bytes_sent",
            "packets_received",
            "packets_sent",
            "errors_received",
            "errors_sent",
            "packets_dropped_inbound",
            "packets_dropped_outbound",
        ] {
            if let Some(value) = counters.get(key) {
                lines.push(format!("  {key:<28} {}", display_json_scalar(value)));
            }
        }
    }
    for (section, label) in [
        ("bond", "Bond / LACP"),
        ("bridge", "Bridge / FDB"),
        ("vlan", "VLAN"),
        ("ovn", "OVN router / chassis"),
        ("routes", "Routes"),
    ] {
        if let Some(value) = state.get(section).filter(|value| !value.is_null()) {
            lines.push(String::new());
            lines.push(label.into());
            match value {
                serde_json::Value::Object(object) => {
                    for (key, value) in object {
                        lines.push(format!("  {key:<24} {}", display_json_scalar(value)));
                    }
                }
                serde_json::Value::Array(values) => {
                    lines.extend(
                        values
                            .iter()
                            .map(|value| format!("  {}", display_json_scalar(value))),
                    );
                }
                _ => lines.push(format!("  {}", display_json_scalar(value))),
            }
        }
    }
    if lines.is_empty() {
        "No structured network state was reported by the server.".into()
    } else {
        lines.join("\n")
    }
}

impl DeviceManager {
    fn new(definition: ResourceDefinition) -> Self {
        let devices = definition
            .value
            .get("devices")
            .and_then(serde_json::Value::as_object)
            .map(|devices| {
                devices
                    .iter()
                    .map(|(name, value)| {
                        let config = value
                            .as_object()
                            .map(|config| {
                                config
                                    .iter()
                                    .filter_map(|(key, value)| {
                                        value.as_str().map(|value| (key.clone(), value.to_owned()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (name.clone(), config)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            definition,
            devices,
            selected: 0,
            editor: None,
            delete_input: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn kind(&self) -> ResourceKind {
        self.definition.kind
    }

    pub fn project(&self) -> &str {
        &self.definition.project
    }

    pub fn devices(&self) -> &[(String, std::collections::BTreeMap<String, String>)] {
        &self.devices
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn editor_fields(&self) -> Option<impl Iterator<Item = (&'static str, &str, bool, bool)>> {
        self.editor.as_ref().map(|editor| {
            editor.fields.iter().enumerate().map(|(index, field)| {
                (
                    field.label,
                    field.value.as_str(),
                    !field.choices.is_empty(),
                    index == editor.field,
                )
            })
        })
    }

    pub fn editing(&self) -> bool {
        self.editor.is_some()
    }

    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }

    pub fn delete_phrase(&self) -> Option<String> {
        self.devices
            .get(self.selected)
            .map(|(name, _)| format!("delete device {name}"))
    }
    fn definition_with_devices(&self) -> ResourceDefinition {
        let mut definition = self.definition.clone();
        let devices = self
            .devices
            .iter()
            .map(|(name, config)| {
                let config = config
                    .iter()
                    .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                    .collect();
                (name.clone(), serde_json::Value::Object(config))
            })
            .collect();
        definition.value["devices"] = serde_json::Value::Object(devices);
        definition
    }
}

impl DeviceForm {
    fn new(
        name: Option<String>,
        config: Option<&std::collections::BTreeMap<String, String>>,
    ) -> Self {
        let value = |key: &str| {
            config
                .and_then(|config| config.get(key))
                .cloned()
                .unwrap_or_default()
        };
        let fields = vec![
            DeviceField {
                label: "Name",
                key: "name",
                value: name.clone().unwrap_or_default(),
                choices: &[],
            },
            DeviceField {
                label: "Type",
                key: "type",
                value: {
                    let current = value("type");
                    if current.is_empty() {
                        "disk".to_owned()
                    } else {
                        current
                    }
                },
                choices: &[
                    "disk",
                    "nic",
                    "gpu",
                    "usb",
                    "pci",
                    "tpm",
                    "infiniband",
                    "unix-char",
                    "unix-block",
                    "proxy",
                ],
            },
            DeviceField {
                label: "Source",
                key: "source",
                value: value("source"),
                choices: &[],
            },
            DeviceField {
                label: "Path",
                key: "path",
                value: value("path"),
                choices: &[],
            },
            DeviceField {
                label: "Pool",
                key: "pool",
                value: value("pool"),
                choices: &[],
            },
            DeviceField {
                label: "Network",
                key: "network",
                value: value("network"),
                choices: &[],
            },
            DeviceField {
                label: "Parent",
                key: "parent",
                value: value("parent"),
                choices: &[],
            },
            DeviceField {
                label: "NIC mode",
                key: "nictype",
                value: value("nictype"),
                choices: &["", "bridged", "macvlan", "physical", "routed", "sriov"],
            },
            DeviceField {
                label: "Vendor ID",
                key: "vendorid",
                value: value("vendorid"),
                choices: &[],
            },
            DeviceField {
                label: "Product ID",
                key: "productid",
                value: value("productid"),
                choices: &[],
            },
            DeviceField {
                label: "Listen",
                key: "listen",
                value: value("listen"),
                choices: &[],
            },
            DeviceField {
                label: "Connect",
                key: "connect",
                value: value("connect"),
                choices: &[],
            },
            DeviceField {
                label: "Device ID",
                key: "id",
                value: value("id"),
                choices: &[],
            },
            DeviceField {
                label: "PCI address",
                key: "address",
                value: value("address"),
                choices: &[],
            },
            DeviceField {
                label: "USB bus",
                key: "busnum",
                value: value("busnum"),
                choices: &[],
            },
            DeviceField {
                label: "USB device",
                key: "devnum",
                value: value("devnum"),
                choices: &[],
            },
            DeviceField {
                label: "Mode",
                key: "mode",
                value: value("mode"),
                choices: &[],
            },
            DeviceField {
                label: "Major",
                key: "major",
                value: value("major"),
                choices: &[],
            },
            DeviceField {
                label: "Minor",
                key: "minor",
                value: value("minor"),
                choices: &[],
            },
            DeviceField {
                label: "Required",
                key: "required",
                value: value("required"),
                choices: &["", "true", "false"],
            },
        ];
        Self {
            original_name: name,
            fields,
            field: 0,
        }
    }

    fn field_value(&self, key: &str) -> String {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.trim().to_owned())
            .unwrap_or_default()
    }

    fn device(&self) -> (String, std::collections::BTreeMap<String, String>) {
        let name = self.field_value("name");
        let config = self
            .fields
            .iter()
            .filter(|field| field.key != "name" && !field.value.trim().is_empty())
            .map(|field| (field.key.to_owned(), field.value.trim().to_owned()))
            .collect();
        (name, config)
    }

    fn cycle(&mut self, reverse: bool) {
        let field = &mut self.fields[self.field];
        if field.choices.is_empty() {
            return;
        }
        let current = field
            .choices
            .iter()
            .position(|choice| *choice == field.value)
            .unwrap_or(0);
        let next = if reverse {
            (current + field.choices.len() - 1) % field.choices.len()
        } else {
            (current + 1) % field.choices.len()
        };
        field.value = field.choices[next].to_owned();
    }
}

impl VolumeManager {
    fn new(pool: String) -> Self {
        Self {
            pool,
            volumes: Vec::new(),
            selected: 0,
            create_form: None,
            edit_form: None,
            attach_form: None,
            detach_form: None,
            snapshots: None,
            backups: None,
            import_form: None,
            copy_form: None,
            protocol_form: None,
            proxy_address: None,
            file_form: None,
            detail: None,
            bitmaps: None,
            delete_input: None,
            rebuild_input: None,
        }
    }

    pub fn pool(&self) -> &str {
        &self.pool
    }
    pub fn bitmaps(&self) -> Option<&VolumeBitmapsForm> {
        self.bitmaps.as_ref()
    }

    pub fn volumes(&self) -> &[StorageVolumeSummary] {
        &self.volumes
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn create_fields(&self) -> Option<[(&'static str, &str, bool); 9]> {
        self.create_form.as_ref().map(|form| {
            [
                ("Name", form.name.as_str(), form.field == 0),
                ("Project", form.project.as_str(), form.field == 1),
                ("Content type", form.content_type.as_str(), form.field == 2),
                ("Description", form.description.as_str(), form.field == 3),
                ("Size", form.size.as_str(), form.field == 4),
                ("Initial UID", form.initial_uid.as_str(), form.field == 5),
                ("Initial GID", form.initial_gid.as_str(), form.field == 6),
                ("Initial mode", form.initial_mode.as_str(), form.field == 7),
                (
                    "Shared block volume",
                    if form.shared { "true" } else { "false" },
                    form.field == 8,
                ),
            ]
        })
    }

    pub fn edit_fields(&self) -> Option<[(&'static str, &str, bool); 2]> {
        self.edit_form.as_ref().map(|form| {
            [
                ("Description", form.description.as_str(), form.field == 0),
                ("Size / quota", form.size.as_str(), form.field == 1),
            ]
        })
    }
    pub fn import_fields(&self) -> Option<[(&'static str, &str, bool, bool); 4]> {
        self.import_form.as_ref().map(|form| {
            [
                (
                    "Local archive",
                    form.local_path.as_str(),
                    false,
                    form.field == 0,
                ),
                ("Project", form.project.as_str(), false, form.field == 1),
                ("New name", form.name.as_str(), false, form.field == 2),
                (
                    "Import type",
                    form.import_type.as_str(),
                    true,
                    form.field == 3,
                ),
            ]
        })
    }
    pub fn copy_fields(&self) -> Option<Vec<(&'static str, String, bool, bool)>> {
        self.copy_form.as_ref().map(|form| {
            vec![
                (
                    "Target pool",
                    form.target_pool.clone(),
                    false,
                    form.field == 0,
                ),
                (
                    "Target project",
                    form.target_project.clone(),
                    false,
                    form.field == 1,
                ),
                (
                    "Target name",
                    form.target_name.clone(),
                    false,
                    form.field == 2,
                ),
                (
                    "Volume only",
                    if form.volume_only { "yes" } else { "no" }.into(),
                    true,
                    form.field == 3,
                ),
                (
                    "Refresh existing",
                    if form.refresh { "yes" } else { "no" }.into(),
                    true,
                    form.field == 4,
                ),
                (
                    "Live migration",
                    if form.supports_live {
                        if form.live { "yes" } else { "no" }
                    } else {
                        "unavailable"
                    }
                    .into(),
                    form.supports_live,
                    form.field == 5,
                ),
                (
                    "Exclude old snapshots",
                    if form.supports_refresh_exclude {
                        if form.refresh_exclude_older {
                            "yes"
                        } else {
                            "no"
                        }
                    } else {
                        "unavailable"
                    }
                    .into(),
                    form.supports_refresh_exclude,
                    form.field == 6,
                ),
            ]
        })
    }
    pub fn protocol_fields(&self) -> Option<[(&'static str, &str, bool, bool); 2]> {
        self.protocol_form.as_ref().map(|(protocol, port, field)| {
            [
                ("Protocol", protocol.as_str(), true, *field == 0),
                ("Local port", port.as_str(), false, *field == 1),
            ]
        })
    }
    pub fn proxy_address(&self) -> Option<&str> {
        self.proxy_address.as_deref()
    }
    pub fn file_form(&self) -> Option<&VolumeFileForm> {
        self.file_form.as_ref()
    }
    pub fn detail(&self) -> Option<(&str, &str)> {
        self.detail
            .as_ref()
            .map(|(title, body)| (title.as_str(), body.as_str()))
    }

    pub fn attach_fields(&self) -> Option<[(&'static str, String, bool); 3]> {
        self.attach_form.as_ref().map(|form| {
            let instance = form
                .instances
                .get(form.instance)
                .map(|(name, project)| format!("{name} ({project})"))
                .unwrap_or_else(|| "no instances".to_owned());
            [
                (
                    if form.target_kind == ResourceKind::Profile {
                        "Profile"
                    } else {
                        "Instance"
                    },
                    instance,
                    form.field == 0,
                ),
                ("Device name", form.device_name.clone(), form.field == 1),
                ("Mount path", form.path.clone(), form.field == 2),
            ]
        })
    }

    pub fn detach_fields(&self) -> Option<[(&'static str, String, bool); 2]> {
        self.detach_form.as_ref().map(|form| {
            let instance = form
                .instances
                .get(form.instance)
                .map(|(name, project)| format!("{name} ({project})"))
                .unwrap_or_else(|| "no instances".to_owned());
            [
                (
                    if form.target_kind == ResourceKind::Profile {
                        "Profile"
                    } else {
                        "Instance"
                    },
                    instance,
                    form.field == 0,
                ),
                ("Device name", form.device_name.clone(), form.field == 1),
            ]
        })
    }

    pub fn snapshots_form(&self) -> Option<&VolumeSnapshotsForm> {
        self.snapshots.as_ref()
    }
    pub fn backups_form(&self) -> Option<&VolumeBackupsForm> {
        self.backups.as_ref()
    }

    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }

    pub fn delete_phrase(&self) -> Option<String> {
        self.volumes
            .get(self.selected)
            .map(|volume| format!("delete volume {}", volume.name))
    }
    pub fn rebuild_input(&self) -> Option<&str> {
        self.rebuild_input.as_deref()
    }
    pub fn rebuild_phrase(&self) -> Option<String> {
        self.volumes
            .get(self.selected)
            .map(|volume| format!("rebuild storage volume {}", volume.name))
    }
}

impl VolumeSnapshotsForm {
    pub fn volume(&self) -> &StorageVolumeSummary {
        &self.volume
    }
    pub fn snapshots(&self) -> &[StorageVolumeSnapshotSummary] {
        &self.snapshots
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn name_input(&self) -> Option<(bool, &str)> {
        self.name_input
            .as_ref()
            .map(|(rename, input)| (*rename, input.as_str()))
    }
    pub fn confirmation(&self) -> Option<(bool, &str)> {
        self.confirmation
            .as_ref()
            .map(|(restore, input)| (*restore, input.as_str()))
    }
    fn selected_name(&self) -> Option<&str> {
        self.snapshots
            .get(self.selected)
            .map(|item| item.name.as_str())
    }
    pub fn phrase(&self, restore: bool) -> String {
        format!(
            "{} volume snapshot {}",
            if restore { "restore" } else { "delete" },
            self.selected_name().unwrap_or_default()
        )
    }
}

impl VolumeBackupsForm {
    pub fn volume(&self) -> &StorageVolumeSummary {
        &self.volume
    }
    pub fn backups(&self) -> &[BackupSummary] {
        &self.backups
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
    fn selected_name(&self) -> Option<&str> {
        self.backups
            .get(self.selected)
            .map(|backup| backup.name.rsplit('/').next().unwrap_or(&backup.name))
    }
    pub fn name_input(&self) -> Option<(bool, &str)> {
        self.name_input
            .as_ref()
            .map(|(rename, input)| (*rename, input.as_str()))
    }
    pub const fn create_options(&self) -> (bool, bool) {
        self.create_options
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn export_path(&self) -> Option<&str> {
        self.export_path.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete volume backup {}",
            self.selected_name().unwrap_or_default()
        )
    }
}

impl VolumeBitmapsForm {
    pub fn volume(&self) -> &StorageVolumeSummary {
        &self.volume
    }
    pub fn bitmaps(&self) -> &[StorageVolumeBitmap] {
        &self.bitmaps
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn create_fields(&self) -> Option<[(&'static str, &str, bool, bool); 4]> {
        self.create
            .as_ref()
            .map(|(name, granularity, persistent, disabled, field)| {
                [
                    ("Name", name.as_str(), false, *field == 0),
                    (
                        "Granularity bytes",
                        granularity.as_str(),
                        false,
                        *field == 1,
                    ),
                    (
                        "Persistent",
                        if *persistent { "on" } else { "off" },
                        true,
                        *field == 2,
                    ),
                    (
                        "Initially disabled",
                        if *disabled { "on" } else { "off" },
                        true,
                        *field == 3,
                    ),
                ]
            })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete bitmap {}",
            self.bitmaps
                .get(self.selected)
                .map(|v| v.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl BucketManager {
    fn new(pool: String) -> Self {
        Self {
            pool,
            buckets: Vec::new(),
            selected: 0,
            editor: None,
            delete_input: None,
            keys: None,
            backups: None,
            import_form: None,
        }
    }
    pub fn pool(&self) -> &str {
        &self.pool
    }
    pub fn buckets(&self) -> &[StorageBucketSummary] {
        &self.buckets
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 4]> {
        self.editor.as_ref().map(|f| {
            [
                ("Name", f.name.as_str(), f.field == 0),
                ("Project", f.project.as_str(), f.field == 1),
                ("Description", f.description.as_str(), f.field == 2),
                ("Size / quota", f.size.as_str(), f.field == 3),
            ]
        })
    }
    pub fn editor_is_editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|f| f.editing)
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete bucket {}",
            self.buckets
                .get(self.selected)
                .map(|b| b.name.as_str())
                .unwrap_or_default()
        )
    }
    pub fn keys(&self) -> Option<&BucketKeysForm> {
        self.keys.as_ref()
    }
    pub fn backups(&self) -> Option<&BucketBackupsForm> {
        self.backups.as_ref()
    }
    pub fn import_fields(&self) -> Option<[(&'static str, &str, bool); 3]> {
        self.import_form.as_ref().map(|f| {
            [
                ("Local archive", f.path.as_str(), f.field == 0),
                ("Project", f.project.as_str(), f.field == 1),
                ("New name", f.name.as_str(), f.field == 2),
            ]
        })
    }
}

impl BucketBackupsForm {
    pub fn bucket(&self) -> &StorageBucketSummary {
        &self.bucket
    }
    pub fn backups(&self) -> &[BackupSummary] {
        &self.backups
    }
    pub const fn selected(&self) -> usize {
        self.selected
    }
    fn selected_name(&self) -> Option<&str> {
        self.backups
            .get(self.selected)
            .map(|b| b.name.rsplit('/').next().unwrap_or(&b.name))
    }
    pub fn name_input(&self) -> Option<(bool, &str)> {
        self.name_input.as_ref().map(|(r, v)| (*r, v.as_str()))
    }
    pub fn compression(&self) -> &str {
        &self.compression
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn export_path(&self) -> Option<&str> {
        self.export_path.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete bucket backup {}",
            self.selected_name().unwrap_or_default()
        )
    }
}

impl BucketKeysForm {
    pub fn bucket(&self) -> &StorageBucketSummary {
        &self.bucket
    }
    pub fn keys(&self) -> &[StorageBucketKeySummary] {
        &self.keys
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 3]> {
        self.editor.as_ref().map(|f| {
            [
                ("Name", f.name.as_str(), f.field == 0),
                ("Description", f.description.as_str(), f.field == 1),
                ("Role", f.role.as_str(), f.field == 2),
            ]
        })
    }
    pub fn editor_is_editing(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| editor.original_name.is_some())
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete bucket key {}",
            self.keys
                .get(self.selected)
                .map(|k| k.name.as_str())
                .unwrap_or_default()
        )
    }
    pub fn credentials(&self) -> Option<&StorageBucketKeySummary> {
        self.credentials.as_ref()
    }
}

impl NetworkServicesManager {
    fn new(network: String, project: String) -> Self {
        Self {
            network,
            project,
            leases: Vec::new(),
            forwards: Vec::new(),
            acls: Vec::new(),
            load_balancers: Vec::new(),
            peers: Vec::new(),
            tab: NetworkServicesTab::Leases,
            selected: 0,
            form: None,
            delete_input: None,
            acl_rules: None,
            detail: None,
            rename_input: None,
            load_balancer_history: Vec::new(),
        }
    }

    pub fn network(&self) -> &str {
        &self.network
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn tab(&self) -> NetworkServicesTab {
        self.tab
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn leases(&self) -> &[NetworkLease] {
        &self.leases
    }
    pub fn forwards(&self) -> &[NetworkForwardSummary] {
        &self.forwards
    }
    pub fn acls(&self) -> &[NetworkAclSummary] {
        &self.acls
    }
    pub fn load_balancers(&self) -> &[NetworkLoadBalancerSummary] {
        &self.load_balancers
    }
    pub fn peers(&self) -> &[NetworkPeerSummary] {
        &self.peers
    }
    pub fn acl_rules(&self) -> Option<&AclRulesForm> {
        self.acl_rules.as_ref()
    }
    pub fn detail(&self) -> Option<(&str, &str)> {
        self.detail
            .as_ref()
            .map(|(title, content)| (title.as_str(), content.as_str()))
    }
    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }

    pub fn form_fields(&self) -> Option<Vec<(&'static str, &str, bool, bool)>> {
        match self.form.as_ref()? {
            NetworkServiceForm::Forward { fields, field, .. } => Some(
                [
                    "Listen address",
                    "Description",
                    "Protocol",
                    "Listen port",
                    "Target address",
                    "Target port",
                    "Health checks",
                    "Check interval",
                    "Check timeout",
                    "Failure count",
                    "Success count",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    (
                        label,
                        fields[index].as_str(),
                        matches!(index, 2 | 7),
                        index == *field,
                    )
                })
                .collect(),
            ),
            NetworkServiceForm::Acl {
                name,
                description,
                field,
            } => Some(vec![
                ("Name", name.as_str(), false, *field == 0),
                ("Description", description.as_str(), false, *field == 1),
            ]),
            NetworkServiceForm::LoadBalancer { fields, field, .. } => Some(
                [
                    "Listen address",
                    "Description",
                    "Protocol",
                    "Listen port",
                    "Backend name",
                    "Target address",
                    "Target port",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, label)| (label, fields[index].as_str(), index == 2, index == *field))
                .collect(),
            ),
            NetworkServiceForm::Peer { fields, field, .. } => Some(
                [
                    "Name",
                    "Description",
                    "Type",
                    "Target project",
                    "Target network",
                    "Target integration",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, label)| (label, fields[index].as_str(), index == 2, index == *field))
                .collect(),
            ),
        }
    }

    pub fn delete_phrase(&self) -> Option<String> {
        match self.tab {
            NetworkServicesTab::Forwards => self
                .forwards
                .get(self.selected)
                .map(|item| format!("delete forward {}", item.listen_address)),
            NetworkServicesTab::Acls => self
                .acls
                .get(self.selected)
                .map(|item| format!("delete acl {}", item.name)),
            NetworkServicesTab::LoadBalancers => self
                .load_balancers
                .get(self.selected)
                .map(|item| format!("delete load balancer {}", item.listen_address)),
            NetworkServicesTab::Peers => self
                .peers
                .get(self.selected)
                .map(|item| format!("delete peer {}", item.name)),
            NetworkServicesTab::Leases => None,
        }
    }

    fn len(&self) -> usize {
        match self.tab {
            NetworkServicesTab::Leases => self.leases.len(),
            NetworkServicesTab::Forwards => self.forwards.len(),
            NetworkServicesTab::Acls => self.acls.len(),
            NetworkServicesTab::LoadBalancers => self.load_balancers.len(),
            NetworkServicesTab::Peers => self.peers.len(),
        }
    }
}

impl AclRulesForm {
    fn rules(&self) -> &[NetworkAclRule] {
        if self.ingress {
            &self.acl.ingress
        } else {
            &self.acl.egress
        }
    }
    fn rules_mut(&mut self) -> &mut Vec<NetworkAclRule> {
        if self.ingress {
            &mut self.acl.ingress
        } else {
            &mut self.acl.egress
        }
    }
    pub fn acl(&self) -> &NetworkAclSummary {
        &self.acl
    }
    pub fn ingress(&self) -> bool {
        self.ingress
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn visible_rules(&self) -> &[NetworkAclRule] {
        self.rules()
    }
    pub fn editor_fields(&self) -> Option<Vec<(&'static str, &str, bool, bool)>> {
        self.editor.as_ref().map(|f| {
            [
                "Action",
                "Source",
                "Destination",
                "Protocol",
                "Source port",
                "Destination port",
                "ICMP type",
                "ICMP code",
                "Description",
                "State",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                (
                    label,
                    f.fields[index].as_str(),
                    matches!(index, 0 | 3 | 9),
                    index == f.field,
                )
            })
            .collect()
        })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete {} rule {}",
            if self.ingress { "ingress" } else { "egress" },
            self.selected + 1
        )
    }
}

impl AclRuleEditor {
    fn from_rule(original: Option<usize>, rule: Option<&NetworkAclRule>) -> Self {
        let r = rule.cloned().unwrap_or(NetworkAclRule {
            action: "allow".to_owned(),
            protocol: "tcp".to_owned(),
            state: "enabled".to_owned(),
            ..Default::default()
        });
        Self {
            original,
            fields: [
                r.action,
                r.source,
                r.destination,
                r.protocol,
                r.source_port,
                r.destination_port,
                r.icmp_type,
                r.icmp_code,
                r.description,
                r.state,
            ],
            field: 0,
        }
    }
    fn rule(&self) -> NetworkAclRule {
        NetworkAclRule {
            action: self.fields[0].clone(),
            source: self.fields[1].trim().to_owned(),
            destination: self.fields[2].trim().to_owned(),
            protocol: self.fields[3].clone(),
            source_port: self.fields[4].trim().to_owned(),
            destination_port: self.fields[5].trim().to_owned(),
            icmp_type: self.fields[6].trim().to_owned(),
            icmp_code: self.fields[7].trim().to_owned(),
            description: self.fields[8].trim().to_owned(),
            state: self.fields[9].clone(),
        }
    }
}

impl ZoneManager {
    fn new() -> Self {
        Self {
            zones: Vec::new(),
            selected: 0,
            editor: None,
            delete_input: None,
            records: None,
            rename_input: None,
        }
    }
    pub fn zones(&self) -> &[NetworkZoneSummary] {
        &self.zones
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 3]> {
        self.editor.as_ref().map(|f| {
            [
                ("Name", f.name.as_str(), f.field == 0),
                ("Project", f.project.as_str(), f.field == 1),
                ("Description", f.description.as_str(), f.field == 2),
            ]
        })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete zone {}",
            self.zones
                .get(self.selected)
                .map(|z| z.name.as_str())
                .unwrap_or_default()
        )
    }
    pub fn records(&self) -> Option<&ZoneRecordsForm> {
        self.records.as_ref()
    }
}
impl ZoneRecordsForm {
    pub fn zone(&self) -> &NetworkZoneSummary {
        &self.zone
    }
    pub fn records(&self) -> &[NetworkZoneRecordSummary] {
        &self.records
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool, bool); 5]> {
        self.editor.as_ref().map(|f| {
            [
                ("Name", f.name.as_str(), false, f.field == 0),
                ("Description", f.description.as_str(), false, f.field == 1),
                ("Type", f.entry_type.as_str(), true, f.field == 2),
                ("Value", f.value.as_str(), false, f.field == 3),
                ("TTL", f.ttl.as_str(), false, f.field == 4),
            ]
        })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete zone record {}",
            self.records
                .get(self.selected)
                .map(|r| r.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl BgpForm {
    fn new(definition: ResourceDefinition) -> Self {
        let ipv4_instances = config_value(&definition, "bgp.ipv4.instances");
        let ipv6_instances = config_value(&definition, "bgp.ipv6.instances");
        let ipv4_nexthop = config_value(&definition, "bgp.ipv4.nexthop");
        let ipv6_nexthop = config_value(&definition, "bgp.ipv6.nexthop");
        Self {
            definition,
            fields: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "180".to_owned(),
                ipv4_instances,
                ipv6_instances,
                ipv4_nexthop,
                ipv6_nexthop,
            ],
            field: 0,
        }
    }
    pub fn network(&self) -> &str {
        &self.definition.name
    }
    pub fn project(&self) -> &str {
        &self.definition.project
    }
    pub fn fields(&self) -> impl Iterator<Item = (&'static str, &str, bool, bool)> {
        [
            "Peer name",
            "Peer address",
            "Peer interface",
            "Peer ASN",
            "Peer password",
            "Hold time",
            "Advertise IPv4 instances",
            "Advertise IPv6 instances",
            "IPv4 next hop",
            "IPv6 next hop",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            (
                label,
                self.fields[index].as_str(),
                index == 6 || index == 7,
                index == self.field,
            )
        })
    }

    fn updated_definition(&self) -> ResourceDefinition {
        let mut definition = self.definition.clone();
        let config = definition
            .value
            .get_mut("config")
            .and_then(serde_json::Value::as_object_mut)
            .expect("network config object");
        for (key, index) in [
            ("bgp.ipv4.instances", 6),
            ("bgp.ipv6.instances", 7),
            ("bgp.ipv4.nexthop", 8),
            ("bgp.ipv6.nexthop", 9),
        ] {
            if self.fields[index].trim().is_empty() {
                config.remove(key);
            } else {
                config.insert(
                    key.to_owned(),
                    serde_json::Value::String(self.fields[index].trim().to_owned()),
                );
            }
        }
        let peer = self.fields[0].trim();
        if !peer.is_empty() {
            for (suffix, index) in [
                ("address", 1),
                ("interface", 2),
                ("asn", 3),
                ("password", 4),
                ("holdtime", 5),
            ] {
                let key = format!("bgp.peers.{peer}.{suffix}");
                if self.fields[index].trim().is_empty() {
                    config.remove(&key);
                } else {
                    config.insert(
                        key,
                        serde_json::Value::String(self.fields[index].trim().to_owned()),
                    );
                }
            }
        }
        definition
    }
}

fn parse_cpu_topology(value: &str) -> Result<(u32, u32, u32), String> {
    let mut topology = std::collections::BTreeMap::new();
    for assignment in value.split(',') {
        let (key, value) = assignment
            .trim()
            .split_once('=')
            .ok_or_else(|| "CPU topology must use key=value assignments".to_owned())?;
        if !matches!(key, "sockets" | "cores" | "threads") {
            return Err(format!("unsupported CPU topology key: {key}"));
        }
        let value = value
            .parse::<u32>()
            .map_err(|_| format!("CPU topology {key} must be a positive integer"))?;
        if value == 0 {
            return Err(format!("CPU topology {key} must be greater than zero"));
        }
        if topology.insert(key, value).is_some() {
            return Err(format!("CPU topology key repeated: {key}"));
        }
    }
    Ok((
        *topology.get("sockets").unwrap_or(&1),
        *topology.get("cores").unwrap_or(&1),
        *topology.get("threads").unwrap_or(&1),
    ))
}

impl AddressSetManager {
    fn new() -> Self {
        Self {
            sets: Vec::new(),
            selected: 0,
            editor: None,
            delete_input: None,
            rename_input: None,
        }
    }
    pub fn sets(&self) -> &[NetworkAddressSetSummary] {
        &self.sets
    }
    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 4]> {
        self.editor.as_ref().map(|f| {
            [
                ("Name", f.name.as_str(), f.field == 0),
                ("Project", f.project.as_str(), f.field == 1),
                ("Description", f.description.as_str(), f.field == 2),
                ("Addresses", f.addresses.as_str(), f.field == 3),
            ]
        })
    }
    pub fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|f| f.editing)
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete address set {}",
            self.sets
                .get(self.selected)
                .map(|s| s.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl IntegrationManager {
    fn new() -> Self {
        Self {
            integrations: Vec::new(),
            selected: 0,
            editor: None,
            delete_input: None,
            rename_input: None,
        }
    }
    pub fn integrations(&self) -> &[NetworkIntegrationSummary] {
        &self.integrations
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }
    pub fn editor_fields(&self) -> Option<Vec<(&'static str, &str, bool, bool)>> {
        self.editor.as_ref().map(|editor| {
            [
                "Name",
                "Type",
                "Description",
                "OVN northbound",
                "OVN southbound",
                "OVN CA cert",
                "OVN client cert",
                "OVN client key",
                "Transit pattern",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                (
                    label,
                    editor.fields[index].as_str(),
                    false,
                    index == editor.field,
                )
            })
            .collect()
        })
    }
    pub fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.editing)
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete network integration {}",
            self.integrations
                .get(self.selected)
                .map(|v| v.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl RenameForm {
    pub fn current_name(&self) -> &str {
        &self.current_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn new_name(&self) -> &str {
        &self.new_name
    }
}

impl RebuildForm {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn image(&self) -> &str {
        &self.image
    }
    pub fn confirmation(&self) -> &str {
        &self.confirmation
    }
    pub fn field(&self) -> usize {
        self.field
    }
    pub fn phrase(&self) -> String {
        format!("rebuild {}", self.name)
    }
}

impl InstanceLogManager {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn logs(&self) -> &[String] {
        &self.logs
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn clear_console_input(&self) -> Option<&str> {
        self.clear_console_input.as_deref()
    }
    pub fn cleanup_exec_input(&self) -> Option<&str> {
        self.cleanup_exec_input.as_deref()
    }
    pub fn exec_output_count(&self) -> usize {
        self.logs
            .iter()
            .filter(|log| log.starts_with("exec-output/"))
            .count()
    }
    pub fn cleanup_exec_phrase(&self) -> String {
        format!(
            "delete {} exec outputs {}",
            self.exec_output_count(),
            self.name
        )
    }
    pub fn selected_log(&self) -> Option<&str> {
        self.logs.get(self.selected).map(String::as_str)
    }
    pub fn delete_phrase(&self) -> String {
        format!("delete log {}", self.selected_log().unwrap_or_default())
    }
    pub fn clear_phrase(&self) -> String {
        format!("clear console {}", self.name)
    }
}

impl NetworkAllocationExplorer {
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn filter(&self) -> &str {
        &self.filter
    }
    pub fn filtering(&self) -> bool {
        self.filtering
    }
}

impl MetricsExplorer {
    pub fn instance(&self) -> &str {
        &self.instance
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn filter(&self) -> &str {
        &self.filter
    }
    pub fn filtering(&self) -> bool {
        self.filtering
    }
}

impl InstanceMaintenanceManager {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn tab(&self) -> MaintenanceTab {
        self.tab
    }
    pub fn templates(&self) -> &[String] {
        &self.templates
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn metadata(&self) -> &serde_json::Value {
        &self.metadata
    }
    pub fn metadata_editor(&self) -> Option<(&str, &str, usize)> {
        self.metadata_editor
            .as_ref()
            .map(|(k, v, f)| (k.as_str(), v.as_str(), *f))
    }
    pub fn template_editor(&self) -> Option<(&str, &str, usize)> {
        self.template_editor
            .as_ref()
            .map(|(n, c, f)| (n.as_str(), c.as_str(), *f))
    }
    pub fn template_permission_fields(&self) -> Option<[(&'static str, &str, bool); 4]> {
        self.template_permissions
            .as_ref()
            .map(|(path, uid, gid, mode, field)| {
                [
                    ("Target path", path.as_str(), *field == 0),
                    ("UID", uid.as_str(), *field == 1),
                    ("GID", gid.as_str(), *field == 2),
                    ("Mode", mode.as_str(), *field == 3),
                ]
            })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn dump_path(&self) -> Option<&str> {
        self.dump_path.as_deref()
    }
    pub fn screenshot_path(&self) -> Option<&str> {
        self.screenshot_path.as_deref()
    }
    pub fn repair_input(&self) -> Option<&str> {
        self.repair_input.as_deref()
    }
    pub fn selected_template(&self) -> Option<&str> {
        self.templates.get(self.selected).map(String::as_str)
    }
    pub fn protocol_fields(&self) -> Option<[(&'static str, &str, bool, bool); 2]> {
        self.protocol_form.as_ref().map(|(protocol, port, field)| {
            [
                ("Protocol", protocol.as_str(), true, *field == 0),
                ("Local port", port.as_str(), false, *field == 1),
            ]
        })
    }
    pub fn proxy_address(&self) -> Option<&str> {
        self.proxy_address.as_deref()
    }
    pub fn bitmap_fields(&self) -> Option<[(&'static str, &str, bool, bool); 4]> {
        self.bitmap_form
            .as_ref()
            .map(|(name, granularity, persistent, disabled, field)| {
                [
                    ("Name", name.as_str(), false, *field == 0),
                    (
                        "Granularity bytes",
                        granularity.as_str(),
                        false,
                        *field == 1,
                    ),
                    (
                        "Persistent",
                        if *persistent { "on" } else { "off" },
                        true,
                        *field == 2,
                    ),
                    (
                        "Initially disabled",
                        if *disabled { "on" } else { "off" },
                        true,
                        *field == 3,
                    ),
                ]
            })
    }
}

pub fn metric_rows<'a>(metrics: &'a str, explorer: &MetricsExplorer) -> Vec<(&'a str, &'a str)> {
    let instance_marker = format!("name=\"{}\"", explorer.instance);
    let instance_alt = format!("instance=\"{}\"", explorer.instance);
    let project_marker = format!("project=\"{}\"", explorer.project);
    let filter = explorer.filter.to_lowercase();
    metrics
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| line.contains(&instance_marker) || line.contains(&instance_alt))
        .filter(|line| {
            explorer.project.is_empty()
                || explorer.project == "default"
                || line.contains(&project_marker)
        })
        .filter(|line| filter.is_empty() || line.to_lowercase().contains(&filter))
        .filter_map(|line| line.rsplit_once(char::is_whitespace))
        .collect()
}

fn allocation_matches(value: &serde_json::Value, filter: &str) -> bool {
    filter.is_empty()
        || value
            .to_string()
            .to_lowercase()
            .contains(&filter.to_lowercase())
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

impl OperationDetailForm {
    pub fn operation(&self) -> &crate::incus::Operation {
        &self.operation
    }
}

#[derive(Clone, Debug)]
struct ConfigField {
    label: &'static str,
    key: ConfigKey,
    kind: ConfigFieldKind,
    value: String,
    dirty: bool,
}

#[derive(Clone, Copy, Debug)]
enum ConfigKey {
    Description,
    Config(&'static str),
}

#[derive(Clone, Copy, Debug)]
enum ConfigFieldKind {
    Text,
    Secret,
    Toggle,
    Choice(&'static [&'static str]),
}

impl ConfigForm {
    fn new(definition: ResourceDefinition) -> Self {
        let fields = config_fields(&definition);
        Self {
            definition,
            fields,
            field: 0,
            omitted_capabilities: Vec::new(),
        }
    }

    pub fn kind(&self) -> ResourceKind {
        self.definition.kind
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn project(&self) -> &str {
        &self.definition.project
    }

    pub fn cpu_topology_preview(&self) -> Option<Result<String, String>> {
        if self.definition.kind != ResourceKind::Instance {
            return None;
        }
        let value = self
            .fields
            .iter()
            .find(|field| matches!(field.key, ConfigKey::Config("limits.cpu")))?
            .value
            .trim();
        if !value.contains('=') {
            return None;
        }
        Some(parse_cpu_topology(value).map(|(sockets, cores, threads)| {
            format!(
                "CPU topology preview: {sockets} sockets × {cores} cores × {threads} threads = {} vCPUs",
                sockets * cores * threads
            )
        }))
    }

    pub fn fields(&self) -> impl Iterator<Item = (&'static str, &str, bool, bool, bool)> {
        self.fields.iter().enumerate().map(|(index, field)| {
            (
                field.label,
                field.value.as_str(),
                matches!(
                    field.kind,
                    ConfigFieldKind::Toggle | ConfigFieldKind::Choice(_)
                ),
                matches!(field.kind, ConfigFieldKind::Secret),
                index == self.field,
            )
        })
    }
    pub fn omitted_capabilities(&self) -> &[&'static str] {
        &self.omitted_capabilities
    }

    fn previous(&mut self) {
        if !self.fields.is_empty() {
            self.field = (self.field + self.fields.len() - 1) % self.fields.len();
        }
    }

    fn next(&mut self) {
        if !self.fields.is_empty() {
            self.field = (self.field + 1) % self.fields.len();
        }
    }

    fn input(&mut self, character: char) {
        if let Some(field) = self.fields.get_mut(self.field)
            && matches!(field.kind, ConfigFieldKind::Text | ConfigFieldKind::Secret)
        {
            if matches!(field.kind, ConfigFieldKind::Secret) && !field.dirty {
                field.value.clear();
            }
            field.value.push(character);
            field.dirty = true;
        }
    }

    fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.field)
            && matches!(field.kind, ConfigFieldKind::Text | ConfigFieldKind::Secret)
        {
            if matches!(field.kind, ConfigFieldKind::Secret) && !field.dirty {
                field.value.clear();
            } else {
                field.value.pop();
            }
            field.dirty = true;
        }
    }

    fn toggle(&mut self) {
        if let Some(field) = self.fields.get_mut(self.field) {
            match field.kind {
                ConfigFieldKind::Toggle => {
                    field.value = if field.value == "true" {
                        "false"
                    } else {
                        "true"
                    }
                    .to_owned();
                }
                ConfigFieldKind::Choice(choices) if !choices.is_empty() => {
                    let current = choices
                        .iter()
                        .position(|choice| *choice == field.value)
                        .unwrap_or(0);
                    field.value = choices[(current + 1) % choices.len()].to_owned();
                }
                _ => {}
            }
        }
    }

    pub fn snapshot_policy_preview(&self) -> Option<String> {
        if self.definition.kind != ResourceKind::Instance {
            return None;
        }
        let value = |key| {
            self.fields
                .iter()
                .find(|field| matches!(field.key, ConfigKey::Config(candidate) if candidate == key))
                .map(|field| field.value.as_str())
                .unwrap_or("")
        };
        let schedule = value("snapshots.schedule");
        let expiry = value("snapshots.expiry");
        if schedule.is_empty() {
            return Some("Automatic snapshots disabled; no scheduled retention deletions.".into());
        }
        let pattern = value("snapshots.pattern");
        Some(format!(
            "Policy preview: create {pattern} on {schedule}; {}.",
            if expiry.is_empty() {
                "retain indefinitely".into()
            } else {
                format!("delete each scheduled snapshot after {expiry}")
            }
        ))
    }
    pub fn image_policy_preview(&self) -> Option<String> {
        if self.definition.kind != ResourceKind::Server {
            return None;
        }
        let value = |key| {
            self.fields
                .iter()
                .find(|field| matches!(field.key, ConfigKey::Config(candidate) if candidate == key))
                .map(|field| field.value.as_str())
                .unwrap_or("")
        };
        let interval = value("images.auto_update_interval");
        let cached = value("images.auto_update_cached") == "true";
        let expiry = value("images.remote_cache_expiry");
        Some(format!(
            "Image policy preview: check auto-updating images every {} hours; cached images {}; remote cache expires after {} hours.",
            if interval.is_empty() { "6" } else { interval },
            if cached {
                "are refreshed"
            } else {
                "are not refreshed"
            },
            if expiry.is_empty() { "10" } else { expiry }
        ))
    }

    pub fn authorization_preview(&self) -> Option<String> {
        if self.definition.kind != ResourceKind::Server {
            return None;
        }
        let value = |key| {
            self.fields
                .iter()
                .find(|field| matches!(field.key, ConfigKey::Config(candidate) if candidate == key))
                .map(|field| field.value.trim())
                .unwrap_or("")
        };
        let oidc = !value("oidc.issuer").is_empty() && !value("oidc.client.id").is_empty();
        let fga_url = value("authorization.openfga.api.url");
        let fga_store = value("authorization.openfga.store.id");
        let fga_token = !value("authorization.openfga.api.token").is_empty();
        Some(format!(
            "Access preview: OIDC authentication {}; OpenFGA authorization {}{}.",
            if oidc { "configured" } else { "incomplete" },
            if fga_url.is_empty() || fga_store.is_empty() {
                "incomplete"
            } else {
                "configured"
            },
            if fga_token {
                " with API token"
            } else {
                " without API token"
            }
        ))
    }

    fn value(&self) -> serde_json::Value {
        let mut value = self.definition.value.clone();
        let server = self.definition.kind == ResourceKind::Server;
        if server {
            let config = value.as_object_mut().expect("server config is an object");
            for field in &self.fields {
                if let ConfigKey::Config(key) = field.key {
                    if field.value.is_empty() {
                        config.remove(key);
                    } else {
                        config.insert(
                            key.to_owned(),
                            serde_json::Value::String(field.value.clone()),
                        );
                    }
                }
            }
            return value;
        }

        let object = value
            .as_object_mut()
            .expect("resource definition is an object");
        for field in &self.fields {
            if matches!(field.key, ConfigKey::Description) {
                object.insert(
                    "description".to_owned(),
                    serde_json::Value::String(field.value.clone()),
                );
            }
        }
        let config = object
            .entry("config")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("resource config is an object");
        for field in &self.fields {
            match field.key {
                ConfigKey::Description => {}
                ConfigKey::Config(key) if field.value.is_empty() => {
                    config.remove(key);
                }
                ConfigKey::Config(key) => {
                    config.insert(
                        key.to_owned(),
                        serde_json::Value::String(field.value.clone()),
                    );
                }
            }
        }
        value
    }
}

impl ResourceCreateForm {
    fn new(kind: ResourceKind) -> Self {
        let text = |label, key, value: &str| CreateField {
            label,
            key,
            kind: CreateFieldKind::Text,
            value: value.to_owned(),
        };
        let toggle = |label, key, value| CreateField {
            label,
            key,
            kind: CreateFieldKind::Toggle,
            value: if value { "true" } else { "false" }.to_owned(),
        };
        let choice = |label, key, values: &'static [&'static str]| CreateField {
            label,
            key,
            kind: CreateFieldKind::Choice(values),
            value: values.first().copied().unwrap_or_default().to_owned(),
        };
        let fields = match kind {
            ResourceKind::Network => vec![
                text("Name", CreateKey::Name, ""),
                text("Project", CreateKey::Project, "default"),
                choice(
                    "Type",
                    CreateKey::Type,
                    &["bridge", "ovn", "macvlan", "sriov", "physical"],
                ),
                text("Description", CreateKey::Description, ""),
                text("IPv4 address", CreateKey::Config("ipv4.address"), "auto"),
                toggle("IPv4 NAT", CreateKey::Config("ipv4.nat"), true),
                toggle("IPv4 DHCP", CreateKey::Config("ipv4.dhcp"), true),
                text("IPv6 address", CreateKey::Config("ipv6.address"), "auto"),
                toggle("IPv6 NAT", CreateKey::Config("ipv6.nat"), false),
            ],
            ResourceKind::StoragePool => vec![
                text("Name", CreateKey::Name, ""),
                choice("Driver", CreateKey::Driver, &["dir", "btrfs", "lvm", "zfs"]),
                text("Description", CreateKey::Description, ""),
                text("Source", CreateKey::Config("source"), ""),
                text("Size", CreateKey::Config("size"), ""),
                toggle(
                    "Wipe source signatures",
                    CreateKey::Config("source.wipe"),
                    false,
                ),
            ],
            ResourceKind::Project => vec![
                text("Name", CreateKey::Name, ""),
                text("Description", CreateKey::Description, ""),
                toggle(
                    "Features images",
                    CreateKey::Config("features.images"),
                    true,
                ),
                toggle(
                    "Features profiles",
                    CreateKey::Config("features.profiles"),
                    true,
                ),
                toggle(
                    "Features networks",
                    CreateKey::Config("features.networks"),
                    false,
                ),
            ],
            ResourceKind::Profile => vec![
                text("Name", CreateKey::Name, ""),
                text("Project", CreateKey::Project, "default"),
                text("Description", CreateKey::Description, ""),
                text("CPU limit", CreateKey::Config("limits.cpu"), ""),
                text("Memory limit", CreateKey::Config("limits.memory"), ""),
                toggle(
                    "Allow nesting",
                    CreateKey::Config("security.nesting"),
                    false,
                ),
            ],
            _ => Vec::new(),
        };
        Self {
            kind,
            fields,
            field: 0,
            wipe_confirmation: None,
        }
    }

    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub fn fields(&self) -> impl Iterator<Item = (&'static str, &str, &'static str, bool)> {
        self.fields.iter().enumerate().map(|(index, field)| {
            let kind = match field.kind {
                CreateFieldKind::Text => "input",
                CreateFieldKind::Toggle => "toggle",
                CreateFieldKind::Choice(_) => "choice",
            };
            (field.label, field.value.as_str(), kind, index == self.field)
        })
    }

    pub fn wipe_confirmation(&self) -> Option<&str> {
        self.wipe_confirmation.as_deref()
    }

    pub fn wipe_phrase(&self) -> String {
        format!(
            "wipe source {} and create pool {}",
            self.config("source"),
            self.name()
        )
    }

    fn config(&self, key: &str) -> String {
        self.fields
            .iter()
            .find(|field| matches!(field.key, CreateKey::Config(candidate) if candidate == key))
            .map(|field| field.value.trim().to_owned())
            .unwrap_or_default()
    }

    fn next(&mut self) {
        if !self.fields.is_empty() {
            self.field = (self.field + 1) % self.fields.len();
        }
    }

    fn previous(&mut self) {
        if !self.fields.is_empty() {
            self.field = (self.field + self.fields.len() - 1) % self.fields.len();
        }
    }

    fn input(&mut self, character: char) {
        if let Some(field) = self.fields.get_mut(self.field)
            && matches!(field.kind, CreateFieldKind::Text)
        {
            field.value.push(character);
        }
    }

    fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.field)
            && matches!(field.kind, CreateFieldKind::Text)
        {
            field.value.pop();
        }
    }

    fn cycle(&mut self, reverse: bool) {
        let Some(field) = self.fields.get_mut(self.field) else {
            return;
        };
        match field.kind {
            CreateFieldKind::Toggle => {
                field.value = if field.value == "true" {
                    "false"
                } else {
                    "true"
                }
                .to_owned();
            }
            CreateFieldKind::Choice(values) if !values.is_empty() => {
                let current = values
                    .iter()
                    .position(|value| *value == field.value)
                    .unwrap_or(0);
                let next = if reverse {
                    (current + values.len() - 1) % values.len()
                } else {
                    (current + 1) % values.len()
                };
                field.value = values[next].to_owned();
            }
            _ => {}
        }
    }

    fn name(&self) -> String {
        self.fields
            .iter()
            .find(|field| matches!(field.key, CreateKey::Name))
            .map(|field| field.value.trim().to_owned())
            .unwrap_or_default()
    }

    fn project(&self) -> String {
        self.fields
            .iter()
            .find(|field| matches!(field.key, CreateKey::Project))
            .map(|field| field.value.trim().to_owned())
            .unwrap_or_default()
    }

    fn value(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        let mut config = serde_json::Map::new();
        for field in &self.fields {
            match field.key {
                CreateKey::Name => {
                    object.insert("name".to_owned(), serde_json::json!(field.value.trim()));
                }
                CreateKey::Project => {}
                CreateKey::Type => {
                    object.insert("type".to_owned(), serde_json::json!(field.value));
                }
                CreateKey::Driver => {
                    object.insert("driver".to_owned(), serde_json::json!(field.value));
                }
                CreateKey::Description => {
                    object.insert("description".to_owned(), serde_json::json!(field.value));
                }
                CreateKey::Config(key) if !field.value.is_empty() => {
                    config.insert(key.to_owned(), serde_json::json!(field.value));
                }
                CreateKey::Config(_) => {}
            }
        }
        object.insert("config".to_owned(), serde_json::Value::Object(config));
        serde_json::Value::Object(object)
    }
}

impl ResourceDeleteConfirmation {
    fn new(kind: ResourceKind, name: String, project: String) -> Self {
        Self {
            kind,
            name,
            project,
            input: String::new(),
        }
    }

    pub fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn expected_phrase(&self) -> String {
        format!("delete {} {}", self.kind.label(), self.name)
    }
}

impl ImageAliasForm {
    fn new(image: &crate::incus::ImageSummary) -> Self {
        Self {
            fingerprint: image.fingerprint.clone(),
            project: image.project.clone(),
            alias: String::new(),
            description: String::new(),
            field: 0,
        }
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn field(&self) -> usize {
        self.field
    }
}

impl ImageAliasManager {
    fn new(image: &crate::incus::ImageSummary) -> Self {
        Self {
            fingerprint: image.fingerprint.clone(),
            project: image.project.clone(),
            aliases: image.aliases.clone(),
            selected: 0,
            editor: None,
            delete_input: None,
        }
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn aliases(&self) -> &[crate::incus::ImageAlias] {
        &self.aliases
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn editor_fields(&self) -> Option<[(&'static str, &str, bool); 2]> {
        self.editor.as_ref().map(|editor| {
            [
                ("Name", editor.name.as_str(), editor.field == 0),
                (
                    "Description",
                    editor.description.as_str(),
                    editor.field == 1,
                ),
            ]
        })
    }
    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }
    pub fn delete_phrase(&self) -> String {
        format!(
            "delete image alias {}",
            self.aliases
                .get(self.selected)
                .map(|v| v.name.as_str())
                .unwrap_or_default()
        )
    }
}

impl ArtifactsForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            instance_type: instance.instance_type.clone(),
            snapshots: Vec::new(),
            backups: Vec::new(),
            tab: ArtifactTab::Snapshots,
            selected: 0,
            delete_input: None,
            restoring: false,
            rename_input: None,
            transfer: None,
            retention_preview: false,
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn tab(&self) -> ArtifactTab {
        self.tab
    }

    pub fn snapshots(&self) -> &[SnapshotSummary] {
        &self.snapshots
    }

    pub fn backups(&self) -> &[BackupSummary] {
        &self.backups
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn delete_input(&self) -> Option<&str> {
        self.delete_input.as_deref()
    }

    pub fn selected_name(&self) -> Option<&str> {
        match self.tab {
            ArtifactTab::Snapshots => self
                .snapshots
                .get(self.selected)
                .map(|item| item.name.as_str()),
            ArtifactTab::Backups => self
                .backups
                .get(self.selected)
                .map(|item| item.name.as_str()),
        }
    }

    pub fn delete_phrase(&self) -> Option<String> {
        self.selected_name().map(|name| {
            if self.restoring {
                format!("restore snapshot {name}")
            } else {
                match self.tab {
                    ArtifactTab::Snapshots => format!("delete snapshot {name}"),
                    ArtifactTab::Backups => format!("delete backup {name}"),
                }
            }
        })
    }

    pub fn restoring(&self) -> bool {
        self.restoring
    }

    pub fn rename_input(&self) -> Option<&str> {
        self.rename_input.as_deref()
    }

    pub fn transfer(&self) -> Option<(bool, &str)> {
        self.transfer
            .as_ref()
            .map(|(importing, path)| (*importing, path.as_str()))
    }
    pub fn retention_preview(&self) -> bool {
        self.retention_preview
    }
    pub fn retention_rows(&self) -> Vec<(String, String, bool)> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0);
        let mut rows = Vec::new();
        for snapshot in &self.snapshots {
            if !snapshot.expires_at.is_empty() {
                let expired = parse_rfc3339_timestamp(&snapshot.expires_at)
                    .is_some_and(|expiry| expiry <= now);
                rows.push((
                    format!("snapshot/{}", snapshot.name),
                    snapshot.expires_at.clone(),
                    expired,
                ));
            }
        }
        for backup in &self.backups {
            if !backup.expires_at.is_empty() {
                let expired =
                    parse_rfc3339_timestamp(&backup.expires_at).is_some_and(|expiry| expiry <= now);
                rows.push((
                    format!("backup/{}", backup.name),
                    backup.expires_at.clone(),
                    expired,
                ));
            }
        }
        rows.sort_by(|a, b| a.1.cmp(&b.1));
        rows
    }

    fn len(&self) -> usize {
        match self.tab {
            ArtifactTab::Snapshots => self.snapshots.len(),
            ArtifactTab::Backups => self.backups.len(),
        }
    }

    fn clamp(&mut self) {
        self.selected = self.selected.min(self.len().saturating_sub(1));
    }
}

fn config_fields(definition: &ResourceDefinition) -> Vec<ConfigField> {
    let text = |label, key| ConfigField {
        label,
        key: ConfigKey::Config(key),
        kind: ConfigFieldKind::Text,
        value: config_value(definition, key),
        dirty: false,
    };
    let secret = |label, key| ConfigField {
        label,
        key: ConfigKey::Config(key),
        kind: ConfigFieldKind::Secret,
        value: config_value(definition, key),
        dirty: false,
    };
    let toggle = |label, key| ConfigField {
        label,
        key: ConfigKey::Config(key),
        kind: ConfigFieldKind::Toggle,
        value: if config_value(definition, key) == "true" {
            "true".to_owned()
        } else {
            "false".to_owned()
        },
        dirty: false,
    };
    let choice = |label, key, choices: &'static [&'static str]| ConfigField {
        label,
        key: ConfigKey::Config(key),
        kind: ConfigFieldKind::Choice(choices),
        value: config_value(definition, key),
        dirty: false,
    };
    let description = || ConfigField {
        label: "Description",
        key: ConfigKey::Description,
        kind: ConfigFieldKind::Text,
        value: definition
            .value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        dirty: false,
    };

    match definition.kind {
        ResourceKind::Instance => vec![
            description(),
            text("CPU limit", "limits.cpu"),
            text("CPU allowance", "limits.cpu.allowance"),
            text("CPU priority", "limits.cpu.priority"),
            text("CPU NUMA nodes", "limits.cpu.nodes"),
            text("Memory limit", "limits.memory"),
            text("Memory swap", "limits.memory.swap"),
            toggle("Memory huge pages", "limits.memory.hugepages"),
            text("OOM priority", "limits.memory.oom_priority"),
            text("Process limit", "limits.processes"),
            toggle("Start at boot", "boot.autostart"),
            toggle("Auto-restart on failure", "boot.autorestart"),
            text("Boot priority", "boot.autostart.priority"),
            text("Boot delay", "boot.autostart.delay"),
            text("Boot host shutdown timeout", "boot.host_shutdown_timeout"),
            choice(
                "Snapshot schedule",
                "snapshots.schedule",
                &["", "@hourly", "@daily", "@weekly", "@monthly", "@annually"],
            ),
            choice(
                "Snapshot expiry",
                "snapshots.expiry",
                &["", "1d", "7d", "30d", "90d", "1y"],
            ),
            text("Snapshot name pattern", "snapshots.pattern"),
            toggle("Snapshot while stopped", "snapshots.schedule.stopped"),
            toggle("Allow nesting", "security.nesting"),
            toggle("Privileged", "security.privileged"),
            toggle("Protection delete", "security.protection.delete"),
            toggle("Protection shift", "security.protection.shift"),
            toggle("Secure boot", "security.secureboot"),
            toggle("Legacy CSM firmware", "security.csm"),
            toggle("Virtual IOMMU", "security.iommu"),
            toggle("AMD SEV", "security.sev"),
            toggle("AMD SEV encrypted state", "security.sev.policy.es"),
            text("SEV session data", "security.sev.session.data"),
            text("SEV session DH", "security.sev.session.dh"),
            toggle("Agent metrics", "security.agent.metrics"),
            toggle("Migration stateful", "migration.stateful"),
            choice(
                "Cluster evacuation mode",
                "cluster.evacuate",
                &["auto", "stop", "migrate", "live-migrate"],
            ),
            text("OCI entrypoint", "oci.entrypoint"),
            text("OCI working directory", "oci.cwd"),
            text("OCI UID", "oci.uid"),
            text("OCI GID", "oci.gid"),
            text("OCI DNS domain", "oci.dns.domain"),
            text("OCI DNS servers", "oci.dns.nameservers"),
            text("OCI DNS search", "oci.dns.search"),
            text("QEMU scriptlet", "raw.qemu.scriptlet"),
            text("QMP pre-start", "raw.qemu.qmp.pre-start"),
            text("QMP post-start", "raw.qemu.qmp.post-start"),
            text("Cloud-init user-data", "cloud-init.user-data"),
            text("Cloud-init vendor-data", "cloud-init.vendor-data"),
            text("Cloud-init network-config", "cloud-init.network-config"),
            text("Volatile cluster target", "volatile.cluster.target"),
        ],
        ResourceKind::Network => vec![
            description(),
            text("IPv4 address", "ipv4.address"),
            toggle("IPv4 NAT", "ipv4.nat"),
            toggle("IPv4 DHCP", "ipv4.dhcp"),
            text("IPv4 DHCP ranges", "ipv4.dhcp.ranges"),
            text("IPv4 DHCP gateway", "ipv4.dhcp.gateway"),
            text("IPv4 DHCP expiry", "ipv4.dhcp.expiry"),
            text("IPv4 routes", "ipv4.routes"),
            text("IPv6 address", "ipv6.address"),
            toggle("IPv6 NAT", "ipv6.nat"),
            toggle("IPv6 DHCP", "ipv6.dhcp"),
            toggle("IPv6 DHCP stateful", "ipv6.dhcp.stateful"),
            text("IPv6 DHCP ranges", "ipv6.dhcp.ranges"),
            text("IPv6 DHCP expiry", "ipv6.dhcp.expiry"),
            text("IPv6 routes", "ipv6.routes"),
            text("DNS domain", "dns.domain"),
            choice("DNS mode", "dns.mode", &["managed", "dynamic", "none"]),
            text("DNS search", "dns.search"),
            choice("Bridge driver", "bridge.driver", &["native", "openvswitch"]),
            text("Bridge MTU", "bridge.mtu"),
            text("Bridge external interfaces", "bridge.external_interfaces"),
            toggle("Bridge multicast snooping", "bridge.multicast_snooping"),
            text("Network ACLs", "security.acls"),
            text(
                "Default ingress action",
                "security.acls.default.ingress.action",
            ),
            text(
                "Default egress action",
                "security.acls.default.egress.action",
            ),
            toggle("IPv4 L3-only", "ipv4.l3only"),
            toggle("IPv6 L3-only", "ipv6.l3only"),
            text("IPv4 NAT source", "ipv4.nat.address"),
            text("IPv6 NAT source", "ipv6.nat.address"),
            text("OVN network", "network"),
            text("Parent interface", "parent"),
            text("VLAN", "vlan"),
            text("Tagged VLANs", "vlan.tagged"),
            toggle("GVRP registration", "gvrp"),
            text("MTU", "mtu"),
            text("Physical IPv4 gateway", "ipv4.gateway"),
            text("Physical IPv4 OVN ranges", "ipv4.ovn.ranges"),
            text("Physical IPv6 gateway", "ipv6.gateway"),
            text("Physical IPv6 OVN ranges", "ipv6.ovn.ranges"),
            choice(
                "OVN ingress mode",
                "ovn.ingress_mode",
                &["l2proxy", "routed"],
            ),
        ],
        ResourceKind::StoragePool => vec![
            description(),
            text("Source", "source"),
            text("Size", "size"),
            text("Volume size default", "volume.size"),
            text("Volume filesystem", "volume.block.filesystem"),
            text("Volume mount options", "volume.block.mount_options"),
            text("Volume security shift", "volume.security.shifted"),
            text("Volume security shared", "volume.security.shared"),
            text("Volume initial UID", "volume.initial.uid"),
            text("Volume initial GID", "volume.initial.gid"),
            text("Volume initial mode", "volume.initial.mode"),
            toggle("Wipe source signatures", "source.wipe"),
            text("Btrfs mount options", "btrfs.mount_options"),
            text("LVM thin pool", "lvm.thinpool_name"),
            text("LVM volume group", "lvm.vg_name"),
            text("LVM metadata size", "lvm.metadata_size"),
            text("LVM thin metadata size", "lvm.thinpool_metadata_size"),
            text("ZFS pool name", "zfs.pool_name"),
            text("ZFS vdev layout", "zfs.vdev"),
            text("ZFS clone copy", "zfs.clone_copy"),
            text("Ceph cluster", "ceph.cluster_name"),
            text("Ceph user", "ceph.user.name"),
            text("Ceph RBD features", "ceph.rbd.features"),
            text("LINSTOR resource group", "linstor.resource_group"),
            text("PowerFlex gateway", "powerflex.gateway"),
            text("PowerFlex pool", "powerflex.pool"),
        ],
        ResourceKind::Project => vec![
            description(),
            toggle("Features images", "features.images"),
            toggle("Features profiles", "features.profiles"),
            toggle("Features networks", "features.networks"),
            toggle("Features storage volumes", "features.storage.volumes"),
            toggle("Restricted", "restricted"),
            text("Restricted containers", "restricted.containers.lowlevel"),
            text("Restricted VMs", "restricted.virtual-machines.lowlevel"),
            text("Allowed uplinks", "restricted.networks.uplinks"),
            text("Allowed storage pools", "restricted.devices.disk.paths"),
            text("Instance limit", "limits.instances"),
            text("Container limit", "limits.containers"),
            text("VM limit", "limits.virtual-machines"),
            text("CPU limit", "limits.cpu"),
            text("Memory limit", "limits.memory"),
            text("Disk limit", "limits.disk"),
            text("Network limit", "limits.networks"),
        ],
        ResourceKind::Profile => vec![
            description(),
            text("CPU limit", "limits.cpu"),
            text("CPU allowance", "limits.cpu.allowance"),
            text("Memory limit", "limits.memory"),
            text("Memory swap", "limits.memory.swap"),
            toggle("Start at boot", "boot.autostart"),
            toggle("Allow nesting", "security.nesting"),
            toggle("Privileged", "security.privileged"),
            toggle("Protection delete", "security.protection.delete"),
            text("Secure boot", "security.secureboot"),
            text("Cloud-init user-data", "cloud-init.user-data"),
            text("Cloud-init vendor-data", "cloud-init.vendor-data"),
            text("Cloud-init network-config", "cloud-init.network-config"),
        ],
        ResourceKind::Image => Vec::new(),
        ResourceKind::Server => vec![
            text("HTTPS address", "core.https_address"),
            text("Metrics address", "core.metrics_address"),
            toggle("Metrics authentication", "core.metrics_authentication"),
            text("Storage buckets address", "core.storage_buckets_address"),
            text("BGP listen address", "core.bgp_address"),
            text("BGP ASN", "core.bgp_asn"),
            text("BGP router ID", "core.bgp_routerid"),
            text("DNS listen address", "core.dns_address"),
            text("Debug address", "core.debug_address"),
            text("HTTP proxy", "core.proxy_http"),
            text("HTTPS proxy", "core.proxy_https"),
            text("Proxy ignore hosts", "core.proxy_ignore_hosts"),
            text("OVN northbound", "network.ovn.northbound_connection"),
            text("OVN CA certificate", "network.ovn.ca_cert"),
            text("OVN client certificate", "network.ovn.client_cert"),
            text("OVN client key", "network.ovn.client_key"),
            text("Trusted proxies", "core.https_trusted_proxy"),
            text("Remote token expiry", "core.remote_token_expiry"),
            text("Shutdown action", "core.shutdown_action"),
            text("Shutdown timeout", "core.shutdown_timeout"),
            toggle("Trust CA certificates", "core.trust_ca_certificates"),
            toggle("Syslog socket", "core.syslog_socket"),
            toggle("Accept ACME terms", "acme.agree_tos"),
            text("ACME CA URL", "acme.ca_url"),
            text("ACME challenge", "acme.challenge"),
            text("ACME domain", "acme.domain"),
            text("ACME email", "acme.email"),
            text("ACME HTTP listener", "acme.http.port"),
            text("ACME DNS provider", "acme.provider"),
            text("ACME provider environment", "acme.provider.environment"),
            text("ACME DNS resolvers", "acme.provider.resolvers"),
            text("Cluster offline threshold", "cluster.offline_threshold"),
            text("Cluster healing threshold", "cluster.healing_threshold"),
            text("Cluster max voters", "cluster.max_voters"),
            text("Cluster max standby", "cluster.max_standby"),
            text("Rebalance batch", "cluster.rebalance.batch"),
            text("Rebalance cooldown", "cluster.rebalance.cooldown"),
            text("Rebalance interval", "cluster.rebalance.interval"),
            text("Rebalance threshold", "cluster.rebalance.threshold"),
            choice(
                "Image update interval",
                "images.auto_update_interval",
                &["1", "6", "12", "24", "48", "168"],
            ),
            toggle("Auto-update cached images", "images.auto_update_cached"),
            text("Image compression", "images.compression_algorithm"),
            text("Default architecture", "images.default_architecture"),
            choice(
                "Remote cache expiry",
                "images.remote_cache_expiry",
                &["1", "6", "10", "24", "72", "168", "720"],
            ),
            text("OIDC issuer", "oidc.issuer"),
            text("OIDC client ID", "oidc.client.id"),
            text("OIDC audience", "oidc.audience"),
            text("OIDC claim", "oidc.claim"),
            text("OIDC scopes", "oidc.scopes"),
            text("OpenFGA API URL", "authorization.openfga.api.url"),
            secret("OpenFGA API token", "authorization.openfga.api.token"),
            text("OpenFGA store ID", "authorization.openfga.store.id"),
            text("Authorization scriptlet", "authorization.scriptlet"),
            text("Placement scriptlet", "instances.placement.scriptlet"),
            text("NIC host-name policy", "instances.nic.host_name"),
            toggle("Per-instance LXCFS", "instances.lxcfs.per_instance"),
            text("Backup archive volume", "storage.backups_volume"),
            text("Image archive volume", "storage.images_volume"),
            text("Instance logs volume", "storage.logs_volume"),
            text(
                "LINSTOR controller",
                "storage.linstor.controller_connection",
            ),
            text("LINSTOR CA certificate", "storage.linstor.ca_cert"),
            text("LINSTOR client certificate", "storage.linstor.client_cert"),
            text("LINSTOR client key", "storage.linstor.client_key"),
            text("LINSTOR satellite name", "storage.linstor.satellite.name"),
        ],
    }
}

fn config_value(definition: &ResourceDefinition, key: &str) -> String {
    let config = if definition.kind == ResourceKind::Server {
        &definition.value
    } else {
        definition
            .value
            .get("config")
            .unwrap_or(&serde_json::Value::Null)
    };
    config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn config_key_extension(key: &str) -> Option<&'static str> {
    match key {
        "boot.autorestart" => Some("instance_auto_restart"),
        "migration.stateful" => Some("container_migration_stateful"),
        "raw.qemu.scriptlet" => Some("qemu_scriptlet"),
        "raw.qemu.qmp.early" | "raw.qemu.qmp.pre-start" | "raw.qemu.qmp.post-start" => {
            Some("qemu_raw_qmp")
        }
        "ipv4.dhcp.expiry" | "ipv6.dhcp.expiry" => Some("network_dhcp_expiry"),
        "ipv4.dhcp.gateway" => Some("network_dhcp_gateway"),
        "ipv4.l3only" | "ipv6.l3only" => Some("network_ovn_l3only"),
        "ipv4.nat.address" | "ipv6.nat.address" => Some("network_ovn_nat_address"),
        "vlan.tagged" => Some("network_physical_vlan"),
        "authorization.scriptlet" => Some("authorization_scriptlet"),
        "instances.placement.scriptlet" => Some("instances_placement_scriptlet"),
        _ => None,
    }
}

impl DetailForm {
    fn new(name: String, project: String) -> Self {
        Self {
            name,
            project,
            detail: None,
            state: None,
            expanded_diff: false,
        }
    }

    pub fn detail(&self) -> Option<&InstanceDetail> {
        self.detail.as_ref()
    }
    pub fn runtime_rows(&self) -> Vec<(String, String)> {
        let Some(state) = self.state.as_ref() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (label, pointers) in [
            ("Started at", &["/started_at"][..]),
            ("Boot ID", &["/os_info/boot_id", "/boot_id"][..]),
            ("OS", &["/os_info/pretty_name", "/os_info/name"][..]),
            (
                "OS version",
                &["/os_info/version", "/os_info/version_id"][..],
            ),
            (
                "Kernel",
                &["/os_info/kernel_version", "/kernel_version"][..],
            ),
            ("Hostname", &["/os_info/hostname", "/hostname"][..]),
            ("PID", &["/pid"][..]),
            ("Processes", &["/processes"][..]),
            ("Memory usage", &["/memory/usage"][..]),
            ("Memory peak", &["/memory/usage_peak"][..]),
            ("CPU time", &["/cpu/usage"][..]),
        ] {
            if let Some(value) = pointers.iter().find_map(|pointer| state.pointer(pointer)) {
                rows.push((label.into(), display_json_scalar(value)));
            }
        }
        let agent = state
            .pointer("/os_info")
            .is_some_and(|value| value.as_object().is_some_and(|object| !object.is_empty()));
        rows.push((
            "Guest agent".into(),
            if agent { "available" } else { "not reported" }.into(),
        ));
        rows
    }
    pub fn expanded_diff(&self) -> bool {
        self.expanded_diff
    }
}

impl ConsoleForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            instance_type: instance.instance_type.clone(),
            output: String::new(),
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn output(&self) -> &str {
        &self.output
    }
    pub fn vm_notice(&self) -> bool {
        self.instance_type == "virtual-machine"
    }
}

impl BackupForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            name: String::new(),
            instance_only: false,
            optimized_storage: false,
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn instance_only(&self) -> bool {
        self.instance_only
    }
    pub const fn optimized_storage(&self) -> bool {
        self.optimized_storage
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileFocus {
    Path,
    Contents,
}

/// A small in-terminal file reader and editor for an instance filesystem.
#[derive(Clone, Debug)]
pub struct FileForm {
    instance_name: String,
    project: String,
    instance_type: String,
    path: String,
    contents: String,
    focus: FileFocus,
    loaded: bool,
    delete_confirmation: Option<String>,
    transfer: Option<FileTransfer>,
    directory_entries: Vec<String>,
    directory_selected: usize,
    browsing_directory: bool,
}

#[derive(Clone, Debug)]
pub struct FileTransfer {
    upload: bool,
    local_path: String,
}

/// Type-to-confirm guard for an operation cancellation request.
#[derive(Clone, Debug)]
pub struct OperationConfirmation {
    operation_id: String,
    description: String,
    input: String,
}

impl OperationConfirmation {
    fn new(operation_id: String, description: String) -> Self {
        Self {
            operation_id,
            description,
            input: String::new(),
        }
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn expected_phrase(&self) -> String {
        format!("cancel {}", self.operation_id)
    }
}

impl FileForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            instance_type: instance.instance_type.clone(),
            path: "/etc/hostname".to_owned(),
            contents: String::new(),
            focus: FileFocus::Path,
            loaded: false,
            delete_confirmation: None,
            transfer: None,
            directory_entries: Vec::new(),
            directory_selected: 0,
            browsing_directory: false,
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }

    pub fn editing_contents(&self) -> bool {
        self.focus == FileFocus::Contents
    }

    pub const fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn vm_agent_notice(&self) -> bool {
        self.instance_type == "virtual-machine"
    }

    pub fn delete_confirmation(&self) -> Option<&str> {
        self.delete_confirmation.as_deref()
    }

    pub fn delete_phrase(&self) -> String {
        format!("delete {}", self.path)
    }
    pub fn transfer(&self) -> Option<(bool, &str)> {
        self.transfer
            .as_ref()
            .map(|value| (value.upload, value.local_path.as_str()))
    }
    pub fn directory_entries(&self) -> &[String] {
        &self.directory_entries
    }
    pub const fn directory_selected(&self) -> usize {
        self.directory_selected
    }
    pub const fn browsing_directory(&self) -> bool {
        self.browsing_directory
    }
}

impl SnapshotForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            name: String::new(),
            stateful: false,
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn stateful(&self) -> bool {
        self.stateful
    }
}

impl ExecForm {
    fn new(instance: &crate::incus::InstanceSummary) -> Self {
        Self {
            instance_name: instance.name.clone(),
            project: instance.project.clone(),
            instance_type: instance.instance_type.clone(),
            command: String::new(),
            result: None,
        }
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn result(&self) -> Option<&ExecResult> {
        self.result.as_ref()
    }

    pub fn vm_agent_notice(&self) -> bool {
        self.instance_type == "virtual-machine"
    }
}

impl CreateForm {
    const FIELD_NAMES: [&'static str; 17] = [
        "Name",
        "Image",
        "Type",
        "Project",
        "Profiles",
        "CPU limit",
        "Memory limit",
        "Storage pool",
        "Root disk size",
        "Network",
        "Cloud-init data",
        "Target member",
        "Privileged",
        "Nesting",
        "Secure boot",
        "Autostart",
        "Start after create",
    ];

    fn new() -> Self {
        let spec = CreateInstanceSpec::default();
        Self {
            profiles: spec.profiles.join(","),
            spec,
            field: 0,
        }
    }

    fn from_image(image: &crate::incus::ImageSummary) -> Self {
        let mut form = Self::new();
        form.spec.image = image
            .aliases
            .first()
            .map(|alias| alias.name.clone())
            .filter(|alias| !alias.is_empty())
            .unwrap_or_else(|| image.fingerprint.clone());
        form.spec.project = if image.project.is_empty() {
            "default".to_owned()
        } else {
            image.project.clone()
        };
        form.spec.instance_type = if image.image_type == "virtual-machine" {
            "virtual-machine".to_owned()
        } else {
            "container".to_owned()
        };
        form
    }

    pub const fn field(&self) -> usize {
        self.field
    }

    pub const fn field_name(&self) -> &'static str {
        Self::FIELD_NAMES[self.field]
    }

    pub fn value(&self, field: usize) -> String {
        match field {
            0 => self.spec.name.clone(),
            1 => self.spec.image.clone(),
            2 => self.spec.instance_type.clone(),
            3 => self.spec.project.clone(),
            4 => self.profiles.clone(),
            5 => self.spec.cpu_limit.clone(),
            6 => self.spec.memory_limit.clone(),
            7 => self.spec.storage_pool.clone(),
            8 => self.spec.root_disk_size.clone(),
            9 => self.spec.network.clone(),
            10 => self.spec.cloud_init_user_data.clone(),
            11 => self.spec.target_member.clone(),
            12 => if self.spec.privileged { "yes" } else { "no" }.to_owned(),
            13 => if self.spec.nesting { "yes" } else { "no" }.to_owned(),
            14 => if self.spec.secure_boot { "yes" } else { "no" }.to_owned(),
            15 => if self.spec.autostart { "yes" } else { "no" }.to_owned(),
            16 => if self.spec.start_after_create {
                "yes"
            } else {
                "no"
            }
            .to_owned(),
            _ => String::new(),
        }
    }

    fn next(&mut self) {
        self.field = (self.field + 1) % Self::FIELD_NAMES.len();
    }

    fn previous(&mut self) {
        self.field = (self.field + Self::FIELD_NAMES.len() - 1) % Self::FIELD_NAMES.len();
    }

    fn toggle_type(&mut self) {
        self.spec.instance_type = if self.spec.instance_type == "container" {
            "virtual-machine".to_owned()
        } else {
            "container".to_owned()
        };
    }

    fn toggle_start(&mut self) {
        self.spec.start_after_create = !self.spec.start_after_create;
    }

    fn is_choice(field: usize) -> bool {
        matches!(field, 2 | 12..=16)
    }

    fn toggle_choice(&mut self) {
        match self.field {
            2 => self.toggle_type(),
            12 => self.spec.privileged = !self.spec.privileged,
            13 => self.spec.nesting = !self.spec.nesting,
            14 => self.spec.secure_boot = !self.spec.secure_boot,
            15 => self.spec.autostart = !self.spec.autostart,
            16 => self.toggle_start(),
            _ => {}
        }
    }

    fn input(&mut self, character: char) {
        if Self::is_choice(self.field) {
            return;
        }
        self.value_mut().push(character);
    }

    fn backspace(&mut self) {
        if Self::is_choice(self.field) {
            return;
        }
        self.value_mut().pop();
    }

    fn spec(&self) -> CreateInstanceSpec {
        let mut spec = self.spec.clone();
        spec.profiles = self
            .profiles
            .split(',')
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        spec
    }

    fn value_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.spec.name,
            1 => &mut self.spec.image,
            3 => &mut self.spec.project,
            4 => &mut self.profiles,
            5 => &mut self.spec.cpu_limit,
            6 => &mut self.spec.memory_limit,
            7 => &mut self.spec.storage_pool,
            8 => &mut self.spec.root_disk_size,
            9 => &mut self.spec.network,
            10 => &mut self.spec.cloud_init_user_data,
            11 => &mut self.spec.target_member,
            _ => unreachable!("choice fields are handled separately"),
        }
    }
}

impl Confirmation {
    fn new(action: InstanceAction, instance_name: String, project: String) -> Self {
        Self {
            action,
            instance_name,
            project,
            input: String::new(),
        }
    }

    pub const fn action(&self) -> InstanceAction {
        self.action
    }

    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn expected_phrase(&self) -> String {
        format!("{} {}", self.action.as_str(), self.instance_name)
    }

    fn is_confirmed(&self) -> bool {
        self.input == self.expected_phrase()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClusterMemberUpdate {
    pub name: String,
    pub description: String,
    pub roles: Vec<String>,
    pub groups: Vec<String>,
    pub failure_domain: String,
    pub config: std::collections::BTreeMap<String, String>,
}

/// A side effect requested by input handling.
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    None,
    Refresh,
    Connect(Transport),
    PersistRemotes(Vec<SavedRemote>),
    PersistViews(Vec<SavedView>),
    PersistSchedules(Vec<BackupSchedule>),
    ReadFleetHealth(Vec<SavedRemote>),
    ReadOpenFga,
    RefreshOpenFga,
    MutateOpenFga {
        tuple: TupleKey,
        delete: bool,
    },
    CheckOpenFga {
        tuple: TupleKey,
    },
    ReadImageRetention,
    ExportAudit {
        path: String,
        value: serde_json::Value,
    },
    ChangeInstanceState {
        name: String,
        project: String,
        action: InstanceAction,
    },
    CreateInstance(CreateInstanceSpec),
    ExecuteCommand {
        name: String,
        project: String,
        command: String,
    },
    InteractiveExec {
        name: String,
        project: String,
        shell: String,
    },
    InteractiveConsole {
        name: String,
        project: String,
        force: bool,
    },
    VgaConsole {
        name: String,
        project: String,
        force: bool,
    },
    CreateSnapshot {
        name: String,
        project: String,
        snapshot_name: String,
        stateful: bool,
    },
    CreateBackup {
        name: String,
        project: String,
        backup_name: String,
        instance_only: bool,
        optimized_storage: bool,
    },
    ReadConsole {
        name: String,
        project: String,
    },
    ReadInstanceDetail {
        name: String,
        project: String,
    },
    ReadInstanceAccess {
        name: String,
        project: String,
    },
    ReadResourceDefinition {
        kind: ResourceKind,
        name: String,
        project: String,
    },
    ReadLoggingTargets,
    ReadScriptlets,
    ReadQemuScriptlet {
        kind: ResourceKind,
        name: String,
        project: String,
    },
    UpdateScriptlet {
        definition: ResourceDefinition,
        value: serde_json::Value,
        activated_source: String,
    },
    ApplyPreseed(PreseedSpec),
    ImportPreseed {
        path: String,
    },
    ExportPreseed {
        path: String,
        spec: PreseedSpec,
    },
    ReadPoolSafety {
        pool: String,
    },
    ReadQmpManager {
        name: String,
        project: String,
    },
    ApplyBulkInstances {
        targets: Vec<BulkTarget>,
        action: InstanceAction,
        concurrency: usize,
    },
    UpdateResource {
        definition: ResourceDefinition,
        value: serde_json::Value,
    },
    CreateResource {
        kind: ResourceKind,
        project: String,
        value: serde_json::Value,
    },
    DeleteResource {
        kind: ResourceKind,
        name: String,
        project: String,
    },
    CreateImageAlias {
        alias: String,
        fingerprint: String,
        project: String,
        description: String,
    },
    UpdateImageAlias {
        alias: String,
        new_name: String,
        fingerprint: String,
        project: String,
        description: String,
    },
    DeleteImageAlias {
        alias: String,
        project: String,
    },
    UpdateImage(ImageUpdateSpec),
    ImportImage {
        project: String,
        path: String,
        aliases: Vec<String>,
        public: bool,
    },
    ExportImage {
        fingerprint: String,
        project: String,
        path: String,
    },
    PushImage {
        fingerprint: String,
        project: String,
        target: String,
        target_project: String,
        secret: String,
        certificate_path: String,
        aliases: Vec<String>,
        profiles: Vec<String>,
    },
    RefreshImage {
        fingerprint: String,
        project: String,
    },
    CopyImageProject {
        fingerprint: String,
        source_project: String,
        target_project: String,
        public: bool,
    },
    PublishInstanceImage(ImagePublishSpec),
    ReadArtifacts {
        name: String,
        project: String,
    },
    DeleteSnapshot {
        name: String,
        project: String,
        snapshot: String,
    },
    RestoreSnapshot {
        name: String,
        project: String,
        snapshot: String,
        stateful: bool,
    },
    RenameSnapshot {
        name: String,
        project: String,
        snapshot: String,
        new_name: String,
    },
    RenameInstance {
        name: String,
        project: String,
        new_name: String,
    },
    RebuildInstance {
        name: String,
        project: String,
        image: Option<String>,
    },
    ReadInstanceLogs {
        name: String,
        project: String,
    },
    ReadInstanceLog {
        name: String,
        project: String,
        filename: String,
    },
    DeleteInstanceLog {
        name: String,
        project: String,
        filename: String,
    },
    CleanupInstanceExecOutputs {
        name: String,
        project: String,
        filenames: Vec<String>,
    },
    ClearConsoleLog {
        name: String,
        project: String,
    },
    ReadInstanceMaintenance {
        name: String,
        project: String,
    },
    ReadInstanceTemplate {
        name: String,
        project: String,
        template: String,
    },
    UpdateInstanceMetadata {
        name: String,
        project: String,
        metadata: serde_json::Value,
        etag: String,
    },
    WriteInstanceTemplate {
        name: String,
        project: String,
        template: String,
        content: String,
    },
    DeleteInstanceTemplate {
        name: String,
        project: String,
        template: String,
    },
    ExportInstanceMemory {
        name: String,
        project: String,
        path: String,
    },
    ExportInstanceScreenshot {
        name: String,
        project: String,
        path: String,
    },
    RepairInstance {
        name: String,
        project: String,
    },
    CreateInstanceBitmaps {
        name: String,
        project: String,
        spec: StorageVolumeBitmapSpec,
    },
    StartInstanceProtocolProxy {
        name: String,
        project: String,
        protocol: String,
        port: u16,
    },
    MoveInstance(MoveInstanceSpec),
    AcknowledgeWarning {
        uuid: String,
    },
    DeleteWarning {
        uuid: String,
    },
    DeleteCertificate {
        fingerprint: String,
    },
    GenerateCertificateToken {
        name: String,
        restricted: bool,
        projects: Vec<String>,
    },
    AddCertificateFromFile {
        name: String,
        restricted: bool,
        projects: Vec<String>,
        path: String,
    },
    UpdateCertificate {
        fingerprint: String,
        name: String,
        restricted: bool,
        projects: Vec<String>,
    },
    UpdateClusterMember {
        name: String,
        description: String,
        roles: Vec<String>,
        groups: Vec<String>,
        failure_domain: String,
        config: std::collections::BTreeMap<String, String>,
    },
    RenameClusterMember {
        name: String,
        new_name: String,
    },
    ReadClusterMemberState {
        name: String,
    },
    ReadClusterConfiguration,
    ReadMetadataConfiguration,
    ConfigureCluster {
        value: serde_json::Value,
    },
    UpdateClusterCertificate {
        certificate_path: String,
        key_path: String,
    },
    ChangeClusterMemberState {
        name: String,
        action: String,
    },
    DeleteClusterMember {
        name: String,
    },
    GenerateClusterJoinToken {
        name: String,
    },
    ReadClusterGroups,
    ReadProjectUsage {
        project: String,
    },
    ReadProjectAccess {
        project: String,
    },
    CreateClusterGroup {
        name: String,
        description: String,
        members: Vec<String>,
    },
    UpdateClusterGroup {
        name: String,
        description: String,
        members: Vec<String>,
    },
    DeleteClusterGroup {
        name: String,
    },
    RenameClusterGroup {
        name: String,
        new_name: String,
    },
    ReadDeviceManager {
        kind: ResourceKind,
        name: String,
        project: String,
    },
    ReadStorageVolumes {
        pool: String,
    },
    ReadStoragePoolResources {
        pool: String,
    },
    ReadStorageVolumeState {
        pool: String,
        project: String,
        volume: String,
    },
    ReadStorageVolumeBitmaps {
        pool: String,
        project: String,
        volume: String,
    },
    CreateStorageVolumeBitmap {
        pool: String,
        project: String,
        volume: String,
        spec: StorageVolumeBitmapSpec,
    },
    DeleteStorageVolumeBitmap {
        pool: String,
        project: String,
        volume: String,
        bitmap: String,
    },
    CreateStorageVolume(StorageVolumeSpec),
    ImportStorageVolume {
        pool: String,
        project: String,
        name: String,
        import_type: String,
        path: String,
    },
    UpdateStorageVolume(StorageVolumeUpdateSpec),
    AttachStorageVolume(VolumeAttachmentSpec),
    DetachStorageVolume(VolumeDetachmentSpec),
    ReadStorageVolumeSnapshots {
        pool: String,
        project: String,
        volume: String,
    },
    ReadStorageVolumeBackups {
        pool: String,
        project: String,
        volume: String,
    },
    CreateStorageVolumeBackup {
        pool: String,
        project: String,
        volume: String,
        name: String,
        volume_only: bool,
        optimized_storage: bool,
    },
    RenameStorageVolumeBackup {
        pool: String,
        project: String,
        volume: String,
        backup: String,
        new_name: String,
    },
    DeleteStorageVolumeBackup {
        pool: String,
        project: String,
        volume: String,
        backup: String,
    },
    ExportStorageVolumeBackup {
        pool: String,
        project: String,
        volume: String,
        backup: String,
        path: String,
    },
    CreateStorageVolumeSnapshot {
        pool: String,
        project: String,
        volume: String,
        snapshot: String,
    },
    RenameStorageVolumeSnapshot {
        pool: String,
        project: String,
        volume: String,
        snapshot: String,
        new_name: String,
    },
    RestoreStorageVolumeSnapshot {
        pool: String,
        project: String,
        volume: String,
        snapshot: String,
    },
    DeleteStorageVolumeSnapshot {
        pool: String,
        project: String,
        volume: String,
        snapshot: String,
    },
    ReadStorageBuckets {
        pool: String,
    },
    CreateStorageBucket {
        pool: String,
        project: String,
        name: String,
        description: String,
        size: String,
    },
    UpdateStorageBucket {
        pool: String,
        project: String,
        name: String,
        description: String,
        size: String,
        config: std::collections::BTreeMap<String, String>,
    },
    DeleteStorageBucket {
        pool: String,
        project: String,
        name: String,
    },
    ReadStorageBucketKeys {
        pool: String,
        project: String,
        bucket: String,
    },
    ReadStorageBucketBackups {
        pool: String,
        project: String,
        bucket: String,
    },
    CreateStorageBucketBackup {
        pool: String,
        project: String,
        bucket: String,
        name: String,
        compression: String,
    },
    RenameStorageBucketBackup {
        pool: String,
        project: String,
        bucket: String,
        backup: String,
        new_name: String,
    },
    DeleteStorageBucketBackup {
        pool: String,
        project: String,
        bucket: String,
        backup: String,
    },
    ExportStorageBucketBackup {
        pool: String,
        project: String,
        bucket: String,
        backup: String,
        path: String,
    },
    ImportStorageBucket {
        pool: String,
        project: String,
        name: String,
        path: String,
    },
    CreateStorageBucketKey {
        pool: String,
        project: String,
        bucket: String,
        name: String,
        description: String,
        role: String,
    },
    UpdateStorageBucketKey {
        pool: String,
        project: String,
        bucket: String,
        name: String,
        description: String,
        role: String,
    },
    DeleteStorageBucketKey {
        pool: String,
        project: String,
        bucket: String,
        name: String,
    },
    DeleteStorageVolume {
        pool: String,
        project: String,
        name: String,
    },
    RebuildStorageVolume {
        pool: String,
        project: String,
        name: String,
    },
    CopyStorageVolume {
        source_pool: String,
        source_project: String,
        source_name: String,
        target_pool: String,
        target_project: String,
        target_name: String,
        volume_only: bool,
        refresh: bool,
        live: bool,
        refresh_exclude_older: bool,
    },
    StartStorageVolumeProtocolProxy {
        pool: String,
        project: String,
        volume: String,
        protocol: String,
        port: u16,
    },
    ReadStorageVolumeFile {
        pool: String,
        project: String,
        volume: String,
        path: String,
    },
    ListStorageVolumeDirectory {
        pool: String,
        project: String,
        volume: String,
        path: String,
    },
    WriteStorageVolumeFile {
        pool: String,
        project: String,
        volume: String,
        path: String,
        content: String,
    },
    DeleteStorageVolumeFile {
        pool: String,
        project: String,
        volume: String,
        path: String,
    },
    PullStorageVolumeFile {
        pool: String,
        project: String,
        volume: String,
        path: String,
        local_path: String,
    },
    PushStorageVolumeFile {
        pool: String,
        project: String,
        volume: String,
        path: String,
        local_path: String,
    },
    ReadNetworkServices {
        network: String,
        project: String,
    },
    ReadNetworkState {
        network: String,
        project: String,
    },
    CreateNetworkForward(NetworkForwardSpec),
    UpdateNetworkForward(NetworkForwardUpdateSpec),
    DeleteNetworkForward {
        network: String,
        project: String,
        listen_address: String,
    },
    CreateNetworkAcl(NetworkAclSpec),
    UpdateNetworkAcl(NetworkAclSummary),
    DeleteNetworkAcl {
        network: String,
        project: String,
        name: String,
    },
    ReadNetworkAclLog {
        network: String,
        project: String,
        name: String,
    },
    RenameNetworkAcl {
        network: String,
        project: String,
        name: String,
        new_name: String,
    },
    CreateNetworkLoadBalancer(NetworkLoadBalancerSpec),
    UpdateNetworkLoadBalancer(NetworkLoadBalancerUpdateSpec),
    DeleteNetworkLoadBalancer {
        network: String,
        project: String,
        listen_address: String,
    },
    ReadNetworkLoadBalancerState {
        network: String,
        project: String,
        address: String,
    },
    CreateNetworkPeer(NetworkPeerSpec),
    UpdateNetworkPeer(NetworkPeerUpdateSpec),
    DeleteNetworkPeer {
        network: String,
        project: String,
        name: String,
    },
    ReadNetworkZones,
    ReadBgpConfiguration {
        network: String,
        project: String,
    },
    ReadNetworkAddressSets,
    ReadNetworkIntegrations,
    CreateNetworkIntegration {
        name: String,
        integration_type: String,
        description: String,
        config: std::collections::BTreeMap<String, String>,
    },
    UpdateNetworkIntegration {
        name: String,
        description: String,
        config: std::collections::BTreeMap<String, String>,
    },
    DeleteNetworkIntegration {
        name: String,
    },
    RenameNetworkIntegration {
        name: String,
        new_name: String,
    },
    CreateNetworkAddressSet {
        project: String,
        name: String,
        description: String,
        addresses: Vec<String>,
    },
    UpdateNetworkAddressSet {
        project: String,
        name: String,
        description: String,
        addresses: Vec<String>,
        config: std::collections::BTreeMap<String, String>,
    },
    DeleteNetworkAddressSet {
        project: String,
        name: String,
    },
    RenameNetworkAddressSet {
        project: String,
        name: String,
        new_name: String,
    },
    CreateNetworkZone {
        project: String,
        name: String,
        description: String,
    },
    UpdateNetworkZone {
        project: String,
        name: String,
        description: String,
        config: std::collections::BTreeMap<String, String>,
    },
    DeleteNetworkZone {
        project: String,
        name: String,
    },
    RenameNetworkZone {
        project: String,
        name: String,
        new_name: String,
    },
    ReadNetworkZoneRecords {
        project: String,
        zone: String,
    },
    CreateNetworkZoneRecord {
        project: String,
        zone: String,
        name: String,
        description: String,
        entry: NetworkZoneRecordEntry,
    },
    UpdateNetworkZoneRecord {
        project: String,
        zone: String,
        name: String,
        description: String,
        entries: Vec<NetworkZoneRecordEntry>,
    },
    DeleteNetworkZoneRecord {
        project: String,
        zone: String,
        name: String,
    },
    DeleteBackup {
        name: String,
        project: String,
        backup: String,
    },
    ExportBackup {
        name: String,
        project: String,
        backup: String,
        path: String,
    },
    ImportBackup {
        project: String,
        path: String,
    },
    ReadFile {
        name: String,
        project: String,
        path: String,
    },
    ListDirectory {
        name: String,
        project: String,
        path: String,
    },
    WriteFile {
        name: String,
        project: String,
        path: String,
        contents: String,
    },
    DeleteFile {
        name: String,
        project: String,
        path: String,
    },
    PullFile {
        name: String,
        project: String,
        path: String,
        local_path: String,
    },
    PushFile {
        name: String,
        project: String,
        path: String,
        local_path: String,
    },
    CancelOperation {
        id: String,
    },
}

impl AppCommand {
    /// Returns a payload-free action name for commands that can change state or
    /// create an operator-visible artifact. Debug payloads are never persisted.
    pub fn journal_action(&self) -> Option<String> {
        let debug = format!("{self:?}");
        let variant = debug.split([' ', '{', '(']).next().unwrap_or_default();
        const PREFIXES: &[&str] = &[
            "Create",
            "Update",
            "Delete",
            "Rename",
            "Restore",
            "Rebuild",
            "Apply",
            "Change",
            "Write",
            "Push",
            "Import",
            "Export",
            "Repair",
            "Attach",
            "Detach",
            "Move",
            "Cleanup",
            "Clear",
            "Acknowledge",
            "Generate",
            "Start",
            "Publish",
            "Copy",
            "RefreshImage",
            "Execute",
            "Interactive",
            "Vga",
        ];
        PREFIXES
            .iter()
            .any(|prefix| variant.starts_with(prefix))
            .then(|| variant.to_owned())
    }
}

impl App {
    /// Whether the workspace should receive nastty's dimmed modal backdrop.
    pub fn overlay_visible(&self) -> bool {
        self.model.last_error.is_some()
            || self.filter_form.is_some()
            || self.action_menu.is_some()
            || self.connection_form.is_some()
            || self.view_manager.is_some()
            || self.audit_export.is_some()
            || self.journal_viewer.is_some()
            || self.fleet_manager.is_some()
            || self.placement_inspector.is_some()
            || self.backup_schedule_form.is_some()
            || self.openfga_manager.is_some()
            || self.image_retention_preview.is_some()
            || self.cluster_group_manager.is_some()
            || self.move_form.is_some()
            || self.project_usage.is_some()
            || self.project_access.is_some()
            || self.instance_access.is_some()
            || self.confirmation.is_some()
            || self.create_form.is_some()
            || self.exec_form.is_some()
            || self.snapshot_form.is_some()
            || self.backup_form.is_some()
            || self.console_form.is_some()
            || self.detail_form.is_some()
            || self.config_form.is_some()
            || self.resource_create_form.is_some()
            || self.resource_delete_confirmation.is_some()
            || self.image_alias_form.is_some()
            || self.image_alias_manager.is_some()
            || self.image_form.is_some()
            || self.artifacts_form.is_some()
            || self.operation_detail.is_some()
            || self.rename_form.is_some()
            || self.rebuild_form.is_some()
            || self.log_manager.is_some()
            || self.allocation_explorer.is_some()
            || self.metrics_explorer.is_some()
            || self.maintenance_manager.is_some()
            || self.storage_resource_detail.is_some()
            || self.settings_manager.is_some()
            || self.logging_target_manager.is_some()
            || self.scriptlet_manager.is_some()
            || self.preseed_manager.is_some()
            || self.pool_safety_manager.is_some()
            || self.qmp_manager.is_some()
            || self.bulk_manager.is_some()
            || self.device_manager.is_some()
            || self.volume_manager.is_some()
            || self.bucket_manager.is_some()
            || self.network_services.is_some()
            || self.zone_manager.is_some()
            || self.bgp_form.is_some()
            || self.address_set_manager.is_some()
            || self.integration_manager.is_some()
            || self.file_form.is_some()
            || self.operation_confirmation.is_some()
            || self.help_visible
    }
    pub fn supports_extension(&self, extension: &str) -> bool {
        self.model
            .server
            .as_ref()
            .is_some_and(|server| server.api_extensions.iter().any(|value| value == extension))
    }

    fn require_extension(&mut self, extension: &str, feature: &str) -> bool {
        if self.supports_extension(extension) {
            true
        } else {
            self.model.last_error = Some(format!(
                "{feature} is unavailable: this Incus server does not advertise the {extension} API extension"
            ));
            false
        }
    }
    pub fn new(target: String) -> Self {
        Self {
            target,
            page: Page::Dashboard,
            selected_instance: 0,
            selected_operation: 0,
            selected_storage_pool: 0,
            selected_network: 0,
            selected_project: 0,
            active_project: "default".to_owned(),
            selected_profile: 0,
            selected_image: 0,
            filter_query: String::new(),
            filter_form: None,
            action_menu: None,
            connection_form: None,
            saved_remotes: Vec::new(),
            saved_views: Vec::new(),
            view_manager: None,
            audit_export: None,
            action_journal: Vec::new(),
            journal_viewer: None,
            fleet_manager: None,
            placement_inspector: None,
            backup_schedules: Vec::new(),
            backup_schedule_form: None,
            openfga_manager: None,
            image_retention_preview: None,
            cluster_group_manager: None,
            move_form: None,
            project_usage: None,
            project_access: None,
            instance_access: None,
            should_quit: false,
            confirmation: None,
            create_form: None,
            exec_form: None,
            snapshot_form: None,
            backup_form: None,
            console_form: None,
            detail_form: None,
            config_form: None,
            focus_config_key: None,
            resource_create_form: None,
            resource_delete_confirmation: None,
            image_alias_form: None,
            image_alias_manager: None,
            image_form: None,
            artifacts_form: None,
            operation_detail: None,
            rename_form: None,
            rebuild_form: None,
            log_manager: None,
            allocation_explorer: None,
            metrics_explorer: None,
            maintenance_manager: None,
            storage_resource_detail: None,
            settings_manager: None,
            logging_target_manager: None,
            scriptlet_manager: None,
            preseed_manager: None,
            pool_safety_manager: None,
            qmp_manager: None,
            bulk_manager: None,
            bulk_progress: None,
            bulk_cancel: None,
            device_manager: None,
            volume_manager: None,
            bucket_manager: None,
            network_services: None,
            zone_manager: None,
            bgp_form: None,
            address_set_manager: None,
            integration_manager: None,
            file_form: None,
            operation_confirmation: None,
            help_visible: false,
            model: AppModel::default(),
        }
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub const fn page(&self) -> Page {
        self.page
    }

    pub const fn selected_instance(&self) -> usize {
        self.selected_instance
    }

    pub const fn selected_operation(&self) -> usize {
        self.selected_operation
    }

    pub const fn selected_storage_pool(&self) -> usize {
        self.selected_storage_pool
    }

    pub const fn selected_network(&self) -> usize {
        self.selected_network
    }

    pub const fn selected_project(&self) -> usize {
        self.selected_project
    }
    pub fn active_project(&self) -> &str {
        &self.active_project
    }

    pub const fn selected_profile(&self) -> usize {
        self.selected_profile
    }

    pub const fn selected_image(&self) -> usize {
        self.selected_image
    }

    pub fn filter_query(&self) -> &str {
        &self.filter_query
    }

    pub fn filter_form(&self) -> Option<&str> {
        self.filter_form.as_deref()
    }

    pub fn action_menu(&self) -> Option<&ActionMenu> {
        self.action_menu.as_ref()
    }
    pub fn connection_form(&self) -> Option<&ConnectionForm> {
        self.connection_form.as_ref()
    }
    pub fn cluster_group_manager(&self) -> Option<&ClusterGroupManager> {
        self.cluster_group_manager.as_ref()
    }
    pub fn move_form(&self) -> Option<&MoveForm> {
        self.move_form.as_ref()
    }
    pub fn project_usage(&self) -> Option<&ProjectUsageForm> {
        self.project_usage.as_ref()
    }
    pub fn project_access(&self) -> Option<&ProjectAccessForm> {
        self.project_access.as_ref()
    }
    pub fn instance_access(&self) -> Option<&InstanceAccessForm> {
        self.instance_access.as_ref()
    }
    pub fn set_target(&mut self, target: String) {
        self.target = target;
        self.connection_form = None;
        self.active_project = "default".to_owned();
        self.model = AppModel::default();
    }

    pub fn set_saved_remotes(&mut self, remotes: Vec<SavedRemote>) {
        self.saved_remotes = remotes;
    }
    pub fn set_saved_views(&mut self, views: Vec<SavedView>) {
        self.saved_views = views;
    }
    pub fn view_manager(&self) -> Option<&SavedViewManager> {
        self.view_manager.as_ref()
    }
    pub fn audit_export(&self) -> Option<&AuditExportForm> {
        self.audit_export.as_ref()
    }
    pub fn journal_viewer(&self) -> Option<&JournalViewer> {
        self.journal_viewer.as_ref()
    }
    pub fn fleet_manager(&self) -> Option<&FleetManager> {
        self.fleet_manager.as_ref()
    }
    pub fn placement_inspector(&self) -> Option<&PlacementInspector> {
        self.placement_inspector.as_ref()
    }
    pub fn backup_schedule_form(&self) -> Option<&BackupScheduleForm> {
        self.backup_schedule_form.as_ref()
    }
    pub fn openfga_manager(&self) -> Option<&OpenFgaManager> {
        self.openfga_manager.as_ref()
    }
    pub fn image_retention_preview(&self) -> Option<&ImageRetentionPreview> {
        self.image_retention_preview.as_ref()
    }
    pub fn load_image_retention(&mut self, definition: ResourceDefinition) {
        let expiry_hours = definition
            .value
            .get("images.remote_cache_expiry")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| value.parse().ok())
            .unwrap_or(10);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0);
        let mut rows = self
            .model
            .images
            .iter()
            .filter(|image| image.cached)
            .map(|image| {
                let expires = parse_rfc3339_timestamp(&image.uploaded_at)
                    .map(|timestamp| timestamp.saturating_add((expiry_hours * 3600) as i64));
                (
                    image.display_name(),
                    expires
                        .map(|timestamp| format!("unix:{timestamp}"))
                        .unwrap_or_else(|| "unknown upload time".into()),
                    expires.is_some_and(|timestamp| timestamp <= now),
                )
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.1.cmp(&right.1));
        self.image_retention_preview = Some(ImageRetentionPreview {
            expiry_hours,
            rows,
            selected: 0,
        });
    }
    pub fn load_openfga(&mut self, definition: ResourceDefinition, tuples: Vec<TupleKey>) {
        let get = |key| {
            definition
                .value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        self.openfga_manager = Some(OpenFgaManager {
            url: get("authorization.openfga.api.url"),
            store: get("authorization.openfga.store.id"),
            token: get("authorization.openfga.api.token"),
            tuples,
            selected: 0,
            fields: [
                "user".into(),
                String::new(),
                "viewer".into(),
                "project".into(),
                String::new(),
            ],
            field: 0,
            operation: 0,
            confirmation: String::new(),
            confirming: false,
            result: String::new(),
        });
    }
    pub fn openfga_connection(&self) -> Option<(String, String, String)> {
        let manager = self.openfga_manager.as_ref()?;
        Some((
            manager.url.clone(),
            manager.store.clone(),
            manager.token.clone(),
        ))
    }
    pub fn load_openfga_tuples(&mut self, tuples: Vec<TupleKey>) {
        if let Some(manager) = self.openfga_manager.as_mut() {
            manager.tuples = tuples;
            manager.selected = manager.selected.min(manager.tuples.len().saturating_sub(1));
            manager.result = "relationships refreshed".into();
        }
    }
    pub fn set_openfga_result(&mut self, result: String) {
        if let Some(manager) = self.openfga_manager.as_mut() {
            manager.result = result;
        }
    }
    pub fn set_backup_schedules(&mut self, schedules: Vec<BackupSchedule>) {
        self.backup_schedules = schedules;
    }
    pub fn take_due_backups(&mut self) -> Vec<(String, String, String, bool, bool)> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        self.backup_schedules
            .iter_mut()
            .filter(|schedule| schedule.due(now))
            .map(|schedule| {
                schedule.last_run = Some(now);
                schedule.last_result = "submitted".into();
                (
                    schedule.instance.clone(),
                    schedule.project.clone(),
                    schedule.backup_name(now),
                    schedule.instance_only,
                    schedule.optimized_storage,
                )
            })
            .collect()
    }
    pub fn backup_schedules(&self) -> &[BackupSchedule] {
        &self.backup_schedules
    }
    pub fn load_fleet_health(&mut self, rows: Vec<FleetRow>) {
        if let Some(manager) = self.fleet_manager.as_mut() {
            manager.rows = rows;
            manager.loading = false;
            manager.selected = manager.selected.min(manager.rows.len().saturating_sub(1));
        }
    }
    pub fn set_action_journal(&mut self, entries: Vec<ActionEntry>) {
        self.action_journal = entries;
    }
    pub fn journal_rows(&self) -> Vec<&ActionEntry> {
        let filter = self
            .journal_viewer
            .as_ref()
            .map(|viewer| viewer.filter.to_ascii_lowercase())
            .unwrap_or_default();
        self.action_journal
            .iter()
            .rev()
            .filter(|entry| {
                filter.is_empty()
                    || entry.action.to_ascii_lowercase().contains(&filter)
                    || entry.target.to_ascii_lowercase().contains(&filter)
                    || entry.status.to_ascii_lowercase().contains(&filter)
            })
            .collect()
    }
    pub fn record_submitted_action(&mut self, command: &AppCommand) -> Option<Vec<ActionEntry>> {
        let action = command.journal_action()?;
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or(0);
        self.action_journal.push(ActionEntry {
            timestamp,
            target: self.target.clone(),
            action,
            status: "submitted".into(),
        });
        if self.action_journal.len() > 2_000 {
            self.action_journal
                .drain(..self.action_journal.len() - 2_000);
        }
        Some(self.action_journal.clone())
    }

    pub fn matches_filter<'a>(&self, values: impl IntoIterator<Item = &'a str>) -> bool {
        if self.filter_query.is_empty() {
            return true;
        }
        let query = self.filter_query.to_ascii_lowercase();
        values
            .into_iter()
            .any(|value| value.to_ascii_lowercase().contains(&query))
    }

    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub const fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn confirmation(&self) -> Option<&Confirmation> {
        self.confirmation.as_ref()
    }

    pub fn create_form(&self) -> Option<&CreateForm> {
        self.create_form.as_ref()
    }

    pub fn exec_form(&self) -> Option<&ExecForm> {
        self.exec_form.as_ref()
    }

    pub fn snapshot_form(&self) -> Option<&SnapshotForm> {
        self.snapshot_form.as_ref()
    }

    pub fn backup_form(&self) -> Option<&BackupForm> {
        self.backup_form.as_ref()
    }

    pub fn console_form(&self) -> Option<&ConsoleForm> {
        self.console_form.as_ref()
    }

    pub fn detail_form(&self) -> Option<&DetailForm> {
        self.detail_form.as_ref()
    }

    pub fn config_form(&self) -> Option<&ConfigForm> {
        self.config_form.as_ref()
    }

    pub fn resource_create_form(&self) -> Option<&ResourceCreateForm> {
        self.resource_create_form.as_ref()
    }

    pub fn resource_delete_confirmation(&self) -> Option<&ResourceDeleteConfirmation> {
        self.resource_delete_confirmation.as_ref()
    }

    pub fn image_alias_form(&self) -> Option<&ImageAliasForm> {
        self.image_alias_form.as_ref()
    }
    pub fn image_alias_manager(&self) -> Option<&ImageAliasManager> {
        self.image_alias_manager.as_ref()
    }

    pub fn image_form(&self) -> Option<&ImageForm> {
        self.image_form.as_ref()
    }

    pub fn artifacts_form(&self) -> Option<&ArtifactsForm> {
        self.artifacts_form.as_ref()
    }

    pub fn operation_detail(&self) -> Option<&OperationDetailForm> {
        self.operation_detail.as_ref()
    }

    pub fn rename_form(&self) -> Option<&RenameForm> {
        self.rename_form.as_ref()
    }

    pub fn rebuild_form(&self) -> Option<&RebuildForm> {
        self.rebuild_form.as_ref()
    }

    pub fn log_manager(&self) -> Option<&InstanceLogManager> {
        self.log_manager.as_ref()
    }

    pub fn allocation_explorer(&self) -> Option<&NetworkAllocationExplorer> {
        self.allocation_explorer.as_ref()
    }
    pub fn metrics_explorer(&self) -> Option<&MetricsExplorer> {
        self.metrics_explorer.as_ref()
    }
    pub fn maintenance_manager(&self) -> Option<&InstanceMaintenanceManager> {
        self.maintenance_manager.as_ref()
    }
    pub fn storage_resource_detail(&self) -> Option<&str> {
        self.storage_resource_detail.as_deref()
    }

    pub fn settings_manager(&self) -> Option<&SettingsManager> {
        self.settings_manager.as_ref()
    }

    pub fn logging_target_manager(&self) -> Option<&LoggingTargetManager> {
        self.logging_target_manager.as_ref()
    }

    pub fn scriptlet_manager(&self) -> Option<&ScriptletManager> {
        self.scriptlet_manager.as_ref()
    }

    pub fn preseed_manager(&self) -> Option<&PreseedManager> {
        self.preseed_manager.as_ref()
    }

    pub fn pool_safety_manager(&self) -> Option<&PoolSafetyManager> {
        self.pool_safety_manager.as_ref()
    }

    pub fn qmp_manager(&self) -> Option<&QmpManager> {
        self.qmp_manager.as_ref()
    }

    pub fn bulk_manager(&self) -> Option<&BulkManager> {
        self.bulk_manager.as_ref()
    }

    pub fn device_manager(&self) -> Option<&DeviceManager> {
        self.device_manager.as_ref()
    }

    pub fn volume_manager(&self) -> Option<&VolumeManager> {
        self.volume_manager.as_ref()
    }

    pub fn bucket_manager(&self) -> Option<&BucketManager> {
        self.bucket_manager.as_ref()
    }

    pub fn network_services(&self) -> Option<&NetworkServicesManager> {
        self.network_services.as_ref()
    }
    pub fn zone_manager(&self) -> Option<&ZoneManager> {
        self.zone_manager.as_ref()
    }
    pub fn bgp_form(&self) -> Option<&BgpForm> {
        self.bgp_form.as_ref()
    }
    pub fn address_set_manager(&self) -> Option<&AddressSetManager> {
        self.address_set_manager.as_ref()
    }
    pub fn integration_manager(&self) -> Option<&IntegrationManager> {
        self.integration_manager.as_ref()
    }

    pub fn file_form(&self) -> Option<&FileForm> {
        self.file_form.as_ref()
    }

    pub fn operation_confirmation(&self) -> Option<&OperationConfirmation> {
        self.operation_confirmation.as_ref()
    }

    /// Shows only actions that are valid for the currently selected instance state.
    pub fn selected_action_hint(&self) -> &'static str {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return "";
        };
        match instance.status.as_str() {
            "Running" => "x stop   R restart   f freeze",
            "Frozen" => "x stop   u unfreeze",
            "Stopped" => "s start   d delete",
            _ => "",
        }
    }

    /// Stores a recent event and tells the event loop whether data should be refreshed.
    pub fn record_event(&mut self, event: EventEntry) -> bool {
        let needs_refresh = event.needs_refresh();
        self.model.events.insert(0, event);
        self.model.events.truncate(200);
        needs_refresh
    }

    /// Loads the read-only dashboard snapshot. A failure remains visible in the UI.
    pub async fn refresh(&mut self, client: &IncusClient) {
        let loaded = tokio::try_join!(
            client.server_info(),
            client.resources(),
            client.instances(),
            client.projects(),
            client.operations(),
            client.storage_pools(),
            client.networks(),
            client.images(),
            client.profiles(),
            client.warnings(),
            client.certificates()
        );
        match loaded {
            Ok((
                server,
                host_resources,
                instances,
                projects,
                operations,
                storage_pools,
                networks,
                images,
                profiles,
                warnings,
                certificates,
            )) => {
                let clustered = server.environment.server_clustered;
                self.model.server = Some(server);
                self.model.host_resources = Some(host_resources);
                self.model.metrics = client.metrics().await.ok();
                self.model.network_allocations =
                    client.network_allocations().await.unwrap_or_default();
                self.model.instances = instances;
                self.model.projects = projects;
                self.model.operations = operations.into_recent();
                self.model.storage_pools = storage_pools;
                self.model.networks = networks;
                self.model.images = images;
                self.model.profiles = profiles;
                self.model.warnings = warnings;
                self.model.certificates = certificates;
                if clustered {
                    match client.cluster_members().await {
                        Ok(members) => self.model.cluster_members = members,
                        Err(error) => {
                            self.model.last_error = Some(error.to_string());
                            return;
                        }
                    }
                } else {
                    self.model.cluster_members.clear();
                }
                self.model.refreshed_at = Some(SystemTime::now());
                self.model.last_error = None;
                self.clamp_selection();
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Executes a safe instance action only after the confirmation flow emitted a command.
    pub async fn execute_instance_action(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        action: InstanceAction,
    ) {
        let result = if action == InstanceAction::Delete {
            client.delete_instance(&name, &project).await
        } else {
            client.change_instance_state(&name, &project, action).await
        };
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn rename_instance(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        new_name: String,
    ) {
        match client.rename_instance(&name, &project, &new_name).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.rename_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn rebuild_instance(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        image: Option<String>,
    ) {
        match client
            .rebuild_instance(&name, &project, image.as_deref())
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.rebuild_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_instance_logs(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
    ) {
        match tokio::try_join!(
            client.instance_logs(&name, &project),
            client.instance_exec_output_logs(&name, &project)
        ) {
            Ok((logs, exec_outputs)) => {
                let mut logs = logs
                    .into_iter()
                    .map(|log| log.rsplit('/').next().unwrap_or(&log).to_owned())
                    .collect::<Vec<_>>();
                logs.extend(
                    exec_outputs.into_iter().map(|log| {
                        format!("exec-output/{}", log.rsplit('/').next().unwrap_or(&log))
                    }),
                );
                self.log_manager = Some(InstanceLogManager {
                    name,
                    project,
                    logs,
                    selected: 0,
                    content: None,
                    delete_input: None,
                    clear_console_input: None,
                    cleanup_exec_input: None,
                });
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_instance_log(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        filename: String,
    ) {
        let result = if let Some(filename) = filename.strip_prefix("exec-output/") {
            client
                .instance_exec_output_log(&name, &project, filename)
                .await
        } else {
            client.instance_log(&name, &project, &filename).await
        };
        match result {
            Ok(content) => {
                if let Some(manager) = self.log_manager.as_mut() {
                    manager.content = Some(content);
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_instance_log(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        filename: String,
    ) {
        let result = if let Some(filename) = filename.strip_prefix("exec-output/") {
            client
                .delete_instance_exec_output_log(&name, &project, filename)
                .await
        } else {
            client.delete_instance_log(&name, &project, &filename).await
        };
        match result {
            Ok(_) => self.load_instance_logs(client, name, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn cleanup_instance_exec_outputs(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        filenames: Vec<String>,
    ) {
        let mut failures = Vec::new();
        for filename in filenames {
            if let Err(error) = client
                .delete_instance_exec_output_log(&name, &project, &filename)
                .await
            {
                failures.push(format!("{filename}: {error}"));
            }
        }
        self.load_instance_logs(client, name, project).await;
        if !failures.is_empty() {
            self.model.last_error = Some(format!(
                "exec-output cleanup partially failed: {}",
                failures.join("; ")
            ));
        }
    }
    pub async fn clear_console_log(&mut self, client: &IncusClient, name: String, project: String) {
        match client.clear_console_log(&name, &project).await {
            Ok(_) => {
                if let Some(manager) = self.log_manager.as_mut() {
                    manager.clear_console_input = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn export_instance_screenshot(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
    ) {
        let result = async {
            let png = client.console_screenshot(&name, &project).await?;
            tokio::fs::write(&path, png).await.map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.screenshot_path = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_instance_maintenance(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
    ) {
        match tokio::try_join!(
            client.instance_metadata(&name, &project),
            client.instance_template_names(&name, &project)
        ) {
            Ok(((metadata, etag), templates)) => {
                self.maintenance_manager = Some(InstanceMaintenanceManager {
                    name,
                    project,
                    tab: MaintenanceTab::Metadata,
                    metadata,
                    metadata_etag: etag,
                    templates,
                    selected: 0,
                    metadata_editor: None,
                    template_editor: None,
                    template_permissions: None,
                    delete_input: None,
                    dump_path: None,
                    screenshot_path: None,
                    repair_input: None,
                    protocol_form: None,
                    proxy_address: None,
                    bitmap_form: None,
                });
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_instance_template(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        template: String,
    ) {
        match client.instance_template(&name, &project, &template).await {
            Ok(content) => {
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.template_editor = Some((template, content, 1));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_instance_metadata(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        metadata: serde_json::Value,
        etag: String,
    ) {
        match client
            .update_instance_metadata(&name, &project, &metadata, &etag)
            .await
        {
            Ok(_) => self.load_instance_maintenance(client, name, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn write_instance_template(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        template: String,
        content: String,
    ) {
        match client
            .write_instance_template(&name, &project, &template, content)
            .await
        {
            Ok(_) => self.load_instance_maintenance(client, name, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_instance_template(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        template: String,
    ) {
        match client
            .delete_instance_template(&name, &project, &template)
            .await
        {
            Ok(_) => self.load_instance_maintenance(client, name, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn export_instance_memory(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
    ) {
        let result = async {
            let bytes = client.instance_memory_dump(&name, &project).await?;
            tokio::fs::write(path, bytes).await.map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.dump_path = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn repair_instance(&mut self, client: &IncusClient, name: String, project: String) {
        match client.repair_instance(&name, &project).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.repair_input = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn create_instance_bitmaps(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        spec: StorageVolumeBitmapSpec,
    ) {
        match client.create_instance_bitmaps(&name, &project, &spec).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.bitmap_form = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn start_instance_protocol_proxy(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        protocol: String,
        port: u16,
    ) {
        match client
            .start_instance_protocol_proxy(&name, &project, &protocol, port)
            .await
        {
            Ok(address) => {
                if let Some(manager) = self.maintenance_manager.as_mut() {
                    manager.protocol_form = None;
                    manager.proxy_address = Some(format!("{protocol}://{address}"));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn move_instance(&mut self, client: &IncusClient, spec: MoveInstanceSpec) {
        match client.move_instance(&spec).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.move_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Creates an instance while keeping the form open on errors for correction.
    pub async fn create_instance(&mut self, client: &IncusClient, spec: CreateInstanceSpec) {
        match client.create_instance(&spec).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.create_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Runs a one-shot command and leaves stdout/stderr in the command modal.
    pub async fn execute_command(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        command: String,
    ) {
        match client.execute_command(&name, &project, &command).await {
            Ok(result) => {
                if let Some(form) = self.exec_form.as_mut() {
                    form.result = Some(result);
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub fn record_interactive_result(
        &mut self,
        result: Result<crate::incus::Operation, IncusError>,
    ) {
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Creates a snapshot and refreshes the rest of the TUI through operation tracking.
    pub async fn create_snapshot(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        snapshot_name: String,
        stateful: bool,
    ) {
        match client
            .create_snapshot(&name, &project, &snapshot_name, stateful)
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.snapshot_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_backup(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        backup_name: String,
        instance_only: bool,
        optimized_storage: bool,
    ) {
        match client
            .create_backup(
                &name,
                &project,
                &backup_name,
                instance_only,
                optimized_storage,
            )
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.backup_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn read_artifacts(&mut self, client: &IncusClient, name: String, project: String) {
        match tokio::try_join!(
            client.snapshots(&name, &project),
            client.backups(&name, &project)
        ) {
            Ok((snapshots, backups)) => {
                if let Some(form) = self.artifacts_form.as_mut() {
                    form.snapshots = snapshots;
                    form.backups = backups;
                    form.clamp();
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_snapshot(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        snapshot: String,
    ) {
        match client.delete_snapshot(&name, &project, &snapshot).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(form) = self.artifacts_form.as_mut() {
                    form.delete_input = None;
                    form.snapshots.retain(|item| item.name != snapshot);
                    form.clamp();
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn restore_snapshot(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        snapshot: String,
        stateful: bool,
    ) {
        match client
            .restore_snapshot(&name, &project, &snapshot, stateful)
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn rename_snapshot(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        snapshot: String,
        new_name: String,
    ) {
        match client
            .rename_snapshot(&name, &project, &snapshot, &new_name)
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.read_artifacts(client, name, project).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_backup(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        backup: String,
    ) {
        match client.delete_backup(&name, &project, &backup).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(form) = self.artifacts_form.as_mut() {
                    form.delete_input = None;
                    form.backups.retain(|item| item.name != backup);
                    form.clamp();
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn export_backup(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        backup: String,
        path: String,
    ) {
        let result = async {
            let archive = client.export_backup(&name, &project, &backup).await?;
            tokio::fs::write(&path, archive)
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self.artifacts_form.as_mut() {
                    form.transfer = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn import_backup(&mut self, client: &IncusClient, project: String, path: String) {
        let result = async {
            let archive = tokio::fs::read(&path).await.map_err(IncusError::Io)?;
            client.import_backup(&project, archive).await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.artifacts_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn read_console(&mut self, client: &IncusClient, name: String, project: String) {
        match client.console_log(&name, &project).await {
            Ok(output) => {
                if let Some(form) = self.console_form.as_mut() {
                    form.output = output;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn read_instance_detail(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
    ) {
        let (detail, state) = tokio::join!(
            client.instance_detail(&name, &project),
            client.instance_state(&name, &project)
        );
        match detail {
            Ok(detail) => {
                if let Some(form) = self.detail_form.as_mut() {
                    form.detail = Some(detail);
                    form.state = state.ok();
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_instance_access(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
    ) {
        match client.instance_access(&name, &project).await {
            Ok(entries) => {
                self.instance_access = Some(InstanceAccessForm {
                    name,
                    project,
                    entries,
                });
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Opens a fetched definition in its structured, ETag-guarded editor.
    pub fn load_configuration_form(&mut self, definition: ResourceDefinition) {
        let mut form = ConfigForm::new(definition);
        let capabilities_known = self.model.server.is_some();
        let extensions = self
            .model
            .server
            .as_ref()
            .map(|server| server.api_extensions.as_slice())
            .unwrap_or_default();
        let mut supported = Vec::with_capacity(form.fields.len());
        for field in std::mem::take(&mut form.fields) {
            let extension = match field.key {
                ConfigKey::Config(key) => config_key_extension(key),
                ConfigKey::Description => None,
            };
            if capabilities_known
                && let Some(extension) = extension
                && !extensions.iter().any(|candidate| candidate == extension)
            {
                if !form.omitted_capabilities.contains(&extension) {
                    form.omitted_capabilities.push(extension);
                }
            } else {
                supported.push(field);
            }
        }
        form.fields = supported;
        if let Some(key) = self.focus_config_key {
            form.field = form
                .fields
                .iter()
                .position(
                    |field| matches!(field.key, ConfigKey::Config(candidate) if candidate == key),
                )
                .unwrap_or(0);
        }
        self.focus_config_key = None;
        self.config_form = Some(form);
        self.model.last_error = None;
    }

    pub fn load_device_manager(&mut self, definition: ResourceDefinition) {
        self.device_manager = Some(DeviceManager::new(definition));
        self.model.last_error = None;
    }

    pub async fn load_storage_volumes(&mut self, client: &IncusClient, pool: String) {
        match client.storage_volumes(&pool).await {
            Ok(volumes) => {
                if self.volume_manager.is_none() {
                    self.volume_manager = Some(VolumeManager::new(pool));
                }
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.volumes = volumes;
                    manager.selected = manager
                        .selected
                        .min(manager.volumes.len().saturating_sub(1));
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_storage_pool_resources(&mut self, client: &IncusClient, pool: String) {
        match client.storage_pool_resources(&pool).await {
            Ok(resources) => {
                self.storage_resource_detail = Some(
                    serde_json::to_string_pretty(&resources)
                        .unwrap_or_else(|_| resources.to_string()),
                )
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_storage_volume_state(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
    ) {
        match client.storage_volume_state(&pool, &project, &volume).await {
            Ok(state) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.detail = Some((
                        format!("Volume state · {volume}"),
                        serde_json::to_string_pretty(&state).unwrap_or_else(|_| state.to_string()),
                    ));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_storage_volume_bitmaps(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
    ) {
        match client
            .storage_volume_bitmaps(&pool, &project, &volume)
            .await
        {
            Ok(bitmaps) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|manager| manager.bitmaps.as_mut())
                {
                    form.bitmaps = bitmaps;
                    form.selected = form.selected.min(form.bitmaps.len().saturating_sub(1));
                    form.create = None;
                    form.delete_input = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn create_storage_volume_bitmap(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        spec: StorageVolumeBitmapSpec,
    ) {
        match client
            .create_storage_volume_bitmap(&pool, &project, &volume, &spec)
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.load_storage_volume_bitmaps(client, pool, project, volume)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_storage_volume_bitmap(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        bitmap: String,
    ) {
        match client
            .delete_storage_volume_bitmap(&pool, &project, &volume, &bitmap)
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.load_storage_volume_bitmaps(client, pool, project, volume)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn copy_storage_volume(
        &mut self,
        client: &IncusClient,
        source_pool: String,
        source_project: String,
        source_name: String,
        target_pool: String,
        target_project: String,
        target_name: String,
        volume_only: bool,
        refresh: bool,
        live: bool,
        refresh_exclude_older: bool,
    ) {
        match client
            .copy_storage_volume(
                &source_pool,
                &source_project,
                &source_name,
                &target_pool,
                &target_project,
                &target_name,
                volume_only,
                refresh,
                live,
                refresh_exclude_older,
            )
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.copy_form = None;
                }
                self.load_storage_volumes(client, source_pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn start_storage_volume_protocol_proxy(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        protocol: String,
        port: u16,
    ) {
        match client
            .start_storage_volume_protocol_proxy(&pool, &project, &volume, &protocol, port)
            .await
        {
            Ok(address) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.protocol_form = None;
                    manager.proxy_address = Some(format!("{protocol}://{address}"));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn read_storage_volume_file(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        path: String,
    ) {
        match client
            .read_storage_volume_file(&pool, &project, &volume, &path)
            .await
        {
            Ok(content) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|m| m.file_form.as_mut())
                {
                    form.contents = content;
                    form.browsing = false;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn list_storage_volume_directory(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        path: String,
    ) {
        match client
            .list_storage_volume_directory(&pool, &project, &volume, &path)
            .await
        {
            Ok(entries) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|m| m.file_form.as_mut())
                {
                    form.entries = entries;
                    form.selected = 0;
                    form.browsing = true;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn write_storage_volume_file(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        path: String,
        content: String,
    ) {
        match client
            .write_storage_volume_file(&pool, &project, &volume, &path, content.into_bytes())
            .await
        {
            Ok(_) => self.model.last_error = None,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_storage_volume_file(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        path: String,
    ) {
        match client
            .delete_storage_volume_file(&pool, &project, &volume, &path)
            .await
        {
            Ok(_) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|m| m.file_form.as_mut())
                {
                    form.delete_input = None;
                    form.contents.clear();
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn transfer_storage_volume_file(
        &mut self,
        client: &IncusClient,
        upload: bool,
        pool: String,
        project: String,
        volume: String,
        path: String,
        local_path: String,
    ) {
        let result = async {
            if upload {
                let bytes = tokio::fs::read(local_path).await.map_err(IncusError::Io)?;
                client
                    .write_storage_volume_file(&pool, &project, &volume, &path, bytes)
                    .await
                    .map(|_| ())
            } else {
                let content = client
                    .read_storage_volume_file(&pool, &project, &volume, &path)
                    .await?;
                tokio::fs::write(local_path, content)
                    .await
                    .map_err(IncusError::Io)
            }
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|m| m.file_form.as_mut())
                {
                    form.transfer = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_storage_volume(&mut self, client: &IncusClient, spec: StorageVolumeSpec) {
        match client.create_storage_volume(&spec).await {
            Ok(_) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.create_form = None;
                }
                self.load_storage_volumes(client, spec.pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn import_storage_volume(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        name: String,
        import_type: String,
        path: String,
    ) {
        let result = async {
            let archive = tokio::fs::read(path).await.map_err(IncusError::Io)?;
            client
                .import_storage_volume(&pool, &project, &name, &import_type, archive)
                .await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.import_form = None;
                }
                self.load_storage_volumes(client, pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn update_storage_volume(
        &mut self,
        client: &IncusClient,
        spec: StorageVolumeUpdateSpec,
    ) {
        match client.update_storage_volume(&spec).await {
            Ok(_) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.edit_form = None;
                }
                self.load_storage_volumes(client, spec.pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn attach_storage_volume(
        &mut self,
        client: &IncusClient,
        spec: VolumeAttachmentSpec,
    ) {
        if spec.instance_project != spec.volume_project {
            self.model.last_error = Some(
                "custom volumes can only be attached to targets in the same project".to_owned(),
            );
            return;
        }
        let result = async {
            let definition = client
                .resource_definition(
                    spec.target_kind,
                    &spec.instance_name,
                    &spec.instance_project,
                )
                .await?;
            let mut value = definition.value.clone();
            let devices = value
                .as_object_mut()
                .expect("instance definition is an object")
                .entry("devices")
                .or_insert_with(|| serde_json::json!({}));
            let device = serde_json::json!({
                "type": "disk",
                "pool": spec.pool,
                "source": spec.volume_name,
                "path": spec.path,
            });
            devices
                .as_object_mut()
                .expect("instance devices is an object")
                .insert(spec.device_name, device);
            client.update_resource(&definition, &value).await
        }
        .await;
        match result {
            Ok(_) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.attach_form = None;
                    let pool = manager.pool.clone();
                    self.load_storage_volumes(client, pool).await;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn detach_storage_volume(
        &mut self,
        client: &IncusClient,
        spec: VolumeDetachmentSpec,
    ) {
        let result = async {
            let definition = client
                .resource_definition(
                    spec.target_kind,
                    &spec.instance_name,
                    &spec.instance_project,
                )
                .await?;
            let mut value = definition.value.clone();
            let devices = value
                .get_mut("devices")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| {
                    IncusError::InvalidResource("instance has no local devices".to_owned())
                })?;
            let device = devices.get(&spec.device_name).ok_or_else(|| {
                IncusError::InvalidResource(format!("device '{}' does not exist", spec.device_name))
            })?;
            let matches_volume = device.get("type").and_then(serde_json::Value::as_str)
                == Some("disk")
                && device.get("pool").and_then(serde_json::Value::as_str)
                    == Some(spec.pool.as_str())
                && device.get("source").and_then(serde_json::Value::as_str)
                    == Some(spec.volume_name.as_str());
            if !matches_volume {
                return Err(IncusError::InvalidResource(
                    "selected device does not attach this volume".to_owned(),
                ));
            }
            devices.remove(&spec.device_name);
            client.update_resource(&definition, &value).await
        }
        .await;
        match result {
            Ok(_) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.detach_form = None;
                    let pool = manager.pool.clone();
                    self.load_storage_volumes(client, pool).await;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_storage_volume_snapshots(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
    ) {
        match client
            .storage_volume_snapshots(&pool, &project, &volume)
            .await
        {
            Ok(snapshots) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    let selected_volume = manager
                        .volumes
                        .iter()
                        .find(|item| item.name == volume && item.project == project)
                        .cloned();
                    if let Some(selected_volume) = selected_volume {
                        if let Some(form) = manager.snapshots.as_mut() {
                            form.snapshots = snapshots;
                            form.selected =
                                form.selected.min(form.snapshots.len().saturating_sub(1));
                            form.name_input = None;
                            form.confirmation = None;
                        } else {
                            manager.snapshots = Some(VolumeSnapshotsForm {
                                volume: selected_volume,
                                snapshots,
                                selected: 0,
                                name_input: None,
                                confirmation: None,
                            });
                        }
                    }
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_storage_volume_snapshot(
        &mut self,
        client: &IncusClient,
        command: AppCommand,
    ) {
        let (pool, project, volume, result) = match command {
            AppCommand::CreateStorageVolumeSnapshot {
                pool,
                project,
                volume,
                snapshot,
            } => {
                let result = client
                    .create_storage_volume_snapshot(&pool, &project, &volume, &snapshot)
                    .await;
                (pool, project, volume, result)
            }
            AppCommand::RenameStorageVolumeSnapshot {
                pool,
                project,
                volume,
                snapshot,
                new_name,
            } => {
                let result = client
                    .rename_storage_volume_snapshot(&pool, &project, &volume, &snapshot, &new_name)
                    .await;
                (pool, project, volume, result)
            }
            AppCommand::RestoreStorageVolumeSnapshot {
                pool,
                project,
                volume,
                snapshot,
            } => {
                let result = client
                    .restore_storage_volume_snapshot(&pool, &project, &volume, &snapshot)
                    .await;
                (pool, project, volume, result)
            }
            AppCommand::DeleteStorageVolumeSnapshot {
                pool,
                project,
                volume,
                snapshot,
            } => {
                let result = client
                    .delete_storage_volume_snapshot(&pool, &project, &volume, &snapshot)
                    .await;
                (pool, project, volume, result)
            }
            _ => return,
        };
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.load_storage_volume_snapshots(client, pool, project, volume)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_storage_volume_backups(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
    ) {
        match client
            .storage_volume_backups(&pool, &project, &volume)
            .await
        {
            Ok(backups) => {
                if let Some(manager) = self.volume_manager.as_mut()
                    && let Some(selected_volume) = manager
                        .volumes
                        .iter()
                        .find(|item| item.name == volume && item.project == project)
                        .cloned()
                {
                    manager.backups = Some(VolumeBackupsForm {
                        volume: selected_volume,
                        backups,
                        selected: 0,
                        name_input: None,
                        create_options: (false, false),
                        delete_input: None,
                        export_path: None,
                    });
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_storage_volume_backup(
        &mut self,
        client: &IncusClient,
        command: AppCommand,
    ) {
        let (pool, project, volume, result) = match command {
            AppCommand::CreateStorageVolumeBackup {
                pool,
                project,
                volume,
                name,
                volume_only,
                optimized_storage,
            } => {
                let result = client
                    .create_storage_volume_backup(
                        &pool,
                        &project,
                        &volume,
                        &name,
                        volume_only,
                        optimized_storage,
                    )
                    .await;
                (pool, project, volume, result)
            }
            AppCommand::RenameStorageVolumeBackup {
                pool,
                project,
                volume,
                backup,
                new_name,
            } => {
                let result = client
                    .rename_storage_volume_backup(&pool, &project, &volume, &backup, &new_name)
                    .await;
                (pool, project, volume, result)
            }
            AppCommand::DeleteStorageVolumeBackup {
                pool,
                project,
                volume,
                backup,
            } => {
                let result = client
                    .delete_storage_volume_backup(&pool, &project, &volume, &backup)
                    .await;
                (pool, project, volume, result)
            }
            _ => return,
        };
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.load_storage_volume_backups(client, pool, project, volume)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn export_storage_volume_backup(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        volume: String,
        backup: String,
        path: String,
    ) {
        let result = async {
            let archive = client
                .export_storage_volume_backup(&pool, &project, &volume, &backup)
                .await?;
            tokio::fs::write(&path, archive)
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self
                    .volume_manager
                    .as_mut()
                    .and_then(|m| m.backups.as_mut())
                {
                    form.export_path = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_storage_volume(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        name: String,
    ) {
        match client.delete_storage_volume(&pool, &project, &name).await {
            Ok(_) => {
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.delete_input = None;
                }
                self.load_storage_volumes(client, pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn rebuild_storage_volume(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        name: String,
    ) {
        match client.rebuild_storage_volume(&pool, &project, &name).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.volume_manager.as_mut() {
                    manager.rebuild_input = None;
                }
                self.load_storage_volumes(client, pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_storage_buckets(&mut self, client: &IncusClient, pool: String) {
        match client.storage_buckets(&pool).await {
            Ok(buckets) => {
                if self.bucket_manager.is_none() {
                    self.bucket_manager = Some(BucketManager::new(pool));
                }
                if let Some(manager) = self.bucket_manager.as_mut() {
                    manager.buckets = buckets;
                    manager.selected = manager
                        .selected
                        .min(manager.buckets.len().saturating_sub(1));
                    manager.editor = None;
                    manager.delete_input = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_storage_bucket(&mut self, client: &IncusClient, command: AppCommand) {
        let (pool, result) = match command {
            AppCommand::CreateStorageBucket {
                pool,
                project,
                name,
                description,
                size,
            } => {
                let result = client
                    .create_storage_bucket(&pool, &project, &name, &description, &size)
                    .await;
                (pool, result)
            }
            AppCommand::UpdateStorageBucket {
                pool,
                project,
                name,
                description,
                size,
                config,
            } => {
                let result = client
                    .update_storage_bucket(&pool, &project, &name, &description, config, &size)
                    .await;
                (pool, result)
            }
            AppCommand::DeleteStorageBucket {
                pool,
                project,
                name,
            } => {
                let result = client.delete_storage_bucket(&pool, &project, &name).await;
                (pool, result)
            }
            _ => return,
        };
        match result {
            Ok(_) => self.load_storage_buckets(client, pool).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_storage_bucket_keys(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        bucket: String,
    ) {
        match client.storage_bucket_keys(&pool, &project, &bucket).await {
            Ok(keys) => {
                if let Some(manager) = self.bucket_manager.as_mut()
                    && let Some(bucket_value) = manager
                        .buckets
                        .iter()
                        .find(|item| item.name == bucket && item.project == project)
                        .cloned()
                {
                    manager.keys = Some(BucketKeysForm {
                        bucket: bucket_value,
                        keys,
                        selected: 0,
                        editor: None,
                        delete_input: None,
                        credentials: None,
                    });
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_storage_bucket_key(&mut self, client: &IncusClient, command: AppCommand) {
        match command {
            AppCommand::CreateStorageBucketKey {
                pool,
                project,
                bucket,
                name,
                description,
                role,
            } => {
                match client
                    .create_storage_bucket_key(&pool, &project, &bucket, &name, &description, &role)
                    .await
                {
                    Ok(credentials) => {
                        self.load_storage_bucket_keys(client, pool, project, bucket)
                            .await;
                        if let Some(keys) =
                            self.bucket_manager.as_mut().and_then(|m| m.keys.as_mut())
                        {
                            keys.credentials = Some(credentials);
                        }
                    }
                    Err(error) => self.model.last_error = Some(error.to_string()),
                }
            }
            AppCommand::DeleteStorageBucketKey {
                pool,
                project,
                bucket,
                name,
            } => {
                match client
                    .delete_storage_bucket_key(&pool, &project, &bucket, &name)
                    .await
                {
                    Ok(_) => {
                        self.load_storage_bucket_keys(client, pool, project, bucket)
                            .await
                    }
                    Err(error) => self.model.last_error = Some(error.to_string()),
                }
            }
            AppCommand::UpdateStorageBucketKey {
                pool,
                project,
                bucket,
                name,
                description,
                role,
            } => {
                match client
                    .update_storage_bucket_key(&pool, &project, &bucket, &name, &description, &role)
                    .await
                {
                    Ok(_) => {
                        self.load_storage_bucket_keys(client, pool, project, bucket)
                            .await
                    }
                    Err(error) => self.model.last_error = Some(error.to_string()),
                }
            }
            _ => {}
        }
    }

    pub async fn load_storage_bucket_backups(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        bucket: String,
    ) {
        match client
            .storage_bucket_backups(&pool, &project, &bucket)
            .await
        {
            Ok(backups) => {
                if let Some(manager) = self.bucket_manager.as_mut()
                    && let Some(selected) = manager
                        .buckets
                        .iter()
                        .find(|b| b.name == bucket && b.project == project)
                        .cloned()
                {
                    manager.backups = Some(BucketBackupsForm {
                        bucket: selected,
                        backups,
                        selected: 0,
                        name_input: None,
                        compression: "gzip".to_owned(),
                        delete_input: None,
                        export_path: None,
                    });
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_storage_bucket_backup(
        &mut self,
        client: &IncusClient,
        command: AppCommand,
    ) {
        let (pool, project, bucket, result) = match command {
            AppCommand::CreateStorageBucketBackup {
                pool,
                project,
                bucket,
                name,
                compression,
            } => {
                let result = client
                    .create_storage_bucket_backup(&pool, &project, &bucket, &name, &compression)
                    .await;
                (pool, project, bucket, result)
            }
            AppCommand::RenameStorageBucketBackup {
                pool,
                project,
                bucket,
                backup,
                new_name,
            } => {
                let result = client
                    .rename_storage_bucket_backup(&pool, &project, &bucket, &backup, &new_name)
                    .await;
                (pool, project, bucket, result)
            }
            AppCommand::DeleteStorageBucketBackup {
                pool,
                project,
                bucket,
                backup,
            } => {
                let result = client
                    .delete_storage_bucket_backup(&pool, &project, &bucket, &backup)
                    .await;
                (pool, project, bucket, result)
            }
            _ => return,
        };
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.load_storage_bucket_backups(client, pool, project, bucket)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn export_storage_bucket_backup(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        bucket: String,
        backup: String,
        path: String,
    ) {
        let result = async {
            let archive = client
                .export_storage_bucket_backup(&pool, &project, &bucket, &backup)
                .await?;
            tokio::fs::write(path, archive)
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self
                    .bucket_manager
                    .as_mut()
                    .and_then(|m| m.backups.as_mut())
                {
                    form.export_path = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn import_storage_bucket(
        &mut self,
        client: &IncusClient,
        pool: String,
        project: String,
        name: String,
        path: String,
    ) {
        let result = async {
            let archive = tokio::fs::read(path).await.map_err(IncusError::Io)?;
            client
                .import_storage_bucket(&pool, &project, &name, archive)
                .await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.bucket_manager.as_mut() {
                    manager.import_form = None;
                }
                self.load_storage_buckets(client, pool).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_network_services(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
    ) {
        match tokio::try_join!(
            client.network_leases(&network, &project),
            client.network_forwards(&network, &project),
            client.network_acls(&project)
        ) {
            Ok((leases, forwards, acls)) => {
                let load_balancers = client
                    .network_load_balancers(&network, &project)
                    .await
                    .unwrap_or_default();
                let peers = client
                    .network_peers(&network, &project)
                    .await
                    .unwrap_or_default();
                if self.network_services.is_none() {
                    self.network_services = Some(NetworkServicesManager::new(network, project));
                }
                if let Some(manager) = self.network_services.as_mut() {
                    manager.leases = leases;
                    manager.forwards = forwards;
                    manager.acls = acls;
                    manager.load_balancers = load_balancers;
                    manager.peers = peers;
                    manager.selected = manager.selected.min(manager.len().saturating_sub(1));
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_network_state(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
    ) {
        match client.network_state(&network, &project).await {
            Ok(state) => {
                if let Some(manager) = self.network_services.as_mut() {
                    manager.detail = Some((
                        format!("Network state · {network}"),
                        format_network_state(&state),
                    ));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_network_forward(&mut self, client: &IncusClient, spec: NetworkForwardSpec) {
        match client
            .create_network_forward(
                &spec.network,
                &spec.project,
                &spec.listen_address,
                &spec.description,
                spec.port,
            )
            .await
        {
            Ok(_) => {
                if let Some(manager) = self.network_services.as_mut() {
                    manager.form = None;
                }
                self.load_network_services(client, spec.network, spec.project)
                    .await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_network_forward(
        &mut self,
        client: &IncusClient,
        spec: NetworkForwardUpdateSpec,
    ) {
        match client.update_network_forward(&spec).await {
            Ok(_) => {
                self.load_network_services(client, spec.network, spec.project)
                    .await
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_network_forward(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        listen_address: String,
    ) {
        match client
            .delete_network_forward(&network, &project, &listen_address)
            .await
        {
            Ok(_) => self.load_network_services(client, network, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_network_acl(&mut self, client: &IncusClient, spec: NetworkAclSpec) {
        match client
            .create_network_acl(&spec.project, &spec.name, &spec.description)
            .await
        {
            Ok(_) => {
                if let Some(manager) = self.network_services.as_mut() {
                    manager.form = None;
                    let network = manager.network.clone();
                    let project = manager.project.clone();
                    self.load_network_services(client, network, project).await;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_network_acl(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        name: String,
    ) {
        match client.delete_network_acl(&project, &name).await {
            Ok(_) => self.load_network_services(client, network, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_network_acl_log(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        name: String,
    ) {
        match client.network_acl_log(&project, &name).await {
            Ok(content) => {
                if let Some(manager) = self.network_services.as_mut() {
                    manager.detail = Some((format!("ACL log · {name}"), content));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
        let _ = network;
    }
    pub async fn rename_network_acl(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        name: String,
        new_name: String,
    ) {
        match client.rename_network_acl(&project, &name, &new_name).await {
            Ok(_) => self.load_network_services(client, network, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_network_acl(&mut self, client: &IncusClient, acl: NetworkAclSummary) {
        match client
            .update_network_acl(
                &acl.project,
                &acl.name,
                &acl.description,
                acl.ingress,
                acl.egress,
                acl.config,
            )
            .await
        {
            Ok(_) => {
                if let Some(manager) = self.network_services.as_mut() {
                    manager.acl_rules = None;
                    let network = manager.network.clone();
                    let project = manager.project.clone();
                    self.load_network_services(client, network, project).await;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_network_load_balancer(
        &mut self,
        client: &IncusClient,
        spec: NetworkLoadBalancerSpec,
    ) {
        match client.create_network_load_balancer(&spec).await {
            Ok(_) => {
                self.load_network_services(client, spec.network, spec.project)
                    .await
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_network_load_balancer(
        &mut self,
        client: &IncusClient,
        spec: NetworkLoadBalancerUpdateSpec,
    ) {
        match client.update_network_load_balancer(&spec).await {
            Ok(_) => {
                self.load_network_services(client, spec.network, spec.project)
                    .await
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_network_load_balancer(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        listen_address: String,
    ) {
        match client
            .delete_network_load_balancer(&network, &project, &listen_address)
            .await
        {
            Ok(_) => self.load_network_services(client, network, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_network_load_balancer_state(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        address: String,
    ) {
        match client
            .network_load_balancer_state(&network, &project, &address)
            .await
        {
            Ok(state) => {
                if let Some(manager) = self.network_services.as_mut() {
                    let timestamp = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map(|value| value.as_secs())
                        .unwrap_or(0);
                    let summary = format_load_balancer_state(&state);
                    manager
                        .load_balancer_history
                        .push(LoadBalancerHealthSample {
                            timestamp,
                            address: address.clone(),
                            summary: summary.clone(),
                        });
                    if manager.load_balancer_history.len() > 100 {
                        manager
                            .load_balancer_history
                            .drain(..manager.load_balancer_history.len() - 100);
                    }
                    let history = manager
                        .load_balancer_history
                        .iter()
                        .rev()
                        .filter(|sample| sample.address == address)
                        .take(10)
                        .map(|sample| {
                            format!(
                                "{}  {}",
                                sample.timestamp,
                                sample.summary.replace('\n', " · ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    manager.detail = Some((
                        format!("Load balancer health · {address}"),
                        format!(
                            "Current backend health\n{summary}\n\nRecent samples (newest first)\n{history}"
                        ),
                    ));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn create_network_peer(&mut self, client: &IncusClient, spec: NetworkPeerSpec) {
        match client.create_network_peer(&spec).await {
            Ok(_) => {
                self.load_network_services(client, spec.network, spec.project)
                    .await
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_network_peer(&mut self, client: &IncusClient, spec: NetworkPeerUpdateSpec) {
        match client.update_network_peer(&spec).await {
            Ok(_) => {
                self.load_network_services(client, spec.network, spec.project)
                    .await
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_network_peer(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
        name: String,
    ) {
        match client.delete_network_peer(&network, &project, &name).await {
            Ok(_) => self.load_network_services(client, network, project).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_network_zones(&mut self, client: &IncusClient) {
        match client.network_zones().await {
            Ok(zones) => {
                if self.zone_manager.is_none() {
                    self.zone_manager = Some(ZoneManager::new());
                }
                if let Some(manager) = self.zone_manager.as_mut() {
                    manager.zones = zones;
                    manager.selected = manager.selected.min(manager.zones.len().saturating_sub(1));
                    manager.editor = None;
                    manager.delete_input = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_bgp_configuration(
        &mut self,
        client: &IncusClient,
        network: String,
        project: String,
    ) {
        match client
            .resource_definition(ResourceKind::Network, &network, &project)
            .await
        {
            Ok(definition) => self.bgp_form = Some(BgpForm::new(definition)),
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_network_address_sets(&mut self, client: &IncusClient) {
        match client.network_address_sets().await {
            Ok(sets) => {
                if self.address_set_manager.is_none() {
                    self.address_set_manager = Some(AddressSetManager::new());
                }
                if let Some(manager) = self.address_set_manager.as_mut() {
                    manager.sets = sets;
                    manager.selected = manager.selected.min(manager.sets.len().saturating_sub(1));
                    manager.editor = None;
                    manager.delete_input = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn mutate_network_address_set(&mut self, client: &IncusClient, command: AppCommand) {
        let result = match command {
            AppCommand::CreateNetworkAddressSet {
                project,
                name,
                description,
                addresses,
            } => {
                client
                    .create_network_address_set(&project, &name, &description, addresses)
                    .await
            }
            AppCommand::UpdateNetworkAddressSet {
                project,
                name,
                description,
                addresses,
                config,
            } => {
                client
                    .update_network_address_set(&project, &name, &description, addresses, config)
                    .await
            }
            AppCommand::DeleteNetworkAddressSet { project, name } => {
                client.delete_network_address_set(&project, &name).await
            }
            AppCommand::RenameNetworkAddressSet {
                project,
                name,
                new_name,
            } => {
                client
                    .rename_network_address_set(&project, &name, &new_name)
                    .await
            }
            _ => return,
        };
        match result {
            Ok(_) => self.load_network_address_sets(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_network_integrations(&mut self, client: &IncusClient) {
        if self.integration_manager.is_none() {
            self.integration_manager = Some(IntegrationManager::new());
        }
        match client.network_integrations().await {
            Ok(values) => {
                if let Some(manager) = self.integration_manager.as_mut() {
                    manager.integrations = values;
                    manager.selected = manager
                        .selected
                        .min(manager.integrations.len().saturating_sub(1));
                    manager.editor = None;
                    manager.delete_input = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn mutate_network_integration(&mut self, client: &IncusClient, command: AppCommand) {
        let result = match command {
            AppCommand::CreateNetworkIntegration {
                name,
                integration_type,
                description,
                config,
            } => {
                client
                    .create_network_integration(&name, &integration_type, &description, config)
                    .await
            }
            AppCommand::UpdateNetworkIntegration {
                name,
                description,
                config,
            } => {
                client
                    .update_network_integration(&name, &description, config)
                    .await
            }
            AppCommand::DeleteNetworkIntegration { name } => {
                client.delete_network_integration(&name).await
            }
            AppCommand::RenameNetworkIntegration { name, new_name } => {
                client.rename_network_integration(&name, &new_name).await
            }
            _ => return,
        };
        match result {
            Ok(_) => self.load_network_integrations(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn mutate_network_zone(&mut self, client: &IncusClient, command: AppCommand) {
        let result = match command {
            AppCommand::CreateNetworkZone {
                project,
                name,
                description,
            } => {
                client
                    .create_network_zone(&project, &name, &description)
                    .await
            }
            AppCommand::UpdateNetworkZone {
                project,
                name,
                description,
                config,
            } => {
                client
                    .update_network_zone(&project, &name, &description, config)
                    .await
            }
            AppCommand::DeleteNetworkZone { project, name } => {
                client.delete_network_zone(&project, &name).await
            }
            AppCommand::RenameNetworkZone {
                project,
                name,
                new_name,
            } => client.rename_network_zone(&project, &name, &new_name).await,
            _ => return,
        };
        match result {
            Ok(_) => self.load_network_zones(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn load_network_zone_records(
        &mut self,
        client: &IncusClient,
        project: String,
        zone: String,
    ) {
        match client.network_zone_records(&project, &zone).await {
            Ok(records) => {
                if let Some(manager) = self.zone_manager.as_mut()
                    && let Some(zone_value) = manager
                        .zones
                        .iter()
                        .find(|z| z.name == zone && z.project == project)
                        .cloned()
                {
                    manager.records = Some(ZoneRecordsForm {
                        zone: zone_value,
                        records,
                        selected: 0,
                        editor: None,
                        delete_input: None,
                    });
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn mutate_network_zone_record(&mut self, client: &IncusClient, command: AppCommand) {
        let (project, zone, result) = match command {
            AppCommand::CreateNetworkZoneRecord {
                project,
                zone,
                name,
                description,
                entry,
            } => {
                let result = client
                    .create_network_zone_record(&project, &zone, &name, &description, entry)
                    .await;
                (project, zone, result)
            }
            AppCommand::UpdateNetworkZoneRecord {
                project,
                zone,
                name,
                description,
                entries,
            } => {
                let result = client
                    .update_network_zone_record(&project, &zone, &name, &description, entries)
                    .await;
                (project, zone, result)
            }
            AppCommand::DeleteNetworkZoneRecord {
                project,
                zone,
                name,
            } => {
                let result = client
                    .delete_network_zone_record(&project, &zone, &name)
                    .await;
                (project, zone, result)
            }
            _ => return,
        };
        match result {
            Ok(_) => self.load_network_zone_records(client, project, zone).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Saves a definition through Incus's ETag-protected PUT endpoint.
    pub async fn update_resource(
        &mut self,
        client: &IncusClient,
        definition: ResourceDefinition,
        value: serde_json::Value,
    ) {
        match client.update_resource(&definition, &value).await {
            Ok(_) => {
                self.config_form = None;
                self.device_manager = None;
                self.bgp_form = None;
                self.logging_target_manager = None;
                self.pool_safety_manager = None;
                self.qmp_manager = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_resource(
        &mut self,
        client: &IncusClient,
        kind: ResourceKind,
        project: String,
        value: serde_json::Value,
    ) {
        match client.create_resource(kind, &project, &value).await {
            Ok(_) => {
                self.resource_create_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_resource(
        &mut self,
        client: &IncusClient,
        kind: ResourceKind,
        name: String,
        project: String,
    ) {
        match client.delete_resource(kind, &name, &project).await {
            Ok(_) => {
                self.resource_delete_confirmation = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn create_image_alias(
        &mut self,
        client: &IncusClient,
        alias: String,
        fingerprint: String,
        project: String,
        description: String,
    ) {
        match client
            .create_image_alias(&alias, &fingerprint, &project, &description)
            .await
        {
            Ok(_) => {
                self.image_alias_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn update_image_alias(
        &mut self,
        client: &IncusClient,
        alias: String,
        new_name: String,
        fingerprint: String,
        project: String,
        description: String,
    ) {
        match client
            .update_image_alias(&alias, &project, &new_name, &fingerprint, &description)
            .await
        {
            Ok(_) => {
                self.image_alias_manager = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn delete_image_alias(
        &mut self,
        client: &IncusClient,
        alias: String,
        project: String,
    ) {
        match client.delete_image_alias(&alias, &project).await {
            Ok(_) => {
                self.image_alias_manager = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn update_image(&mut self, client: &IncusClient, spec: ImageUpdateSpec) {
        match client.update_image(&spec).await {
            Ok(_) => {
                self.image_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn import_image(
        &mut self,
        client: &IncusClient,
        project: String,
        path: String,
        public: bool,
        aliases: Vec<String>,
    ) {
        let result = async {
            let archive = tokio::fs::read(&path).await.map_err(IncusError::Io)?;
            let filename = std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image.tar.gz");
            client
                .import_image(&project, filename, public, &aliases, archive)
                .await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.image_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn export_image(
        &mut self,
        client: &IncusClient,
        fingerprint: String,
        project: String,
        path: String,
    ) {
        let result = async {
            let archive = client.export_image(&fingerprint, &project).await?;
            tokio::fs::write(&path, archive)
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                self.image_form = None;
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn push_image(
        &mut self,
        client: &IncusClient,
        fingerprint: String,
        project: String,
        target: String,
        target_project: String,
        secret: String,
        certificate_path: String,
        aliases: Vec<String>,
        profiles: Vec<String>,
    ) {
        let result = async {
            let certificate = if certificate_path.is_empty() {
                String::new()
            } else {
                tokio::fs::read_to_string(certificate_path)
                    .await
                    .map_err(IncusError::Io)?
            };
            client
                .push_image(
                    &fingerprint,
                    &project,
                    &target,
                    &target_project,
                    &secret,
                    &certificate,
                    aliases,
                    profiles,
                )
                .await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.image_form = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn refresh_image(
        &mut self,
        client: &IncusClient,
        fingerprint: String,
        project: String,
    ) {
        match client.refresh_image(&fingerprint, &project).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn copy_image_project(
        &mut self,
        client: &IncusClient,
        fingerprint: String,
        source_project: String,
        target_project: String,
        public: bool,
    ) {
        let result = async {
            let archive = client.export_image(&fingerprint, &source_project).await?;
            client
                .import_image(
                    &target_project,
                    &format!("{fingerprint}.tar"),
                    public,
                    &[],
                    archive,
                )
                .await
        }
        .await;
        match result {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.image_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn publish_instance_image(&mut self, client: &IncusClient, spec: ImagePublishSpec) {
        match client.publish_instance_image(&spec).await {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                self.image_form = None;
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn read_file(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
    ) {
        match client.read_file(&name, &project, &path).await {
            Ok(contents) => {
                if let Some(form) = self.file_form.as_mut() {
                    form.contents = contents;
                    form.loaded = true;
                    form.focus = FileFocus::Contents;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn list_directory(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
    ) {
        match client.list_directory(&name, &project, &path).await {
            Ok(entries) => {
                if let Some(form) = self.file_form.as_mut() {
                    form.directory_entries = entries;
                    form.directory_selected = 0;
                    form.browsing_directory = true;
                    form.focus = FileFocus::Path;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn write_file(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
        contents: String,
    ) {
        match client.write_file(&name, &project, &path, contents).await {
            Ok(()) => self.model.last_error = None,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_file(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
    ) {
        match client.delete_file(&name, &project, &path).await {
            Ok(()) => {
                self.file_form = None;
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn pull_file(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
        local_path: String,
    ) {
        let result = async {
            let contents = client.pull_file(&name, &project, &path).await?;
            tokio::fs::write(&local_path, contents)
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self.file_form.as_mut() {
                    form.transfer = None;
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn push_file(
        &mut self,
        client: &IncusClient,
        name: String,
        project: String,
        path: String,
        local_path: String,
    ) {
        let result = async {
            let contents = tokio::fs::read(&local_path).await.map_err(IncusError::Io)?;
            client.push_file(&name, &project, &path, contents).await
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(form) = self.file_form.as_mut() {
                    form.transfer = None;
                }
                self.read_file(client, name, project, path).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn cancel_operation(&mut self, client: &IncusClient, id: String) {
        match client.cancel_operation(&id).await {
            Ok(()) => self.refresh(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn acknowledge_warning(&mut self, client: &IncusClient, uuid: String) {
        match client.acknowledge_warning(&uuid).await {
            Ok(_) => self.refresh(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_warning(&mut self, client: &IncusClient, uuid: String) {
        match client.delete_warning(&uuid).await {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.delete_input = None;
                    manager.selected = 0;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_certificate(&mut self, client: &IncusClient, fingerprint: String) {
        match client.delete_certificate(&fingerprint).await {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.delete_input = None;
                    manager.selected = 0;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn generate_certificate_token(
        &mut self,
        client: &IncusClient,
        name: String,
        restricted: bool,
        projects: Vec<String>,
    ) {
        match client
            .generate_certificate_token(&name, restricted, projects)
            .await
        {
            Ok(operation) => {
                let token = operation
                    .metadata
                    .get("secret")
                    .or_else(|| operation.metadata.get("token"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&operation.id)
                    .to_owned();
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.certificate_form = None;
                    manager.trust_token = Some(token);
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn add_certificate_from_file(
        &mut self,
        client: &IncusClient,
        name: String,
        restricted: bool,
        projects: Vec<String>,
        path: String,
    ) {
        let result = async {
            let certificate = tokio::fs::read_to_string(path)
                .await
                .map_err(IncusError::Io)?;
            client
                .add_certificate(&name, restricted, projects, certificate)
                .await
        }
        .await;
        match result {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.certificate_form = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn update_certificate(
        &mut self,
        client: &IncusClient,
        fingerprint: String,
        name: String,
        restricted: bool,
        projects: Vec<String>,
    ) {
        match client
            .update_certificate(&fingerprint, &name, restricted, projects)
            .await
        {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.certificate_edit = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub(crate) async fn update_cluster_member(
        &mut self,
        client: &IncusClient,
        update: ClusterMemberUpdate,
    ) {
        let ClusterMemberUpdate {
            name,
            description,
            roles,
            groups,
            failure_domain,
            config,
        } = update;
        match client
            .update_cluster_member(&name, &description, roles, groups, &failure_domain, config)
            .await
        {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_form = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn rename_cluster_member(
        &mut self,
        client: &IncusClient,
        name: String,
        new_name: String,
    ) {
        match client.rename_cluster_member(&name, &new_name).await {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_rename_input = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_cluster_member_state(&mut self, client: &IncusClient, name: String) {
        match client.cluster_member_state(&name).await {
            Ok(state) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_detail = Some((
                        "Cluster member state".into(),
                        serde_json::to_string_pretty(&state).unwrap_or_else(|_| state.to_string()),
                    ));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_cluster_configuration(&mut self, client: &IncusClient) {
        match client.cluster_configuration().await {
            Ok(value) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_config = Some(ClusterConfigForm {
                        fields: [
                            value
                                .get("server_name")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .into(),
                            value
                                .get("server_address")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .into(),
                            String::new(),
                            String::new(),
                            String::new(),
                            String::new(),
                        ],
                        enabled: value
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                        field: 0,
                    });
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn read_metadata_configuration(&mut self, client: &IncusClient) {
        match client.metadata_configuration().await {
            Ok(content) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_detail =
                        Some(("Metadata configuration schema".into(), content));
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn configure_cluster(&mut self, client: &IncusClient, value: serde_json::Value) {
        match client.configure_cluster(&value).await {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_config = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }
    pub async fn update_cluster_certificate(
        &mut self,
        client: &IncusClient,
        certificate_path: String,
        key_path: String,
    ) {
        let result = async {
            let certificate = tokio::fs::read_to_string(certificate_path)
                .await
                .map_err(IncusError::Io)?;
            let key = tokio::fs::read_to_string(key_path)
                .await
                .map_err(IncusError::Io)?;
            client.update_cluster_certificate(&certificate, &key).await
        }
        .await;
        match result {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_certificate = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn change_cluster_member_state(
        &mut self,
        client: &IncusClient,
        name: String,
        action: String,
    ) {
        match client
            .change_cluster_member_state(&name, &action, "auto")
            .await
        {
            Ok(operation) => {
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_action = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn delete_cluster_member(&mut self, client: &IncusClient, name: String) {
        match client.delete_cluster_member(&name).await {
            Ok(_) => {
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_action = None;
                }
                self.refresh(client).await;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn generate_cluster_join_token(&mut self, client: &IncusClient, name: String) {
        match client.generate_cluster_join_token(&name).await {
            Ok(operation) => {
                let token = operation
                    .metadata
                    .get("token")
                    .or_else(|| operation.metadata.get("secret"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&operation.id)
                    .to_owned();
                self.model.last_operation = Some(operation);
                if let Some(manager) = self.settings_manager.as_mut() {
                    manager.cluster_join_name = None;
                    manager.trust_token = Some(token);
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_cluster_groups(&mut self, client: &IncusClient) {
        if self.cluster_group_manager.is_none() {
            self.cluster_group_manager = Some(ClusterGroupManager::new());
        }
        match client.cluster_groups().await {
            Ok(groups) => {
                if let Some(manager) = self.cluster_group_manager.as_mut() {
                    manager.groups = groups;
                    manager.selected = manager.selected.min(manager.groups.len().saturating_sub(1));
                    manager.editor = None;
                    manager.delete_input = None;
                }
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn mutate_cluster_group(&mut self, client: &IncusClient, command: AppCommand) {
        let result = match command {
            AppCommand::CreateClusterGroup {
                name,
                description,
                members,
            } => {
                client
                    .create_cluster_group(&name, &description, members)
                    .await
            }
            AppCommand::UpdateClusterGroup {
                name,
                description,
                members,
            } => {
                client
                    .update_cluster_group(&name, &description, members)
                    .await
            }
            AppCommand::DeleteClusterGroup { name } => client.delete_cluster_group(&name).await,
            AppCommand::RenameClusterGroup { name, new_name } => {
                client.rename_cluster_group(&name, &new_name).await
            }
            _ => return,
        };
        match result {
            Ok(_) => self.load_cluster_groups(client).await,
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_project_usage(&mut self, client: &IncusClient, project: String) {
        match client.project_state(&project).await {
            Ok(state) => {
                if let Some(form) = self.project_usage.as_mut() {
                    form.state = Some(state);
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn load_project_access(&mut self, client: &IncusClient, project: String) {
        match client.project_access(&project).await {
            Ok(entries) => {
                self.project_access = Some(ProjectAccessForm { project, entries });
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    /// Handles keyboard input and returns the requested side effect, if any.
    pub fn handle_key(&mut self, key: KeyEvent) -> AppCommand {
        if key.kind != KeyEventKind::Press {
            return AppCommand::None;
        }

        if self.filter_form.is_some() {
            return self.handle_filter_key(key);
        }
        if self.view_manager.is_some() {
            return self.handle_saved_view_key(key);
        }
        if self.audit_export.is_some() {
            return self.handle_audit_export_key(key);
        }
        if self.journal_viewer.is_some() {
            return self.handle_journal_key(key);
        }
        if self.fleet_manager.is_some() {
            return self.handle_fleet_key(key);
        }
        if self.placement_inspector.is_some() {
            return self.handle_placement_inspector_key(key);
        }
        if self.backup_schedule_form.is_some() {
            return self.handle_backup_schedule_key(key);
        }
        if self.openfga_manager.is_some() {
            return self.handle_openfga_key(key);
        }
        if self.image_retention_preview.is_some() {
            return self.handle_image_retention_key(key);
        }
        if self.connection_form.is_some() {
            return self.handle_connection_key(key);
        }
        if self.cluster_group_manager.is_some() {
            return self.handle_cluster_group_key(key);
        }
        if self.move_form.is_some() {
            return self.handle_move_key(key);
        }
        if self.project_usage.is_some() {
            return self.handle_project_usage_key(key);
        }
        if self.project_access.is_some() {
            return self.handle_project_access_key(key);
        }
        if self.instance_access.is_some() {
            return self.handle_instance_access_key(key);
        }
        if self.action_menu.is_some() {
            return self.handle_action_menu_key(key);
        }

        if self.confirmation.is_some() {
            return self.handle_confirmation_key(key);
        }
        if self.create_form.is_some() {
            return self.handle_create_key(key);
        }
        if self.exec_form.is_some() {
            return self.handle_exec_key(key);
        }
        if self.snapshot_form.is_some() {
            return self.handle_snapshot_key(key);
        }
        if self.backup_form.is_some() {
            return self.handle_backup_key(key);
        }
        if self.console_form.is_some() {
            return self.handle_console_key(key);
        }
        if self.detail_form.is_some() {
            return self.handle_detail_key(key);
        }
        if self.config_form.is_some() {
            return self.handle_config_form_key(key);
        }
        if self.resource_create_form.is_some() {
            return self.handle_resource_create_key(key);
        }
        if self.resource_delete_confirmation.is_some() {
            return self.handle_resource_delete_key(key);
        }
        if self.image_alias_form.is_some() {
            return self.handle_image_alias_key(key);
        }
        if self.image_alias_manager.is_some() {
            return self.handle_image_alias_manager_key(key);
        }
        if self.image_form.is_some() {
            return self.handle_image_form_key(key);
        }
        if self.artifacts_form.is_some() {
            return self.handle_artifacts_key(key);
        }
        if self.operation_detail.is_some() {
            return self.handle_operation_detail_key(key);
        }
        if self.rename_form.is_some() {
            return self.handle_rename_key(key);
        }
        if self.rebuild_form.is_some() {
            return self.handle_rebuild_key(key);
        }
        if self.log_manager.is_some() {
            return self.handle_log_manager_key(key);
        }
        if self.allocation_explorer.is_some() {
            return self.handle_allocation_explorer_key(key);
        }
        if self.metrics_explorer.is_some() {
            return self.handle_metrics_explorer_key(key);
        }
        if self.maintenance_manager.is_some() {
            return self.handle_maintenance_manager_key(key);
        }
        if self.storage_resource_detail.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.storage_resource_detail = None;
            }
            return AppCommand::None;
        }
        if self.logging_target_manager.is_some() {
            return self.handle_logging_target_key(key);
        }
        if self.scriptlet_manager.is_some() {
            return self.handle_scriptlet_key(key);
        }
        if self.preseed_manager.is_some() {
            return self.handle_preseed_key(key);
        }
        if self.pool_safety_manager.is_some() {
            return self.handle_pool_safety_key(key);
        }
        if self.qmp_manager.is_some() {
            return self.handle_qmp_key(key);
        }
        if self.bulk_manager.is_some() {
            return self.handle_bulk_key(key);
        }
        if self.settings_manager.is_some() {
            return self.handle_settings_manager_key(key);
        }
        if self.device_manager.is_some() {
            return self.handle_device_manager_key(key);
        }
        if self.bucket_manager.is_some() {
            return self.handle_bucket_manager_key(key);
        }
        if self.volume_manager.is_some() {
            return self.handle_volume_manager_key(key);
        }
        if self.bgp_form.is_some() {
            return self.handle_bgp_key(key);
        }
        if self.address_set_manager.is_some() {
            return self.handle_address_set_key(key);
        }
        if self.integration_manager.is_some() {
            return self.handle_integration_key(key);
        }
        if self.zone_manager.is_some() {
            return self.handle_zone_manager_key(key);
        }
        if self.network_services.is_some() {
            return self.handle_network_services_key(key);
        }
        if self.file_form.is_some() {
            return self.handle_file_key(key);
        }
        if self.operation_confirmation.is_some() {
            return self.handle_operation_confirmation_key(key);
        }
        if self.help_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help_visible = false;
            }
            return AppCommand::None;
        }

        match key.code {
            KeyCode::Char('\'') => {
                self.view_manager = Some(SavedViewManager {
                    views: self.saved_views.clone(),
                    selected: 0,
                    name_input: None,
                });
                return AppCommand::None;
            }
            KeyCode::Char('X') if matches!(self.page, Page::Events | Page::Operations) => {
                self.audit_export = Some(AuditExportForm {
                    path: "./virtty-audit.json".into(),
                    filter: self.filter_query.clone(),
                    include_events: true,
                    include_operations: true,
                    redact: true,
                    field: 0,
                });
                return AppCommand::None;
            }
            KeyCode::Char('y')
                if self.page == Page::Instances && !self.model.instances.is_empty() =>
            {
                self.placement_inspector = Some(PlacementInspector {
                    instance: self.model.instances[self.selected_instance].clone(),
                    members: self.model.cluster_members.clone(),
                    selected: 0,
                });
                return AppCommand::None;
            }
            KeyCode::Char('b')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.page == Page::Instances
                    && !self.model.instances.is_empty() =>
            {
                let instance = &self.model.instances[self.selected_instance];
                let schedule = self
                    .backup_schedules
                    .iter()
                    .find(|schedule| {
                        schedule.instance == instance.name && schedule.project == instance.project
                    })
                    .cloned()
                    .unwrap_or(BackupSchedule {
                        instance: instance.name.clone(),
                        project: instance.project.clone(),
                        interval_seconds: 86400,
                        name_pattern: "scheduled-{timestamp}".into(),
                        instance_only: false,
                        optimized_storage: false,
                        enabled: false,
                        last_run: None,
                        last_result: String::new(),
                    });
                self.backup_schedule_form = Some(BackupScheduleForm { schedule, field: 0 });
                return AppCommand::None;
            }
            KeyCode::Char('/') => {
                self.filter_form = Some(self.filter_query.clone());
            }
            KeyCode::Char('x') => self.open_action_menu(),
            KeyCode::Char('L') => {
                self.filter_query.clear();
                self.clamp_selection();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char(digit @ '0'..='9') => {
                let index = if digit == '0' {
                    Page::ALL.len() - 1
                } else {
                    (digit as u8 - b'1') as usize
                };
                if let Some(page) = Page::ALL.get(index) {
                    self.page = *page;
                }
            }
            KeyCode::Tab => self.page = self.page.next_view(),
            KeyCode::BackTab => self.page = self.page.previous_view(),
            KeyCode::Right | KeyCode::Char('l') => self.page = self.page.next_group(),
            KeyCode::Left | KeyCode::Char('h') => self.page = self.page.previous_group(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Char('c') if self.page == Page::Operations => {
                self.open_operation_confirmation()
            }
            KeyCode::Enter if self.page == Page::Operations => self.open_operation_detail(),
            KeyCode::Char('s') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Start)
            }
            KeyCode::Char('X') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Stop)
            }
            KeyCode::Char('R') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Restart)
            }
            KeyCode::Char('f') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Freeze)
            }
            KeyCode::Char('u') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Unfreeze)
            }
            KeyCode::Char('d') if self.page == Page::Instances => {
                self.open_confirmation(InstanceAction::Delete)
            }
            KeyCode::Char('A')
                if self.page == Page::Instances && !self.model.instances.is_empty() =>
            {
                let instance = &self.model.instances[self.selected_instance];
                let name = instance.name.clone();
                let project = instance.project.clone();
                self.instance_access = Some(InstanceAccessForm {
                    name: name.clone(),
                    project: project.clone(),
                    entries: Vec::new(),
                });
                return AppCommand::ReadInstanceAccess { name, project };
            }
            KeyCode::Char('n' | 'c') if self.page == Page::Instances => {
                let mut form = CreateForm::new();
                form.spec.project = self.active_project.clone();
                self.create_form = Some(form);
                self.model.last_error = None;
            }
            KeyCode::Char('n') if self.page == Page::Networks => {
                self.open_resource_create(ResourceKind::Network);
            }
            KeyCode::Char('n') if self.page == Page::Storage => {
                self.open_resource_create(ResourceKind::StoragePool);
            }
            KeyCode::Char('n') if self.page == Page::Projects => {
                self.open_resource_create(ResourceKind::Project);
            }
            KeyCode::Char('N') if self.page == Page::Projects => {
                self.open_resource_create(ResourceKind::Profile);
            }
            KeyCode::Char('d') if self.page == Page::Networks => {
                self.open_resource_delete(ResourceKind::Network);
            }
            KeyCode::Char('d') if self.page == Page::Storage => {
                self.open_resource_delete(ResourceKind::StoragePool);
            }
            KeyCode::Char('d') if self.page == Page::Projects => {
                self.open_resource_delete(ResourceKind::Project);
            }
            KeyCode::Char('D') if self.page == Page::Projects => {
                self.open_resource_delete(ResourceKind::Profile);
            }
            KeyCode::Char('d') if self.page == Page::Images => {
                self.open_resource_delete(ResourceKind::Image);
            }
            KeyCode::Char('a') if self.page == Page::Images => self.open_image_alias_form(),
            KeyCode::Char('A') if self.page == Page::Images => {
                if let Some(image) = self.model.images.get(self.selected_image) {
                    self.image_alias_manager = Some(ImageAliasManager::new(image));
                }
            }
            KeyCode::Char('e') if self.page == Page::Images => self.open_image_edit_form(),
            KeyCode::Char('O') if self.page == Page::Images => {
                self.open_image_edit_form();
                if let Some(ImageForm::Edit { field, .. }) = self.image_form.as_mut() {
                    *field = 10;
                }
            }
            KeyCode::Char('i') if self.page == Page::Images => {
                self.image_form = Some(ImageForm::Import {
                    project: "default".to_owned(),
                    path: "./image.tar.gz".to_owned(),
                    aliases: String::new(),
                    public: false,
                    field: 0,
                });
            }
            KeyCode::Char('o') if self.page == Page::Images => self.open_image_export_form(),
            KeyCode::Char('S') if self.page == Page::Images => self.open_image_push_form(),
            KeyCode::Char('F') if self.page == Page::Images => {
                return self.refresh_selected_image();
            }
            KeyCode::Char('C') if self.page == Page::Images => self.open_image_copy_form(),
            KeyCode::Char('P') if self.page == Page::Instances => self.open_image_publish_form(),
            KeyCode::Char('B') if self.page == Page::Instances => {
                self.bulk_manager = Some(BulkManager::new(&self.model.instances));
            }
            KeyCode::Enter if self.page == Page::Images => self.launch_selected_image(),
            KeyCode::Char('e') if self.page == Page::Instances => self.open_exec_form(),
            KeyCode::Char('I') if self.page == Page::Instances => {
                return self.open_interactive_exec();
            }
            KeyCode::Char('V') if self.page == Page::Instances => {
                return self
                    .open_interactive_console(key.modifiers.contains(KeyModifiers::CONTROL));
            }
            KeyCode::Char('C') if self.page == Page::Instances => {
                return self.open_vga_console(key.modifiers.contains(KeyModifiers::CONTROL));
            }
            KeyCode::Char('E') if self.page == Page::Instances => {
                self.focus_config_key = None;
                return self.open_instance_configuration_editor();
            }
            KeyCode::Char('O') if self.page == Page::Instances => {
                self.focus_config_key = Some("snapshots.schedule");
                return self.open_instance_configuration_editor();
            }
            KeyCode::Char('E') if self.page == Page::Networks => {
                return self.open_network_configuration_editor();
            }
            KeyCode::Char('X') if self.page == Page::Networks => {
                return self.open_network_services();
            }
            KeyCode::Char('z') if self.page == Page::Networks => {
                self.zone_manager = Some(ZoneManager::new());
                return AppCommand::ReadNetworkZones;
            }
            KeyCode::Char('B') if self.page == Page::Networks => {
                if let Some(network) = self.model.networks.get(self.selected_network) {
                    return AppCommand::ReadBgpConfiguration {
                        network: network.name.clone(),
                        project: network.project.clone(),
                    };
                }
            }
            KeyCode::Char('A') if self.page == Page::Networks => {
                self.address_set_manager = Some(AddressSetManager::new());
                return AppCommand::ReadNetworkAddressSets;
            }
            KeyCode::Char('N') if self.page == Page::Networks => {
                self.integration_manager = Some(IntegrationManager::new());
                return AppCommand::ReadNetworkIntegrations;
            }
            KeyCode::Char('Y') if self.page == Page::Networks => {
                if self.require_extension("network_allocations", "network allocation inspection") {
                    self.allocation_explorer = Some(NetworkAllocationExplorer::default());
                }
            }
            KeyCode::Char('E') if self.page == Page::Storage => {
                return self.open_storage_pool_configuration_editor();
            }
            KeyCode::Char('R') if self.page == Page::Storage => {
                let Some(pool) = self.model.storage_pools.get(self.selected_storage_pool) else {
                    return AppCommand::None;
                };
                return AppCommand::ReadStoragePoolResources {
                    pool: pool.name.clone(),
                };
            }
            KeyCode::Char('G') if self.page == Page::Storage => {
                let Some(pool) = self.model.storage_pools.get(self.selected_storage_pool) else {
                    return AppCommand::None;
                };
                return AppCommand::ReadPoolSafety {
                    pool: pool.name.clone(),
                };
            }
            KeyCode::Char('v') if self.page == Page::Storage => {
                return self.open_volume_manager();
            }
            KeyCode::Char('b') if self.page == Page::Storage => {
                return self.open_bucket_manager();
            }
            KeyCode::Char('E') if self.page == Page::Settings => {
                self.focus_config_key = None;
                return self.open_server_configuration_editor();
            }
            KeyCode::Char('I') if self.page == Page::Settings => {
                self.focus_config_key = Some("images.auto_update_interval");
                return self.open_server_configuration_editor();
            }
            KeyCode::Char('A') if self.page == Page::Settings => {
                self.focus_config_key = Some("oidc.issuer");
                return self.open_server_configuration_editor();
            }
            KeyCode::Char('J') if self.page == Page::Settings => {
                self.journal_viewer = Some(JournalViewer::default());
            }
            KeyCode::Char('F') if self.page == Page::Settings => {
                self.fleet_manager = Some(FleetManager {
                    loading: true,
                    ..Default::default()
                });
                return AppCommand::ReadFleetHealth(self.saved_remotes.clone());
            }
            KeyCode::Char('G') if self.page == Page::Settings => return AppCommand::ReadOpenFga,
            KeyCode::Char('V') if self.page == Page::Images => {
                return AppCommand::ReadImageRetention;
            }
            KeyCode::Char('s') if self.page == Page::Settings => self.open_settings_manager(),
            KeyCode::Char('c') if self.page == Page::Settings => {
                self.connection_form = Some(ConnectionForm::new(self.saved_remotes.clone()));
            }
            KeyCode::Char('K') if self.page == Page::Settings => {
                self.cluster_group_manager = Some(ClusterGroupManager::new());
                return AppCommand::ReadClusterGroups;
            }
            KeyCode::Char('P') if self.page == Page::Settings => {
                self.preseed_manager = Some(PreseedManager::new());
            }
            KeyCode::Char('E') if self.page == Page::Projects => {
                return self.open_project_configuration_editor();
            }
            KeyCode::Enter if self.page == Page::Projects && !self.model.projects.is_empty() => {
                self.active_project = self.model.projects[self.selected_project].name.clone();
                self.model.last_error = None;
            }
            KeyCode::Char('P') if self.page == Page::Projects => {
                return self.open_profile_configuration_editor();
            }
            KeyCode::Char('U')
                if self.page == Page::Projects && !self.model.projects.is_empty() =>
            {
                let project = self.model.projects[self.selected_project].name.clone();
                self.project_usage = Some(ProjectUsageForm {
                    project: project.clone(),
                    state: None,
                });
                return AppCommand::ReadProjectUsage { project };
            }
            KeyCode::Char('A')
                if self.page == Page::Projects && !self.model.projects.is_empty() =>
            {
                let project = self.model.projects[self.selected_project].name.clone();
                self.project_access = Some(ProjectAccessForm {
                    project: project.clone(),
                    entries: Vec::new(),
                });
                return AppCommand::ReadProjectAccess { project };
            }
            KeyCode::Char('G') if self.page == Page::Projects => {
                return self.open_profile_device_manager();
            }
            KeyCode::Char('Y') if self.page == Page::Projects => {
                let Some(profile) = self.model.profiles.get(self.selected_profile) else {
                    return AppCommand::None;
                };
                let name = profile.name.clone();
                let project = profile.project.clone();
                if !self.require_extension("qemu_scriptlet", "profile QEMU scriptlet studio") {
                    return AppCommand::None;
                }
                return AppCommand::ReadQemuScriptlet {
                    kind: ResourceKind::Profile,
                    name,
                    project,
                };
            }
            KeyCode::Char(']')
                if self.page == Page::Projects && !self.model.profiles.is_empty() =>
            {
                self.selected_profile = (self.selected_profile + 1) % self.model.profiles.len();
            }
            KeyCode::Char('[')
                if self.page == Page::Projects && !self.model.profiles.is_empty() =>
            {
                self.selected_profile = (self.selected_profile + self.model.profiles.len() - 1)
                    % self.model.profiles.len();
            }
            KeyCode::Char('p') if self.page == Page::Instances => self.open_snapshot_form(),
            KeyCode::Char('b') if self.page == Page::Instances => self.open_backup_form(),
            KeyCode::Char('m') if self.page == Page::Instances => {
                return self.open_artifacts_form();
            }
            KeyCode::Char('M') if self.page == Page::Instances => self.open_rename_form(),
            KeyCode::Char('W') if self.page == Page::Instances => self.open_rebuild_form(),
            KeyCode::Char('J') if self.page == Page::Instances => return self.open_log_manager(),
            KeyCode::Char('H') if self.page == Page::Instances => self.open_metrics_explorer(),
            KeyCode::Char('U') if self.page == Page::Instances => {
                return self.open_maintenance_manager();
            }
            KeyCode::Char('Y') if self.page == Page::Instances => {
                let Some(instance) = self.model.instances.get(self.selected_instance) else {
                    return AppCommand::None;
                };
                let instance_type = instance.instance_type.clone();
                let name = instance.name.clone();
                let project = instance.project.clone();
                if instance_type != "virtual-machine" {
                    self.model.last_error =
                        Some("QEMU scriptlets are only available for virtual machines".into());
                    return AppCommand::None;
                }
                if !self.require_extension("qemu_scriptlet", "QEMU scriptlet studio") {
                    return AppCommand::None;
                }
                return AppCommand::ReadQemuScriptlet {
                    kind: ResourceKind::Instance,
                    name,
                    project,
                };
            }
            KeyCode::Char('Z') if self.page == Page::Instances => {
                let Some(instance) = self.model.instances.get(self.selected_instance) else {
                    return AppCommand::None;
                };
                let instance_type = instance.instance_type.clone();
                let name = instance.name.clone();
                let project = instance.project.clone();
                if instance_type != "virtual-machine" {
                    self.model.last_error =
                        Some("QMP hooks are only available for virtual machines".into());
                    return AppCommand::None;
                }
                if !self.require_extension("qemu_raw_qmp", "structured QMP hook manager") {
                    return AppCommand::None;
                }
                return AppCommand::ReadQmpManager { name, project };
            }
            KeyCode::Char('T') if self.page == Page::Instances => {
                if let Some(instance) = self.model.instances.get(self.selected_instance) {
                    self.move_form = Some(MoveForm::new(instance));
                }
            }
            KeyCode::Char('g') if self.page == Page::Instances => {
                return self.open_device_manager();
            }
            KeyCode::Char('v') if self.page == Page::Instances => return self.open_console_form(),
            KeyCode::Enter if self.page == Page::Instances => return self.open_detail_form(),
            KeyCode::Char('o') if self.page == Page::Instances => self.open_file_form(),
            KeyCode::Char('r') => return AppCommand::Refresh,
            _ => {}
        }
        AppCommand::None
    }

    fn open_confirmation(&mut self, action: InstanceAction) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            if !action_is_allowed(&instance.status, action) {
                self.model.last_error = Some(format!(
                    "{} is not available while instance '{}' is {}",
                    action.label(),
                    instance.name,
                    instance.status.to_lowercase()
                ));
                return;
            }
            self.confirmation = Some(Confirmation::new(
                action,
                instance.name.clone(),
                instance.project.clone(),
            ));
            self.model.last_error = None;
        }
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> AppCommand {
        let confirmation = self
            .confirmation
            .as_mut()
            .expect("confirmation checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.confirmation = None;
                AppCommand::None
            }
            KeyCode::Backspace => {
                confirmation.input.pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                confirmation.input.push(character);
                AppCommand::None
            }
            KeyCode::Enter if confirmation.is_confirmed() => {
                let confirmation = self
                    .confirmation
                    .take()
                    .expect("confirmation remains present until enter");
                AppCommand::ChangeInstanceState {
                    name: confirmation.instance_name,
                    project: confirmation.project,
                    action: confirmation.action,
                }
            }
            KeyCode::Enter => {
                self.model.last_error = Some(format!(
                    "confirmation must exactly match: {}",
                    confirmation.expected_phrase()
                ));
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_create_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .create_form
            .as_mut()
            .expect("create form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.create_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter => {
                if form.field == CreateForm::FIELD_NAMES.len() - 1 && key.code == KeyCode::Enter {
                    AppCommand::CreateInstance(form.spec())
                } else {
                    form.next();
                    AppCommand::None
                }
            }
            KeyCode::BackTab => {
                form.previous();
                AppCommand::None
            }
            KeyCode::Char(' ') if CreateForm::is_choice(form.field) => {
                form.toggle_choice();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.input(character);
                AppCommand::None
            }
            KeyCode::Backspace => {
                form.backspace();
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_exec_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            if instance.status != "Running" {
                self.model.last_error = Some(format!(
                    "command execution requires a running instance; '{}' is {}",
                    instance.name,
                    instance.status.to_lowercase()
                ));
                return;
            }
            self.exec_form = Some(ExecForm::new(instance));
            self.model.last_error = None;
        }
    }

    fn handle_exec_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .exec_form
            .as_mut()
            .expect("exec form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.exec_form = None;
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::ExecuteCommand {
                name: form.instance_name.clone(),
                project: form.project.clone(),
                command: form.command.clone(),
            },
            KeyCode::Backspace => {
                form.command.pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.command.push(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_snapshot_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            self.snapshot_form = Some(SnapshotForm::new(instance));
            self.model.last_error = None;
        }
    }

    fn handle_snapshot_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .snapshot_form
            .as_mut()
            .expect("snapshot form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.snapshot_form = None;
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::CreateSnapshot {
                name: form.instance_name.clone(),
                project: form.project.clone(),
                snapshot_name: form.name.clone(),
                stateful: form.stateful,
            },
            KeyCode::Backspace => {
                form.name.pop();
                AppCommand::None
            }
            KeyCode::Char(' ') => {
                form.stateful = !form.stateful;
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.name.push(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_backup_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            self.backup_form = Some(BackupForm::new(instance));
            self.model.last_error = None;
        }
    }

    fn handle_backup_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .backup_form
            .as_mut()
            .expect("backup form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.backup_form = None;
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::CreateBackup {
                name: form.instance_name.clone(),
                project: form.project.clone(),
                backup_name: form.name.clone(),
                instance_only: form.instance_only,
                optimized_storage: form.optimized_storage,
            },
            KeyCode::Backspace => {
                form.name.pop();
                AppCommand::None
            }
            KeyCode::Char('i') => {
                form.instance_only = !form.instance_only;
                AppCommand::None
            }
            KeyCode::Char('z') => {
                form.optimized_storage = !form.optimized_storage;
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.name.push(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_console_form(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        let name = instance.name.clone();
        let project = instance.project.clone();
        self.console_form = Some(ConsoleForm::new(instance));
        self.model.last_error = None;
        AppCommand::ReadConsole { name, project }
    }

    fn handle_console_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .console_form
            .as_mut()
            .expect("console form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.console_form = None;
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::ReadConsole {
                name: form.instance_name.clone(),
                project: form.project.clone(),
            },
            _ => AppCommand::None,
        }
    }

    fn open_detail_form(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        let name = instance.name.clone();
        let project = instance.project.clone();
        self.detail_form = Some(DetailForm::new(name.clone(), project.clone()));
        self.model.last_error = None;
        AppCommand::ReadInstanceDetail { name, project }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .detail_form
            .as_mut()
            .expect("detail form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.detail_form = None;
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::ReadInstanceDetail {
                name: form.name.clone(),
                project: form.project.clone(),
            },
            KeyCode::Tab | KeyCode::Char('d') => {
                form.expanded_diff = !form.expanded_diff;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_instance_configuration_editor(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::Instance,
            name: instance.name.clone(),
            project: instance.project.clone(),
        }
    }

    fn open_network_configuration_editor(&mut self) -> AppCommand {
        let Some(network) = self.model.networks.get(self.selected_network) else {
            return AppCommand::None;
        };
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::Network,
            name: network.name.clone(),
            project: network.project.clone(),
        }
    }

    fn open_storage_pool_configuration_editor(&mut self) -> AppCommand {
        let Some(pool) = self.model.storage_pools.get(self.selected_storage_pool) else {
            return AppCommand::None;
        };
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::StoragePool,
            name: pool.name.clone(),
            project: String::new(),
        }
    }

    fn open_server_configuration_editor(&mut self) -> AppCommand {
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::Server,
            name: String::new(),
            project: String::new(),
        }
    }

    fn open_project_configuration_editor(&mut self) -> AppCommand {
        let Some(project) = self.model.projects.get(self.selected_project) else {
            return AppCommand::None;
        };
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::Project,
            name: project.name.clone(),
            project: String::new(),
        }
    }

    fn open_profile_configuration_editor(&mut self) -> AppCommand {
        let Some(profile) = self.model.profiles.get(self.selected_profile) else {
            return AppCommand::None;
        };
        self.model.last_error = None;
        AppCommand::ReadResourceDefinition {
            kind: ResourceKind::Profile,
            name: profile.name.clone(),
            project: profile.project.clone(),
        }
    }

    fn handle_config_form_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .config_form
            .as_mut()
            .expect("configuration form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.config_form = None;
                AppCommand::None
            }
            KeyCode::Enter if form.field + 1 == form.fields.len() => {
                if let Some(Err(error)) = form.cpu_topology_preview() {
                    self.model.last_error = Some(error);
                    AppCommand::None
                } else {
                    AppCommand::UpdateResource {
                        definition: form.definition.clone(),
                        value: form.value(),
                    }
                }
            }
            KeyCode::Tab | KeyCode::Enter => {
                form.next();
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.previous();
                AppCommand::None
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                form.toggle();
                AppCommand::None
            }
            KeyCode::Backspace => {
                form.backspace();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.input(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_resource_create(&mut self, kind: ResourceKind) {
        let mut form = ResourceCreateForm::new(kind);
        if let Some(field) = form
            .fields
            .iter_mut()
            .find(|field| matches!(field.key, CreateKey::Project))
        {
            field.value = self.active_project.clone();
        }
        self.resource_create_form = Some(form);
        self.model.last_error = None;
    }

    fn handle_resource_create_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_source_wipe = self.supports_extension("storage_pool_source_wipe");
        let form = self
            .resource_create_form
            .as_mut()
            .expect("resource creation form checked before handling key");
        if form.wipe_confirmation.is_some() {
            let expected = form.wipe_phrase();
            let input = form
                .wipe_confirmation
                .as_mut()
                .expect("wipe confirmation remains present");
            return match key.code {
                KeyCode::Esc => {
                    form.wipe_confirmation = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    form.wipe_confirmation = None;
                    AppCommand::CreateResource {
                        kind: form.kind,
                        project: form.project(),
                        value: form.value(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.resource_create_form = None;
                AppCommand::None
            }
            KeyCode::Enter if form.field + 1 == form.fields.len() => {
                if form.name().is_empty() {
                    self.model.last_error = Some("resource name is required".to_owned());
                    return AppCommand::None;
                }
                if form.kind == ResourceKind::StoragePool && form.config("source.wipe") == "true" {
                    if !can_source_wipe {
                        self.model.last_error = Some(
                            "source wipe unavailable: server lacks storage_pool_source_wipe".into(),
                        );
                        return AppCommand::None;
                    }
                    let source = form.config("source");
                    if !source.starts_with("/dev/") {
                        self.model.last_error = Some(
                            "source wipe requires an explicit /dev/... block-device identity"
                                .into(),
                        );
                        return AppCommand::None;
                    }
                    form.wipe_confirmation = Some(String::new());
                    return AppCommand::None;
                }
                AppCommand::CreateResource {
                    kind: form.kind,
                    project: form.project(),
                    value: form.value(),
                }
            }
            KeyCode::Tab | KeyCode::Enter => {
                form.next();
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.previous();
                AppCommand::None
            }
            KeyCode::Left => {
                form.cycle(true);
                AppCommand::None
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                form.cycle(false);
                AppCommand::None
            }
            KeyCode::Backspace => {
                form.backspace();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.input(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_resource_delete(&mut self, kind: ResourceKind) {
        let target = match kind {
            ResourceKind::Network => self
                .model
                .networks
                .get(self.selected_network)
                .map(|resource| (resource.name.clone(), resource.project.clone())),
            ResourceKind::StoragePool => self
                .model
                .storage_pools
                .get(self.selected_storage_pool)
                .map(|resource| (resource.name.clone(), String::new())),
            ResourceKind::Project => self
                .model
                .projects
                .get(self.selected_project)
                .map(|resource| (resource.name.clone(), String::new())),
            ResourceKind::Profile => self
                .model
                .profiles
                .get(self.selected_profile)
                .map(|resource| (resource.name.clone(), resource.project.clone())),
            ResourceKind::Image => self
                .model
                .images
                .get(self.selected_image)
                .map(|resource| (resource.fingerprint.clone(), resource.project.clone())),
            _ => None,
        };
        if let Some((name, project)) = target {
            self.resource_delete_confirmation =
                Some(ResourceDeleteConfirmation::new(kind, name, project));
            self.model.last_error = None;
        }
    }

    fn handle_resource_delete_key(&mut self, key: KeyEvent) -> AppCommand {
        let confirmation = self
            .resource_delete_confirmation
            .as_mut()
            .expect("resource deletion checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.resource_delete_confirmation = None;
                AppCommand::None
            }
            KeyCode::Backspace => {
                confirmation.input.pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                confirmation.input.push(character);
                AppCommand::None
            }
            KeyCode::Enter if confirmation.input == confirmation.expected_phrase() => {
                let confirmation = self
                    .resource_delete_confirmation
                    .take()
                    .expect("confirmation remains present until enter");
                AppCommand::DeleteResource {
                    kind: confirmation.kind,
                    name: confirmation.name,
                    project: confirmation.project,
                }
            }
            KeyCode::Enter => {
                self.model.last_error = Some(format!(
                    "confirmation must exactly match: {}",
                    confirmation.expected_phrase()
                ));
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_image_alias_form(&mut self) {
        if let Some(image) = self.model.images.get(self.selected_image) {
            self.image_alias_form = Some(ImageAliasForm::new(image));
            self.model.last_error = None;
        }
    }

    fn open_image_edit_form(&mut self) {
        if let Some(image) = self.model.images.get(self.selected_image) {
            self.image_form = Some(ImageForm::Edit {
                spec: ImageUpdateSpec {
                    fingerprint: image.fingerprint.clone(),
                    project: image.project.clone(),
                    public: image.public,
                    auto_update: image.auto_update,
                    properties: image.properties.clone(),
                },
                fields: [
                    image
                        .properties
                        .get("description")
                        .cloned()
                        .unwrap_or_default(),
                    image.properties.get("os").cloned().unwrap_or_default(),
                    image.properties.get("release").cloned().unwrap_or_default(),
                    image.properties.get("variant").cloned().unwrap_or_default(),
                    image
                        .properties
                        .get("requirements.privileged")
                        .cloned()
                        .unwrap_or_default(),
                    image
                        .properties
                        .get("requirements.nesting")
                        .cloned()
                        .unwrap_or_default(),
                    image
                        .properties
                        .get("requirements.secureboot")
                        .cloned()
                        .unwrap_or_default(),
                    image
                        .properties
                        .get("requirements.cdrom_agent")
                        .cloned()
                        .unwrap_or_default(),
                    image
                        .properties
                        .get("requirements.cdrom_cloud_init")
                        .cloned()
                        .unwrap_or_default(),
                ],
                field: 0,
            });
            self.model.last_error = None;
        }
    }

    fn open_image_export_form(&mut self) {
        if let Some(image) = self.model.images.get(self.selected_image) {
            self.image_form = Some(ImageForm::Export {
                fingerprint: image.fingerprint.clone(),
                project: image.project.clone(),
                path: format!("./{}.tar.gz", image.display_name()),
            });
            self.model.last_error = None;
        }
    }
    fn open_image_push_form(&mut self) {
        let Some(image) = self.model.images.get(self.selected_image) else {
            return;
        };
        self.image_form = Some(ImageForm::Push {
            fingerprint: image.fingerprint.clone(),
            project: image.project.clone(),
            fields: [
                String::new(),
                "default".into(),
                String::new(),
                String::new(),
                String::new(),
                "default".into(),
            ],
            field: 0,
        });
    }
    fn refresh_selected_image(&mut self) -> AppCommand {
        if !self.require_extension("image_force_refresh", "forced image refresh") {
            return AppCommand::None;
        }
        let Some(image) = self.model.images.get(self.selected_image) else {
            return AppCommand::None;
        };
        AppCommand::RefreshImage {
            fingerprint: image.fingerprint.clone(),
            project: image.project.clone(),
        }
    }
    fn open_image_copy_form(&mut self) {
        let Some(image) = self.model.images.get(self.selected_image) else {
            return;
        };
        self.image_form = Some(ImageForm::CopyProject {
            fingerprint: image.fingerprint.clone(),
            source_project: image.project.clone(),
            target_project: "default".into(),
            public: image.public,
            field: 0,
        });
    }

    fn open_image_publish_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            self.image_form = Some(ImageForm::Publish {
                spec: ImagePublishSpec {
                    instance: instance.name.clone(),
                    project: instance.project.clone(),
                    alias: format!("{}-image", instance.name),
                    description: String::new(),
                    public: false,
                },
                field: 0,
            });
            self.model.last_error = None;
        }
    }

    fn open_interactive_exec(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        if instance.status != "Running" {
            self.model.last_error = Some("interactive exec requires a running instance".to_owned());
            return AppCommand::None;
        }
        AppCommand::InteractiveExec {
            name: instance.name.clone(),
            project: instance.project.clone(),
            shell: "/bin/sh".to_owned(),
        }
    }

    fn open_interactive_console(&mut self, force: bool) -> AppCommand {
        if force && !self.supports_extension("console_force") {
            self.model.last_error =
                Some("forced console unavailable: server lacks console_force".into());
            return AppCommand::None;
        }
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        AppCommand::InteractiveConsole {
            name: instance.name.clone(),
            project: instance.project.clone(),
            force,
        }
    }

    fn open_vga_console(&mut self, force: bool) -> AppCommand {
        if force && !self.supports_extension("console_force") {
            self.model.last_error =
                Some("forced console unavailable: server lacks console_force".into());
            return AppCommand::None;
        }
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        if instance.instance_type != "virtual-machine" {
            self.model.last_error =
                Some("VGA console is only available for virtual machines".to_owned());
            return AppCommand::None;
        }
        AppCommand::VgaConsole {
            name: instance.name.clone(),
            project: instance.project.clone(),
            force,
        }
    }

    fn handle_image_form_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_import_alias = self.supports_extension("image_import_alias");
        let can_image_restrictions = self.supports_extension("image_restrictions");
        let form = self.image_form.as_mut().expect("image form");
        if key.code == KeyCode::Esc {
            self.image_form = None;
            return AppCommand::None;
        }
        match form {
            ImageForm::Edit {
                spec,
                fields,
                field,
            } => match key.code {
                KeyCode::Tab => {
                    *field = (*field + 1) % 11;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + 10) % 11;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field >= 9 => {
                    if *field == 9 {
                        spec.public = !spec.public;
                    } else {
                        spec.auto_update = !spec.auto_update;
                    }
                    AppCommand::None
                }
                KeyCode::Backspace if *field < 9 => {
                    fields[*field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if *field < 9 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    fields[*field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *field < 10 => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::Enter => {
                    if !can_image_restrictions
                        && fields[4..].iter().any(|value| !value.trim().is_empty())
                    {
                        self.model.last_error = Some(
                            "image restrictions unavailable: server lacks image_restrictions"
                                .into(),
                        );
                        return AppCommand::None;
                    }
                    for (key, value) in [
                        "description",
                        "os",
                        "release",
                        "variant",
                        "requirements.privileged",
                        "requirements.nesting",
                        "requirements.secureboot",
                        "requirements.cdrom_agent",
                        "requirements.cdrom_cloud_init",
                    ]
                    .into_iter()
                    .zip(fields.iter())
                    {
                        if value.trim().is_empty() {
                            spec.properties.remove(key);
                        } else {
                            spec.properties
                                .insert(key.to_owned(), value.trim().to_owned());
                        }
                    }
                    AppCommand::UpdateImage(spec.clone())
                }
                _ => AppCommand::None,
            },
            ImageForm::Import {
                project,
                path,
                aliases,
                public,
                field,
            } => match key.code {
                KeyCode::Tab => {
                    *field = (*field + 1) % 4;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 3 => {
                    *public = !*public;
                    AppCommand::None
                }
                KeyCode::Backspace if *field < 3 => {
                    match *field {
                        0 => project.pop(),
                        1 => path.pop(),
                        _ => aliases.pop(),
                    };
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if *field < 3 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    match *field {
                        0 => project.push(c),
                        1 => path.push(c),
                        _ => aliases.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter if *field < 3 => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("image archive path is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter if !aliases.trim().is_empty() && !can_import_alias => {
                    self.model.last_error = Some(
                        "import-time aliases unavailable: server lacks image_import_alias".into(),
                    );
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ImportImage {
                    project: project.trim().to_owned(),
                    path: path.trim().to_owned(),
                    aliases: split_csv(aliases),
                    public: *public,
                },
                _ => AppCommand::None,
            },
            ImageForm::Export {
                fingerprint,
                project,
                path,
            } => match key.code {
                KeyCode::Backspace => {
                    path.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("image export path is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ExportImage {
                    fingerprint: fingerprint.clone(),
                    project: project.clone(),
                    path: path.trim().to_owned(),
                },
                _ => AppCommand::None,
            },
            ImageForm::Publish { spec, field } => match key.code {
                KeyCode::Tab => {
                    *field = (*field + 1) % 3;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                    spec.public = !spec.public;
                    AppCommand::None
                }
                KeyCode::Backspace if *field < 2 => {
                    if *field == 0 {
                        spec.alias.pop();
                    } else {
                        spec.description.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if *field < 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    if *field == 0 {
                        spec.alias.push(c);
                    } else {
                        spec.description.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Enter if *field < 2 => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::PublishInstanceImage(spec.clone()),
                _ => AppCommand::None,
            },
            ImageForm::Push {
                fingerprint,
                project,
                fields,
                field,
            } => match key.code {
                KeyCode::Tab => {
                    *field = (*field + 1) % fields.len();
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + fields.len() - 1) % fields.len();
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    fields[*field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    fields[*field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *field + 1 < fields.len() => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::Enter if fields[0].trim().is_empty() || fields[2].trim().is_empty() => {
                    self.model.last_error =
                        Some("target URL and receive secret are required".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::PushImage {
                    fingerprint: fingerprint.clone(),
                    project: project.clone(),
                    target: fields[0].trim().into(),
                    target_project: fields[1].trim().into(),
                    secret: fields[2].trim().into(),
                    certificate_path: fields[3].trim().into(),
                    aliases: split_csv(&fields[4]),
                    profiles: split_csv(&fields[5]),
                },
                _ => AppCommand::None,
            },
            ImageForm::CopyProject {
                fingerprint,
                source_project,
                target_project,
                public,
                field,
            } => match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    *field = 1 - *field;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 1 => {
                    *public = !*public;
                    AppCommand::None
                }
                KeyCode::Backspace if *field == 0 => {
                    target_project.pop();
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if *field == 0 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    target_project.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *field == 0 => {
                    *field = 1;
                    AppCommand::None
                }
                KeyCode::Enter
                    if target_project.trim().is_empty()
                        || target_project.trim() == source_project =>
                {
                    self.model.last_error =
                        Some("target project must be non-empty and different from source".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::CopyImageProject {
                    fingerprint: fingerprint.clone(),
                    source_project: source_project.clone(),
                    target_project: target_project.trim().into(),
                    public: *public,
                },
                _ => AppCommand::None,
            },
        }
    }

    fn handle_image_alias_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .image_alias_form
            .as_mut()
            .expect("image alias form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.image_alias_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                form.field = 1 - form.field;
                AppCommand::None
            }
            KeyCode::Enter if form.field == 0 => {
                form.field = 1;
                AppCommand::None
            }
            KeyCode::Enter if form.alias.trim().is_empty() => {
                self.model.last_error = Some("image alias name is required".to_owned());
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::CreateImageAlias {
                alias: form.alias.trim().to_owned(),
                fingerprint: form.fingerprint.clone(),
                project: form.project.clone(),
                description: form.description.trim().to_owned(),
            },
            KeyCode::Backspace => {
                if form.field == 0 {
                    form.alias.pop();
                } else {
                    form.description.pop();
                }
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if form.field == 0 {
                    form.alias.push(character);
                } else {
                    form.description.push(character);
                }
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_image_alias_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .image_alias_manager
            .as_mut()
            .expect("image alias manager");
        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase();
            let input = manager.delete_input.as_mut().expect("delete input");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let alias = manager
                        .aliases
                        .get(manager.selected)
                        .map(|v| v.name.clone())
                        .unwrap_or_default();
                    manager.delete_input = None;
                    AppCommand::DeleteImageAlias {
                        alias,
                        project: manager.project.clone(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(editor) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if editor.field == 0 => {
                    editor.field = 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    editor.field = 1 - editor.field;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if editor.field == 0 {
                        editor.name.pop();
                    } else {
                        editor.description.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if editor.field == 0 {
                        editor.name.push(c);
                    } else {
                        editor.description.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Enter if editor.name.trim().is_empty() => {
                    self.model.last_error = Some("alias name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::UpdateImageAlias {
                    alias: editor.original.clone(),
                    new_name: editor.name.trim().to_owned(),
                    fingerprint: manager.fingerprint.clone(),
                    project: manager.project.clone(),
                    description: editor.description.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.image_alias_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.aliases.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.aliases.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.aliases.is_empty() => {
                manager.selected =
                    (manager.selected + manager.aliases.len() - 1) % manager.aliases.len();
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.aliases.is_empty() => {
                let alias = &manager.aliases[manager.selected];
                manager.editor = Some(ImageAliasEditor {
                    original: alias.name.clone(),
                    name: alias.name.clone(),
                    description: alias.description.clone(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.aliases.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn launch_selected_image(&mut self) {
        if let Some(image) = self.model.images.get(self.selected_image) {
            self.create_form = Some(CreateForm::from_image(image));
            self.model.last_error = None;
        }
    }

    fn open_artifacts_form(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        let name = instance.name.clone();
        let project = instance.project.clone();
        self.artifacts_form = Some(ArtifactsForm::new(instance));
        self.model.last_error = None;
        AppCommand::ReadArtifacts { name, project }
    }

    fn handle_artifacts_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .artifacts_form
            .as_mut()
            .expect("artifacts form checked before handling key");
        if form.retention_preview {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('V') | KeyCode::Enter) {
                form.retention_preview = false;
            }
            return AppCommand::None;
        }
        if let Some((importing, path)) = form.transfer.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    form.transfer = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    path.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("backup archive path is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let path = path.trim().to_owned();
                    let importing = *importing;
                    form.transfer = None;
                    if importing {
                        AppCommand::ImportBackup {
                            project: form.project.clone(),
                            path,
                        }
                    } else {
                        AppCommand::ExportBackup {
                            name: form.instance_name.clone(),
                            project: form.project.clone(),
                            backup: form.selected_name().unwrap_or_default().to_owned(),
                            path,
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        if form.rename_input.is_some() {
            let input = form
                .rename_input
                .as_mut()
                .expect("snapshot rename remains present");
            return match key.code {
                KeyCode::Esc => {
                    form.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if input.trim().is_empty() => {
                    self.model.last_error = Some("new snapshot name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let new_name = input.trim().to_owned();
                    let snapshot = form.selected_name().unwrap_or_default().to_owned();
                    form.rename_input = None;
                    AppCommand::RenameSnapshot {
                        name: form.instance_name.clone(),
                        project: form.project.clone(),
                        snapshot,
                        new_name,
                    }
                }
                _ => AppCommand::None,
            };
        }
        if form.delete_input.is_some() {
            let expected = form.delete_phrase().unwrap_or_default();
            let input = form
                .delete_input
                .as_mut()
                .expect("artifact confirmation remains present");
            return match key.code {
                KeyCode::Esc => {
                    form.delete_input = None;
                    form.restoring = false;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let name = form.instance_name.clone();
                    let project = form.project.clone();
                    let artifact = form.selected_name().unwrap_or_default().to_owned();
                    if form.restoring {
                        form.delete_input = None;
                        form.restoring = false;
                        AppCommand::RestoreSnapshot {
                            name,
                            project,
                            snapshot: artifact,
                            stateful: false,
                        }
                    } else {
                        match form.tab {
                            ArtifactTab::Snapshots => AppCommand::DeleteSnapshot {
                                name,
                                project,
                                snapshot: artifact,
                            },
                            ArtifactTab::Backups => AppCommand::DeleteBackup {
                                name,
                                project,
                                backup: artifact,
                            },
                        }
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.artifacts_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                form.tab = if form.tab == ArtifactTab::Snapshots {
                    ArtifactTab::Backups
                } else {
                    ArtifactTab::Snapshots
                };
                form.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if form.len() > 0 => {
                form.selected = (form.selected + 1) % form.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if form.len() > 0 => {
                form.selected = (form.selected + form.len() - 1) % form.len();
                AppCommand::None
            }
            KeyCode::Char('d') if form.selected_name().is_some() => {
                form.restoring = false;
                form.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('r')
                if form.tab == ArtifactTab::Snapshots && form.selected_name().is_some() =>
            {
                form.restoring = true;
                form.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R')
                if form.tab == ArtifactTab::Snapshots && form.selected_name().is_some() =>
            {
                form.rename_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('e')
                if form.tab == ArtifactTab::Backups && form.selected_name().is_some() =>
            {
                let backup = form.selected_name().unwrap_or("backup");
                form.transfer = Some((
                    false,
                    format!("./{}-{}-backup.tar.gz", form.instance_name, backup),
                ));
                AppCommand::None
            }
            KeyCode::Char('i') => {
                form.transfer = Some((true, "./backup.tar.gz".to_owned()));
                AppCommand::None
            }
            KeyCode::Char('n') => {
                let instance = crate::incus::InstanceSummary {
                    name: form.instance_name.clone(),
                    project: form.project.clone(),
                    instance_type: form.instance_type.clone(),
                    ..Default::default()
                };
                if form.tab == ArtifactTab::Snapshots {
                    self.snapshot_form = Some(SnapshotForm::new(&instance));
                } else {
                    self.backup_form = Some(BackupForm::new(&instance));
                }
                self.artifacts_form = None;
                AppCommand::None
            }
            KeyCode::Char('V') => {
                form.retention_preview = true;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_rename_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            self.rename_form = Some(RenameForm {
                current_name: instance.name.clone(),
                project: instance.project.clone(),
                new_name: String::new(),
            });
            self.model.last_error = None;
        }
    }

    fn handle_rename_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .rename_form
            .as_mut()
            .expect("rename form checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.rename_form = None;
                AppCommand::None
            }
            KeyCode::Enter if form.new_name.trim().is_empty() => {
                self.model.last_error = Some("new instance name is required".to_owned());
                AppCommand::None
            }
            KeyCode::Enter if form.new_name.trim() == form.current_name => {
                self.model.last_error = Some("new instance name must be different".to_owned());
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::RenameInstance {
                name: form.current_name.clone(),
                project: form.project.clone(),
                new_name: form.new_name.trim().to_owned(),
            },
            KeyCode::Backspace => {
                form.new_name.pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.new_name.push(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_rebuild_form(&mut self) {
        if !self.require_extension("instances_rebuild", "instance rebuild") {
            return;
        }
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return;
        };
        if instance.status != "Stopped" {
            self.model.last_error = Some("stop the instance before rebuilding it".to_owned());
            return;
        }
        self.rebuild_form = Some(RebuildForm {
            name: instance.name.clone(),
            project: instance.project.clone(),
            image: String::new(),
            confirmation: String::new(),
            field: 0,
        });
        self.model.last_error = None;
    }

    fn handle_rebuild_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .rebuild_form
            .as_mut()
            .expect("rebuild form checked before handling key");
        let expected = format!("rebuild {}", form.name);
        match key.code {
            KeyCode::Esc => {
                self.rebuild_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                form.field = 1 - form.field;
                AppCommand::None
            }
            KeyCode::Backspace => {
                if form.field == 0 {
                    form.image.pop();
                } else {
                    form.confirmation.pop();
                }
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if form.field == 0 {
                    form.image.push(character);
                } else {
                    form.confirmation.push(character);
                }
                AppCommand::None
            }
            KeyCode::Enter if form.field == 0 => {
                form.field = 1;
                AppCommand::None
            }
            KeyCode::Enter if form.confirmation != expected => {
                self.model.last_error =
                    Some(format!("confirmation must exactly match: {expected}"));
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::RebuildInstance {
                name: form.name.clone(),
                project: form.project.clone(),
                image: (!form.image.trim().is_empty()).then(|| form.image.trim().to_owned()),
            },
            _ => AppCommand::None,
        }
    }

    fn open_log_manager(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        AppCommand::ReadInstanceLogs {
            name: instance.name.clone(),
            project: instance.project.clone(),
        }
    }

    fn handle_log_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .log_manager
            .as_mut()
            .expect("log manager checked before handling key");
        if manager.content.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.content = None;
            }
            return AppCommand::None;
        }
        if let Some(input) = manager.delete_input.as_mut() {
            let expected = format!(
                "delete log {}",
                manager
                    .logs
                    .get(manager.selected)
                    .map(String::as_str)
                    .unwrap_or_default()
            );
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let filename = manager.logs[manager.selected].clone();
                    AppCommand::DeleteInstanceLog {
                        name: manager.name.clone(),
                        project: manager.project.clone(),
                        filename,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.clear_console_input.as_mut() {
            let expected = format!("clear console {}", manager.name);
            return match key.code {
                KeyCode::Esc => {
                    manager.clear_console_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => AppCommand::ClearConsoleLog {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                },
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.cleanup_exec_input.as_mut() {
            let count = manager
                .logs
                .iter()
                .filter(|log| log.starts_with("exec-output/"))
                .count();
            let expected = format!("delete {count} exec outputs {}", manager.name);
            return match key.code {
                KeyCode::Esc => {
                    manager.cleanup_exec_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => AppCommand::CleanupInstanceExecOutputs {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    filenames: manager
                        .logs
                        .iter()
                        .filter_map(|log| log.strip_prefix("exec-output/").map(ToOwned::to_owned))
                        .collect(),
                },
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.log_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.logs.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.logs.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.logs.is_empty() => {
                manager.selected = (manager.selected + manager.logs.len() - 1) % manager.logs.len();
                AppCommand::None
            }
            KeyCode::Enter if !manager.logs.is_empty() => AppCommand::ReadInstanceLog {
                name: manager.name.clone(),
                project: manager.project.clone(),
                filename: manager.logs[manager.selected].clone(),
            },
            KeyCode::Char('d') if !manager.logs.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('c') => {
                manager.clear_console_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('X') if manager.exec_output_count() > 0 => {
                manager.cleanup_exec_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::ReadInstanceLogs {
                name: manager.name.clone(),
                project: manager.project.clone(),
            },
            _ => AppCommand::None,
        }
    }

    fn handle_allocation_explorer_key(&mut self, key: KeyEvent) -> AppCommand {
        let explorer = self
            .allocation_explorer
            .as_mut()
            .expect("allocation explorer checked before handling key");
        if explorer.filtering {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    explorer.filtering = false;
                    explorer.selected = 0;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    explorer.filter.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    explorer.filter.push(c);
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        let count = self
            .model
            .network_allocations
            .iter()
            .filter(|value| allocation_matches(value, &explorer.filter))
            .count();
        match key.code {
            KeyCode::Esc => {
                self.allocation_explorer = None;
                AppCommand::None
            }
            KeyCode::Char('/') => {
                explorer.filtering = true;
                AppCommand::None
            }
            KeyCode::Char('L') => {
                explorer.filter.clear();
                explorer.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if count > 0 => {
                explorer.selected = (explorer.selected + 1) % count;
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if count > 0 => {
                explorer.selected = (explorer.selected + count - 1) % count;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_metrics_explorer(&mut self) {
        if !self.require_extension("metrics", "instance metrics") {
            return;
        }
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return;
        };
        if self.model.metrics.is_none() {
            self.model.last_error = Some("the server did not expose /1.0/metrics".to_owned());
            return;
        }
        self.metrics_explorer = Some(MetricsExplorer {
            instance: instance.name.clone(),
            project: instance.project.clone(),
            selected: 0,
            filter: String::new(),
            filtering: false,
        });
    }

    fn handle_metrics_explorer_key(&mut self, key: KeyEvent) -> AppCommand {
        let explorer = self
            .metrics_explorer
            .as_mut()
            .expect("metrics explorer checked before handling key");
        if explorer.filtering {
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    explorer.filtering = false;
                    explorer.selected = 0;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    explorer.filter.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    explorer.filter.push(c);
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        let count = self
            .model
            .metrics
            .as_deref()
            .map(|metrics| metric_rows(metrics, explorer).len())
            .unwrap_or(0);
        match key.code {
            KeyCode::Esc => {
                self.metrics_explorer = None;
                AppCommand::None
            }
            KeyCode::Char('/') => {
                explorer.filtering = true;
                AppCommand::None
            }
            KeyCode::Char('L') => {
                explorer.filter.clear();
                explorer.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if count > 0 => {
                explorer.selected = (explorer.selected + 1) % count;
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if count > 0 => {
                explorer.selected = (explorer.selected + count - 1) % count;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_maintenance_manager(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        AppCommand::ReadInstanceMaintenance {
            name: instance.name.clone(),
            project: instance.project.clone(),
        }
    }

    fn handle_maintenance_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_edit_metadata = self.supports_extension("container_edit_metadata");
        let can_memory = self.supports_extension("instance_debug_memory");
        let can_repair = self.supports_extension("instances_debug_repair");
        let can_nbd = self.supports_extension("instance_nbd");
        let can_bitmaps = self.supports_extension("storage_volume_nbd");
        let can_screenshot = self.supports_extension("instance_console_screenshot");
        let can_template_permissions = self.supports_extension("image_template_permissions");
        let selected_is_vm = self
            .maintenance_manager
            .as_ref()
            .and_then(|manager| {
                self.model.instances.iter().find(|instance| {
                    instance.name == manager.name && instance.project == manager.project
                })
            })
            .is_some_and(|instance| instance.instance_type == "virtual-machine");
        let manager = self
            .maintenance_manager
            .as_mut()
            .expect("maintenance manager checked");
        if manager.proxy_address.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.proxy_address = None;
            }
            return AppCommand::None;
        }
        if let Some((name, granularity, persistent, disabled, field)) = manager.bitmap_form.as_mut()
        {
            return match key.code {
                KeyCode::Esc => {
                    manager.bitmap_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if *field < 3 => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                    *persistent = !*persistent;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 3 => {
                    *disabled = !*disabled;
                    AppCommand::None
                }
                KeyCode::Backspace if *field < 2 => {
                    if *field == 0 {
                        name.pop();
                    } else {
                        granularity.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if *field == 0 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    name.push(c);
                    AppCommand::None
                }
                KeyCode::Char(c) if *field == 1 && c.is_ascii_digit() => {
                    granularity.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if name.trim().is_empty() => {
                    self.model.last_error = Some("bitmap name is required".into());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let Ok(granularity) = granularity.parse::<i64>() else {
                        self.model.last_error =
                            Some("bitmap granularity must be a positive byte count".into());
                        return AppCommand::None;
                    };
                    if granularity <= 0 || granularity & (granularity - 1) != 0 {
                        self.model.last_error =
                            Some("bitmap granularity must be a positive power of two".into());
                        return AppCommand::None;
                    }
                    AppCommand::CreateInstanceBitmaps {
                        name: manager.name.clone(),
                        project: manager.project.clone(),
                        spec: StorageVolumeBitmapSpec {
                            name: name.trim().into(),
                            persistent: *persistent,
                            disabled: *disabled,
                            granularity,
                        },
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some((protocol, port, field)) = manager.protocol_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.protocol_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter if *field == 0 => {
                    *field = 1;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 0 => {
                    *protocol = if protocol == "sftp" {
                        "nbd".into()
                    } else {
                        "sftp".into()
                    };
                    AppCommand::None
                }
                KeyCode::Backspace if *field == 1 => {
                    port.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if *field == 1 && c.is_ascii_digit() => {
                    port.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if protocol == "nbd" && !can_nbd => {
                    self.model.last_error =
                        Some("instance NBD unavailable: server lacks instance_nbd".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::StartInstanceProtocolProxy {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    protocol: protocol.clone(),
                    port: port.parse().unwrap_or(0),
                },
                _ => AppCommand::None,
            };
        }
        if let Some((property, value, field)) = manager.metadata_editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.metadata_editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    *field = 1 - *field;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if *field == 0 {
                        property.pop();
                    } else {
                        value.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if *field == 0 {
                        property.push(c);
                    } else {
                        value.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Enter if property.trim().is_empty() => {
                    self.model.last_error = Some("metadata property name is required".into());
                    AppCommand::None
                }
                KeyCode::Enter if !can_edit_metadata => {
                    self.model.last_error = Some(
                        "metadata editing unavailable: server lacks container_edit_metadata".into(),
                    );
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let Some(etag) = manager.metadata_etag.clone() else {
                        self.model.last_error = Some("metadata update requires an ETag".into());
                        return AppCommand::None;
                    };
                    let mut metadata = manager.metadata.clone();
                    if !metadata
                        .get("properties")
                        .is_some_and(serde_json::Value::is_object)
                    {
                        metadata["properties"] = serde_json::json!({});
                    }
                    metadata["properties"][property.trim()] =
                        serde_json::Value::String(value.clone());
                    AppCommand::UpdateInstanceMetadata {
                        name: manager.name.clone(),
                        project: manager.project.clone(),
                        metadata,
                        etag,
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some((path, uid, gid, mode, field)) = manager.template_permissions.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.template_permissions = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if *field < 3 => {
                    *field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    *field = (*field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match *field {
                        0 => {
                            path.pop();
                        }
                        1 => {
                            uid.pop();
                        }
                        2 => {
                            gid.pop();
                        }
                        _ => {
                            mode.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if *field == 0 || c.is_ascii_digit() {
                        match *field {
                            0 => path.push(c),
                            1 => uid.push(c),
                            2 => gid.push(c),
                            _ => mode.push(c),
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("template target path is required".into());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let Some(etag) = manager.metadata_etag.clone() else {
                        self.model.last_error =
                            Some("template permission update requires metadata ETag".into());
                        return AppCommand::None;
                    };
                    let uid_value = uid
                        .parse::<u64>()
                        .map_err(|_| "UID must be an unsigned integer");
                    let gid_value = gid
                        .parse::<u64>()
                        .map_err(|_| "GID must be an unsigned integer");
                    let mode_value = u64::from_str_radix(mode.trim_start_matches('0'), 8)
                        .map_err(|_| "mode must be octal, for example 0644");
                    let (Ok(uid_value), Ok(gid_value), Ok(mode_value)) =
                        (uid_value, gid_value, mode_value)
                    else {
                        self.model.last_error =
                            Some("UID/GID must be integers and mode must be octal".into());
                        return AppCommand::None;
                    };
                    let template = manager
                        .templates
                        .get(manager.selected)
                        .cloned()
                        .unwrap_or_default();
                    let mut metadata = manager.metadata.clone();
                    if !metadata
                        .get("templates")
                        .is_some_and(serde_json::Value::is_object)
                    {
                        metadata["templates"] = serde_json::json!({});
                    }
                    let templates = metadata["templates"]
                        .as_object_mut()
                        .expect("templates object");
                    templates.retain(|_, entry| {
                        entry.get("template").and_then(serde_json::Value::as_str)
                            != Some(template.as_str())
                    });
                    templates.insert(
                        path.trim().into(),
                        serde_json::json!({
                            "template":template, "when":["create","copy"], "create_only":false,
                            "uid":uid_value, "gid":gid_value, "mode":mode_value,
                        }),
                    );
                    AppCommand::UpdateInstanceMetadata {
                        name: manager.name.clone(),
                        project: manager.project.clone(),
                        metadata,
                        etag,
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some((name, content, field)) = manager.template_editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.template_editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    *field = 1 - *field;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if *field == 0 {
                        name.pop();
                    } else {
                        content.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Enter if *field == 0 => {
                    *field = 1;
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if *field == 0 {
                        name.push(c);
                    } else {
                        content.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Char('s')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && !name.trim().is_empty() =>
                {
                    AppCommand::WriteInstanceTemplate {
                        name: manager.name.clone(),
                        project: manager.project.clone(),
                        template: name.trim().into(),
                        content: content.clone(),
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.delete_input.as_mut() {
            let template = manager
                .templates
                .get(manager.selected)
                .cloned()
                .unwrap_or_default();
            let expected = format!("delete template {template}");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => AppCommand::DeleteInstanceTemplate {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    template,
                },
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(path) = manager.dump_path.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.dump_path = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    path.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !path.trim().is_empty() => AppCommand::ExportInstanceMemory {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    path: path.trim().into(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(path) = manager.screenshot_path.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.screenshot_path = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    path.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("screenshot destination is required".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ExportInstanceScreenshot {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    path: path.trim().into(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.repair_input.as_mut() {
            let expected = format!("repair {}", manager.name);
            return match key.code {
                KeyCode::Esc => {
                    manager.repair_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => AppCommand::RepairInstance {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                },
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        let len = if manager.tab == MaintenanceTab::Templates {
            manager.templates.len()
        } else {
            0
        };
        match key.code {
            KeyCode::Esc => {
                self.maintenance_manager = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Right => {
                manager.tab = match manager.tab {
                    MaintenanceTab::Metadata => MaintenanceTab::Templates,
                    MaintenanceTab::Templates => MaintenanceTab::Diagnostics,
                    MaintenanceTab::Diagnostics => MaintenanceTab::Metadata,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::BackTab | KeyCode::Left => {
                manager.tab = match manager.tab {
                    MaintenanceTab::Metadata => MaintenanceTab::Diagnostics,
                    MaintenanceTab::Templates => MaintenanceTab::Metadata,
                    MaintenanceTab::Diagnostics => MaintenanceTab::Templates,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if len > 0 => {
                manager.selected = (manager.selected + 1) % len;
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if len > 0 => {
                manager.selected = (manager.selected + len - 1) % len;
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == MaintenanceTab::Metadata => {
                manager.metadata_editor = Some((String::new(), String::new(), 0));
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == MaintenanceTab::Templates => {
                manager.template_editor = Some((String::new(), String::new(), 0));
                AppCommand::None
            }
            KeyCode::Enter | KeyCode::Char('e')
                if manager.tab == MaintenanceTab::Templates && len > 0 =>
            {
                AppCommand::ReadInstanceTemplate {
                    name: manager.name.clone(),
                    project: manager.project.clone(),
                    template: manager.templates[manager.selected].clone(),
                }
            }
            KeyCode::Char('d') if manager.tab == MaintenanceTab::Templates && len > 0 => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('P')
                if manager.tab == MaintenanceTab::Templates
                    && len > 0
                    && can_template_permissions =>
            {
                let template = manager.templates[manager.selected].clone();
                let existing = manager
                    .metadata
                    .get("templates")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|templates| {
                        templates.iter().find(|(_, entry)| {
                            entry.get("template").and_then(serde_json::Value::as_str)
                                == Some(template.as_str())
                        })
                    });
                let path = existing
                    .map(|(path, _)| path.clone())
                    .unwrap_or_else(|| format!("/{template}"));
                let field = |key: &str, default: &str| {
                    existing
                        .and_then(|(_, entry)| entry.get(key))
                        .map(display_json_scalar)
                        .unwrap_or_else(|| default.into())
                };
                let mode = existing
                    .and_then(|(_, entry)| entry.get("mode"))
                    .and_then(serde_json::Value::as_u64)
                    .map(|mode| format!("0{mode:o}"))
                    .unwrap_or_else(|| field("mode", "0644"));
                manager.template_permissions =
                    Some((path, field("uid", "0"), field("gid", "0"), mode, 0));
                AppCommand::None
            }
            KeyCode::Char('P') if manager.tab == MaintenanceTab::Templates => {
                self.model.last_error = Some(
                    "template permissions unavailable: server lacks image_template_permissions"
                        .into(),
                );
                AppCommand::None
            }
            KeyCode::Char('m') if manager.tab == MaintenanceTab::Diagnostics && can_memory => {
                manager.dump_path = Some(format!("./{}-memory.core", manager.name));
                AppCommand::None
            }
            KeyCode::Char('m') if manager.tab == MaintenanceTab::Diagnostics => {
                self.model.last_error =
                    Some("memory debugging unavailable: server lacks instance_debug_memory".into());
                AppCommand::None
            }
            KeyCode::Char('r') if manager.tab == MaintenanceTab::Diagnostics && can_repair => {
                manager.repair_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('r') if manager.tab == MaintenanceTab::Diagnostics => {
                self.model.last_error =
                    Some("instance repair unavailable: server lacks instances_debug_repair".into());
                AppCommand::None
            }
            KeyCode::Char('s')
                if manager.tab == MaintenanceTab::Diagnostics
                    && can_screenshot
                    && selected_is_vm =>
            {
                manager.screenshot_path = Some(format!("./{}-console.png", manager.name));
                AppCommand::None
            }
            KeyCode::Char('s') if manager.tab == MaintenanceTab::Diagnostics => {
                self.model.last_error = Some(if !selected_is_vm {
                    "console screenshots are only available for virtual machines".into()
                } else {
                    "console screenshots unavailable: server lacks instance_console_screenshot"
                        .into()
                });
                AppCommand::None
            }
            KeyCode::Char('p') if manager.tab == MaintenanceTab::Diagnostics => {
                manager.protocol_form = Some(("sftp".into(), "0".into(), 0));
                AppCommand::None
            }
            KeyCode::Char('b')
                if manager.tab == MaintenanceTab::Diagnostics && can_bitmaps && selected_is_vm =>
            {
                manager.bitmap_form = Some((String::new(), "32768".into(), true, false, 0));
                AppCommand::None
            }
            KeyCode::Char('b') if manager.tab == MaintenanceTab::Diagnostics => {
                self.model.last_error = Some(if !selected_is_vm {
                    "coordinated dirty bitmaps are only available for virtual machines".into()
                } else {
                    "coordinated dirty bitmaps unavailable: server lacks storage_volume_nbd".into()
                });
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_settings_manager(&mut self) {
        self.settings_manager = Some(SettingsManager {
            tab: SettingsTab::Warnings,
            selected: 0,
            delete_input: None,
            certificate_form: None,
            certificate_edit: None,
            trust_token: None,
            cluster_form: None,
            cluster_action: None,
            cluster_join_name: None,
            cluster_rename_input: None,
            cluster_detail: None,
            cluster_config: None,
            cluster_certificate: None,
            capability_filter: None,
            capability_selected: 0,
        });
        self.model.last_error = None;
    }

    pub fn load_logging_targets(&mut self, definition: ResourceDefinition) {
        let supports_webhook = self.supports_extension("server_logging_webhook");
        self.logging_target_manager = Some(LoggingTargetManager::new(definition, supports_webhook));
        self.model.last_error = None;
    }

    pub fn load_scriptlets(&mut self, definition: ResourceDefinition) {
        let supports_placement = self.supports_extension("instances_placement_scriptlet");
        let supports_authorization = self.supports_extension("authorization_scriptlet");
        self.scriptlet_manager = Some(ScriptletManager::new(
            definition,
            supports_placement,
            supports_authorization,
        ));
        self.model.last_error = None;
    }

    pub fn load_qemu_scriptlet(&mut self, definition: ResourceDefinition) {
        self.scriptlet_manager = Some(ScriptletManager::new_qemu(definition));
        self.model.last_error = None;
    }

    pub async fn update_scriptlet(
        &mut self,
        client: &IncusClient,
        definition: ResourceDefinition,
        value: serde_json::Value,
        activated_source: String,
    ) {
        match client.update_resource(&definition, &value).await {
            Ok(_) => {
                if let Some(manager) = self.scriptlet_manager.as_mut() {
                    manager.definition.value = value;
                    manager.original = activated_source.clone();
                    manager.draft = activated_source;
                    manager.cursor = manager.draft.len();
                    manager.activation_input = None;
                    manager.rollback = false;
                    manager.validation = vec![
                        "PASS: Incus accepted and activated the scriptlet; rollback baseline updated"
                            .into(),
                    ];
                }
                self.refresh(client).await;
            }
            Err(error) => {
                if let Some(manager) = self.scriptlet_manager.as_mut() {
                    manager.activation_input = None;
                    manager.validation = vec![format!(
                        "ERROR: Incus rejected the staged scriptlet; active configuration was unchanged: {error}"
                    )];
                }
                self.model.last_error = Some(error.to_string());
            }
        }
    }

    pub async fn import_preseed(&mut self, path: String) {
        let result = async {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(IncusError::Io)?;
            serde_json::from_str::<PreseedSpec>(&content).map_err(|error| {
                IncusError::InvalidResource(format!("invalid typed preseed: {error}"))
            })
        }
        .await;
        match result {
            Ok(spec) => {
                if let Some(manager) = self.preseed_manager.as_mut() {
                    manager.spec = spec;
                    manager.transfer = None;
                    manager.preview = true;
                    manager.results = vec![format!("PASS: imported and parsed {path}")];
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn export_preseed(&mut self, path: String, spec: PreseedSpec) {
        let result = async {
            let content = serde_json::to_string_pretty(&spec).map_err(|error| {
                IncusError::InvalidResource(format!("cannot serialize typed preseed: {error}"))
            })?;
            tokio::fs::write(&path, format!("{content}\n"))
                .await
                .map_err(IncusError::Io)
        }
        .await;
        match result {
            Ok(()) => {
                if let Some(manager) = self.preseed_manager.as_mut() {
                    manager.transfer = None;
                    manager.results = vec![format!("PASS: exported typed preseed to {path}")];
                }
                self.model.last_error = None;
            }
            Err(error) => self.model.last_error = Some(error.to_string()),
        }
    }

    pub async fn apply_preseed(&mut self, client: &IncusClient, spec: PreseedSpec) {
        let mut results = Vec::new();
        if spec.server_enabled {
            match client
                .resource_definition(ResourceKind::Server, "", "")
                .await
            {
                Ok(definition) => {
                    let mut value = definition.value.clone();
                    let config = value.as_object_mut().expect("server configuration");
                    for (key, content) in [
                        ("core.https_address", spec.https_address.trim()),
                        ("core.metrics_address", spec.metrics_address.trim()),
                    ] {
                        if content.is_empty() {
                            config.remove(key);
                        } else {
                            config.insert(key.into(), serde_json::Value::String(content.into()));
                        }
                    }
                    match client.update_resource(&definition, &value).await {
                        Ok(_) => results.push("PASS server: configuration applied".into()),
                        Err(error) => results.push(format!("FAIL server: {error}")),
                    }
                }
                Err(error) => results.push(format!("FAIL server read: {error}")),
            }
        } else {
            results.push("SKIP server".into());
        }
        if spec.cluster_enabled {
            let certificate = if spec.cluster_certificate_path.trim().is_empty() {
                Ok(String::new())
            } else {
                tokio::fs::read_to_string(spec.cluster_certificate_path.trim())
                    .await
                    .map_err(IncusError::Io)
            };
            match certificate {
                Ok(certificate) => {
                    let value = serde_json::json!({
                        "enabled": true,
                        "server_name": spec.cluster_server_name.trim(),
                        "server_address": spec.cluster_server_address.trim(),
                        "cluster_address": spec.cluster_address.trim(),
                        "cluster_token": spec.cluster_token.trim(),
                        "cluster_certificate": certificate,
                        "cluster_password": spec.cluster_password,
                    });
                    match client.configure_cluster(&value).await {
                        Ok(_) => {
                            results.push(format!(
                                "PASS cluster: configured {}",
                                spec.cluster_server_name
                            ));
                            if !spec.cluster_group.trim().is_empty() {
                                match client
                                    .create_cluster_group(
                                        spec.cluster_group.trim(),
                                        "Managed by virtty typed preseed",
                                        vec![spec.cluster_server_name.trim().to_owned()],
                                    )
                                    .await
                                {
                                    Ok(_) => results.push(format!(
                                        "PASS cluster group: created {}",
                                        spec.cluster_group
                                    )),
                                    Err(error) => results.push(format!(
                                        "FAIL cluster group {}: {error}",
                                        spec.cluster_group
                                    )),
                                }
                            }
                        }
                        Err(error) => results.push(format!("FAIL cluster configure: {error}")),
                    }
                }
                Err(error) => results.push(format!("FAIL cluster certificate: {error}")),
            }
        } else {
            results.push("SKIP cluster".into());
        }
        if spec.storage_enabled {
            let mut entries = spec.additional_pools.clone();
            if !spec.pool_name.trim().is_empty() {
                entries.insert(
                    0,
                    PreseedPoolSpec {
                        name: spec.pool_name.trim().into(),
                        driver: spec.pool_driver.clone(),
                        source: spec.pool_source.trim().into(),
                    },
                );
            }
            for entry in entries {
                let mut config = serde_json::Map::new();
                if !entry.source.is_empty() {
                    config.insert("source".into(), entry.source.clone().into());
                }
                let value =
                    serde_json::json!({"name":entry.name, "driver":entry.driver, "config":config});
                match client
                    .create_resource(ResourceKind::StoragePool, "", &value)
                    .await
                {
                    Ok(_) => results.push(format!("PASS storage: created {}", entry.name)),
                    Err(error) => results.push(format!("FAIL storage {}: {error}", entry.name)),
                }
            }
        } else {
            results.push("SKIP storage".into());
        }
        if spec.network_enabled {
            let mut entries = spec.additional_networks.clone();
            if !spec.network_name.trim().is_empty() {
                entries.insert(
                    0,
                    PreseedNetworkSpec {
                        name: spec.network_name.trim().into(),
                        network_type: spec.network_type.clone(),
                        ipv4_address: spec.ipv4_address.clone(),
                        ipv6_address: spec.ipv6_address.clone(),
                    },
                );
            }
            for entry in entries {
                let value = serde_json::json!({"name":entry.name, "type":entry.network_type, "config":{"ipv4.address":entry.ipv4_address,"ipv6.address":entry.ipv6_address}});
                match client
                    .create_resource(ResourceKind::Network, "default", &value)
                    .await
                {
                    Ok(_) => results.push(format!("PASS network: created {}", entry.name)),
                    Err(error) => results.push(format!("FAIL network {}: {error}", entry.name)),
                }
            }
        } else {
            results.push("SKIP network".into());
        }
        if spec.profile_enabled {
            let mut entries = spec.additional_profiles.clone();
            if !spec.profile_name.trim().is_empty() {
                let pool = if !spec.profile_pool.trim().is_empty() {
                    spec.profile_pool.trim().into()
                } else if spec.pool_name.trim().is_empty() {
                    spec.additional_pools
                        .last()
                        .map(|item| item.name.clone())
                        .unwrap_or_default()
                } else {
                    spec.pool_name.trim().into()
                };
                let network = if !spec.profile_network.trim().is_empty() {
                    spec.profile_network.trim().into()
                } else if spec.network_name.trim().is_empty() {
                    spec.additional_networks
                        .last()
                        .map(|item| item.name.clone())
                        .unwrap_or_default()
                } else {
                    spec.network_name.trim().into()
                };
                entries.insert(
                    0,
                    PreseedProfileSpec {
                        name: spec.profile_name.trim().into(),
                        cpu: spec.profile_cpu.trim().into(),
                        memory: spec.profile_memory.trim().into(),
                        pool,
                        network,
                    },
                );
            }
            for entry in entries {
                let mut config = serde_json::Map::new();
                if !entry.cpu.is_empty() {
                    config.insert("limits.cpu".into(), entry.cpu.clone().into());
                }
                if !entry.memory.is_empty() {
                    config.insert("limits.memory".into(), entry.memory.clone().into());
                }
                let mut devices = serde_json::Map::new();
                if !entry.pool.is_empty() {
                    devices.insert(
                        "root".into(),
                        serde_json::json!({"type":"disk","path":"/","pool":entry.pool}),
                    );
                }
                if !entry.network.is_empty() {
                    devices.insert(
                        "eth0".into(),
                        serde_json::json!({"type":"nic","network":entry.network}),
                    );
                }
                let value = serde_json::json!({"name":entry.name,"description":"Managed by virtty typed preseed","config":config,"devices":devices});
                match client
                    .create_resource(ResourceKind::Profile, "default", &value)
                    .await
                {
                    Ok(_) => results.push(format!("PASS profile: created {}", entry.name)),
                    Err(error) => results.push(format!("FAIL profile {}: {error}", entry.name)),
                }
            }
        } else {
            results.push("SKIP profile".into());
        }
        if let Some(manager) = self.preseed_manager.as_mut() {
            manager.results = results;
            manager.preview = true;
        }
        self.refresh(client).await;
    }

    pub fn load_pool_safety(
        &mut self,
        definition: ResourceDefinition,
        resources: serde_json::Value,
    ) {
        let driver = definition
            .value
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                self.model
                    .storage_pools
                    .iter()
                    .find(|pool| pool.name == definition.name)
                    .map(|pool| pool.driver.clone())
            })
            .unwrap_or_default();
        self.pool_safety_manager = Some(PoolSafetyManager::new(definition, resources, driver));
        self.model.last_error = None;
    }

    fn handle_pool_safety_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_resize = self.supports_extension("storage_pool_loop_resize");
        let manager = self
            .pool_safety_manager
            .as_mut()
            .expect("pool safety manager checked before input");
        if let Some(input) = manager.confirmation.as_mut() {
            let expected = format!(
                "grow storage pool {} to {}",
                manager.definition.name, manager.new_size
            );
            return match key.code {
                KeyCode::Esc => {
                    manager.confirmation = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => match manager.growth_value() {
                    Ok(value) => {
                        manager.confirmation = None;
                        AppCommand::UpdateResource {
                            definition: manager.definition.clone(),
                            value,
                        }
                    }
                    Err(error) => {
                        self.model.last_error = Some(error);
                        AppCommand::None
                    }
                },
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.pool_safety_manager = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                manager.tab = if manager.tab == PoolSafetyTab::Growth {
                    PoolSafetyTab::Health
                } else {
                    PoolSafetyTab::Growth
                };
                AppCommand::None
            }
            KeyCode::Backspace if manager.tab == PoolSafetyTab::Growth => {
                manager.new_size.pop();
                AppCommand::None
            }
            KeyCode::Char('g') if manager.tab == PoolSafetyTab::Growth && can_resize => {
                match manager.growth_value() {
                    Ok(_) => manager.confirmation = Some(String::new()),
                    Err(error) => self.model.last_error = Some(error),
                }
                AppCommand::None
            }
            KeyCode::Char('g') if manager.tab == PoolSafetyTab::Growth => {
                self.model.last_error =
                    Some("pool growth unavailable: server lacks storage_pool_loop_resize".into());
                AppCommand::None
            }
            KeyCode::Char(c)
                if manager.tab == PoolSafetyTab::Growth
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                manager.new_size.push(c);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    pub fn load_qmp_manager(&mut self, definition: ResourceDefinition) {
        self.qmp_manager = Some(QmpManager::new(definition));
        self.model.last_error = None;
    }

    fn handle_qmp_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .qmp_manager
            .as_mut()
            .expect("QMP manager checked before input");
        if manager.confirmation.is_some() {
            let expected = format!(
                "apply QMP stage {} to VM {}",
                manager.stage, manager.definition.name
            );
            let input = manager.confirmation.as_mut().expect("QMP confirmation");
            return match key.code {
                KeyCode::Esc => {
                    manager.confirmation = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    manager.confirmation = None;
                    AppCommand::UpdateResource {
                        definition: manager.definition.clone(),
                        value: manager.value(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.qmp_manager = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter => {
                manager.field = (manager.field + 1) % 4;
                AppCommand::None
            }
            KeyCode::BackTab => {
                manager.field = (manager.field + 3) % 4;
                AppCommand::None
            }
            KeyCode::Left if manager.field == 0 => {
                manager.cycle_stage(true);
                AppCommand::None
            }
            KeyCode::Right | KeyCode::Char(' ') if manager.field == 0 => {
                manager.cycle_stage(false);
                AppCommand::None
            }
            KeyCode::Left if manager.field == 1 => {
                manager.cycle_command(true);
                AppCommand::None
            }
            KeyCode::Right | KeyCode::Char(' ') if manager.field == 1 => {
                manager.cycle_command(false);
                AppCommand::None
            }
            KeyCode::Backspace if manager.field == 2 => {
                manager.argument_name.pop();
                AppCommand::None
            }
            KeyCode::Backspace if manager.field == 3 => {
                manager.argument_value.pop();
                AppCommand::None
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Err(error) = manager.add_command() {
                    self.model.last_error = Some(error);
                }
                AppCommand::None
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !manager.commands.is_empty() =>
            {
                manager.commands.remove(manager.selected);
                manager.selected = manager
                    .selected
                    .min(manager.commands.len().saturating_sub(1));
                AppCommand::None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                manager.confirmation = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.commands.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.commands.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.commands.is_empty() => {
                manager.selected =
                    (manager.selected + manager.commands.len() - 1) % manager.commands.len();
                AppCommand::None
            }
            KeyCode::Char(c)
                if manager.field == 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                manager.argument_name.push(c);
                AppCommand::None
            }
            KeyCode::Char(c)
                if manager.field == 3 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                manager.argument_value.push(c);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_bulk_key(&mut self, key: KeyEvent) -> AppCommand {
        if self
            .bulk_manager
            .as_ref()
            .is_some_and(|manager| manager.running)
        {
            return match key.code {
                KeyCode::Char('c') => {
                    if let Some(cancel) = &self.bulk_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    if let Some(manager) = self.bulk_manager.as_mut() {
                        manager.cancelled = true;
                    }
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        let manager = self
            .bulk_manager
            .as_mut()
            .expect("bulk manager checked before input");
        if manager.confirmation.is_some() {
            let expected = format!(
                "apply bulk {} to {} instances",
                manager.action.as_str(),
                manager.eligible_count()
            );
            let input = manager.confirmation.as_mut().expect("bulk confirmation");
            return match key.code {
                KeyCode::Esc => {
                    manager.confirmation = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    manager.confirmation = None;
                    AppCommand::ApplyBulkInstances {
                        targets: manager.selected_targets(),
                        action: manager.action,
                        concurrency: manager.concurrency,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.bulk_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.targets.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.targets.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.targets.is_empty() => {
                manager.selected =
                    (manager.selected + manager.targets.len() - 1) % manager.targets.len();
                AppCommand::None
            }
            KeyCode::Char(' ') if !manager.targets.is_empty() => {
                manager.targets[manager.selected].selected =
                    !manager.targets[manager.selected].selected;
                AppCommand::None
            }
            KeyCode::Char('A') => {
                let all = manager.targets.iter().all(|target| target.selected);
                for target in &mut manager.targets {
                    target.selected = !all;
                }
                AppCommand::None
            }
            KeyCode::Left => {
                manager.cycle_action(true);
                manager.results.clear();
                AppCommand::None
            }
            KeyCode::Right => {
                manager.cycle_action(false);
                manager.results.clear();
                AppCommand::None
            }
            KeyCode::Char('+') => {
                manager.concurrency = (manager.concurrency + 1).min(32);
                AppCommand::None
            }
            KeyCode::Char('-') => {
                manager.concurrency = manager.concurrency.saturating_sub(1).max(1);
                AppCommand::None
            }
            KeyCode::Char('d') => {
                manager.dry_run = !manager.dry_run;
                AppCommand::None
            }
            KeyCode::Char('r') if !manager.failed.is_empty() => {
                for target in &mut manager.targets {
                    target.selected = manager
                        .failed
                        .iter()
                        .any(|(name, project)| name == &target.name && project == &target.project);
                }
                manager.results = vec![format!(
                    "Retry selection: {} failed targets",
                    manager.failed.len()
                )];
                AppCommand::None
            }
            KeyCode::Enter if manager.selected_count() == 0 => {
                self.model.last_error = Some("select at least one bulk target".into());
                AppCommand::None
            }
            KeyCode::Enter if manager.dry_run => {
                manager.results = manager
                    .targets
                    .iter()
                    .filter(|target| target.selected)
                    .map(|target| {
                        if action_is_allowed(&target.status, manager.action) {
                            format!(
                                "PASS {}:{} {} is eligible",
                                target.project, target.name, target.status
                            )
                        } else {
                            format!(
                                "SKIP {}:{} action {} invalid while {}",
                                target.project,
                                target.name,
                                manager.action.as_str(),
                                target.status
                            )
                        }
                    })
                    .collect();
                AppCommand::None
            }
            KeyCode::Enter if manager.eligible_count() == 0 => {
                self.model.last_error = Some("none of the selected instances are eligible".into());
                AppCommand::None
            }
            KeyCode::Enter => {
                manager.confirmation = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    pub fn start_bulk_instances(
        &mut self,
        client: &IncusClient,
        targets: Vec<BulkTarget>,
        action: InstanceAction,
        concurrency: usize,
    ) {
        use futures_util::{StreamExt, stream};
        let client = client.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(manager) = self.bulk_manager.as_mut() {
            manager.results.clear();
            manager.failed.clear();
            manager.running = true;
            manager.completed = 0;
            manager.total = targets.len();
            manager.cancelled = false;
        }
        self.bulk_progress = Some(receiver);
        self.bulk_cancel = Some(cancel.clone());
        tokio::spawn(async move {
            stream::iter(targets.into_iter().map(|target| {
                let client = client.clone();
                let cancel = cancel.clone();
                async move {
                    if cancel.load(Ordering::Relaxed) {
                        return (target, Err("cancelled before dispatch".into()));
                    }
                    if !action_is_allowed(&target.status, action) {
                        return (target, Ok(None));
                    }
                    let result = if action == InstanceAction::Delete {
                        client.delete_instance(&target.name, &target.project).await
                    } else {
                        client
                            .change_instance_state(&target.name, &target.project, action)
                            .await
                    };
                    (target, result.map(Some).map_err(|error| error.to_string()))
                }
            }))
            .buffer_unordered(concurrency.clamp(1, 32))
            .for_each(|(target, result)| {
                let sender = sender.clone();
                async move {
                    let _ = sender.send(BulkProgress::Outcome {
                        target,
                        result: Box::new(result),
                    });
                }
            })
            .await;
            let _ = sender.send(BulkProgress::Finished);
        });
    }

    pub fn poll_bulk_progress(&mut self) -> bool {
        let mut finished = false;
        let Some(receiver) = self.bulk_progress.as_mut() else {
            return false;
        };
        while let Ok(progress) = receiver.try_recv() {
            match progress {
                BulkProgress::Outcome { target, result } => {
                    if let Some(manager) = self.bulk_manager.as_mut() {
                        manager.completed += 1;
                        match *result {
                            Ok(Some(operation)) => {
                                manager.results.push(format!(
                                    "PASS {}:{} operation {}",
                                    target.project, target.name, operation.id
                                ));
                                self.model.last_operation = Some(operation);
                            }
                            Ok(None) => manager.results.push(format!(
                                "SKIP {}:{} invalid while {}",
                                target.project, target.name, target.status
                            )),
                            Err(error) if error == "cancelled before dispatch" => {
                                manager.cancelled = true;
                                manager.results.push(format!(
                                    "CANCEL {}:{} not dispatched",
                                    target.project, target.name
                                ));
                            }
                            Err(error) => {
                                manager.results.push(format!(
                                    "FAIL {}:{} {error}",
                                    target.project, target.name
                                ));
                                manager.failed.push((target.name, target.project));
                            }
                        }
                    }
                }
                BulkProgress::Finished => {
                    finished = true;
                    if let Some(manager) = self.bulk_manager.as_mut() {
                        manager.running = false;
                    }
                }
            }
        }
        if finished {
            self.bulk_progress = None;
            self.bulk_cancel = None;
        }
        finished
    }

    fn handle_logging_target_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .logging_target_manager
            .as_mut()
            .expect("logging target manager checked before input");
        if let Some(input) = manager.delete_input.as_mut() {
            let name = manager
                .targets
                .get(manager.selected)
                .cloned()
                .unwrap_or_default();
            let expected = format!("delete logging target {name}");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let mut value = manager.definition.value.clone();
                    if let Some(config) = value.as_object_mut() {
                        let prefix = format!("logging.{name}.");
                        config.retain(|key, _| !key.starts_with(&prefix));
                    }
                    manager.delete_input = None;
                    AppCommand::UpdateResource {
                        definition: manager.definition.clone(),
                        value,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field + 1 < form.values.len() => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + form.values.len() - 1) % form.values.len();
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    if matches!(form.field, 1 | 11) =>
                {
                    let choices: &[&str] = if form.field == 1 && manager.supports_webhook {
                        &["loki", "syslog", "webhook"]
                    } else if form.field == 1 {
                        &["loki", "syslog"]
                    } else {
                        &["trace", "debug", "info", "warning", "error"]
                    };
                    let current = choices
                        .iter()
                        .position(|choice| *choice == form.values[form.field])
                        .unwrap_or(0);
                    let backwards = key.code == KeyCode::Left;
                    let next = if backwards {
                        (current + choices.len() - 1) % choices.len()
                    } else {
                        (current + 1) % choices.len()
                    };
                    form.values[form.field] = choices[next].to_owned();
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    form.values[form.field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.values[form.field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter if form.values[0].trim().is_empty() => {
                    self.model.last_error = Some("logging target name is required".into());
                    AppCommand::None
                }
                KeyCode::Enter if form.values[1].trim().is_empty() => {
                    self.model.last_error = Some("logging target type is required".into());
                    AppCommand::None
                }
                KeyCode::Enter if form.values[2].trim().is_empty() => {
                    self.model.last_error = Some("logging target address is required".into());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let saved = form.clone();
                    let definition = manager.definition.clone();
                    AppCommand::UpdateResource {
                        value: LoggingTargetManager::value_for(&definition, &saved),
                        definition,
                    }
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.logging_target_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.targets.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.targets.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.targets.is_empty() => {
                manager.selected =
                    (manager.selected + manager.targets.len() - 1) % manager.targets.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(LoggingTargetForm::new(manager, None));
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.targets.is_empty() => {
                let name = manager.targets[manager.selected].clone();
                manager.editor = Some(LoggingTargetForm::new(manager, Some(&name)));
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.targets.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_scriptlet_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .scriptlet_manager
            .as_mut()
            .expect("scriptlet manager checked before input");
        if let Some(input) = manager.activation_input.as_mut() {
            let action = if manager.rollback {
                "rollback"
            } else {
                "activate"
            };
            let expected = format!("{action} {} scriptlet", manager.kind.label());
            return match key.code {
                KeyCode::Esc => {
                    manager.activation_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let source = if manager.rollback {
                        manager.original.clone()
                    } else {
                        manager.draft.clone()
                    };
                    manager.activation_input = None;
                    AppCommand::UpdateScriptlet {
                        definition: manager.definition.clone(),
                        value: manager.value(&source),
                        activated_source: source,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.scriptlet_manager = None;
                AppCommand::None
            }
            KeyCode::Tab => {
                manager.switch_kind();
                AppCommand::None
            }
            KeyCode::F(2) => {
                manager.validate();
                AppCommand::None
            }
            KeyCode::F(3) => {
                manager.template();
                AppCommand::None
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if manager.validate() {
                    manager.rollback = false;
                    manager.activation_input = Some(String::new());
                }
                AppCommand::None
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                manager.rollback = true;
                manager.activation_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Left => {
                if manager.cursor > 0 {
                    manager.cursor = manager.draft[..manager.cursor]
                        .char_indices()
                        .last()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                }
                AppCommand::None
            }
            KeyCode::Right => {
                if manager.cursor < manager.draft.len() {
                    manager.cursor += manager.draft[manager.cursor..]
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or(0);
                }
                AppCommand::None
            }
            KeyCode::Backspace => {
                if manager.cursor > 0 {
                    let previous = manager.draft[..manager.cursor]
                        .char_indices()
                        .last()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    manager.draft.replace_range(previous..manager.cursor, "");
                    manager.cursor = previous;
                    manager.validation.clear();
                }
                AppCommand::None
            }
            KeyCode::Enter => {
                manager.draft.insert(manager.cursor, '\n');
                manager.cursor += 1;
                manager.validation.clear();
                AppCommand::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                manager.draft.insert(manager.cursor, c);
                manager.cursor += c.len_utf8();
                manager.validation.clear();
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_preseed_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .preseed_manager
            .as_mut()
            .expect("preseed manager checked before input");
        if let Some((import, path)) = manager.transfer.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.transfer = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    path.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if path.trim().is_empty() => {
                    self.model.last_error = Some("preseed file path is required".into());
                    AppCommand::None
                }
                KeyCode::Enter if *import => AppCommand::ImportPreseed {
                    path: path.trim().to_owned(),
                },
                KeyCode::Enter => AppCommand::ExportPreseed {
                    path: path.trim().to_owned(),
                    spec: manager.spec.clone(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.apply_input.as_mut() {
            let expected = "apply typed preseed";
            return match key.code {
                KeyCode::Esc => {
                    manager.apply_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if input == expected => {
                    manager.apply_input = None;
                    AppCommand::ApplyPreseed(manager.spec.clone())
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.preseed_manager = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter => {
                manager.field = (manager.field + 1) % PreseedManager::FIELD_COUNT;
                AppCommand::None
            }
            KeyCode::BackTab => {
                manager.field =
                    (manager.field + PreseedManager::FIELD_COUNT - 1) % PreseedManager::FIELD_COUNT;
                AppCommand::None
            }
            KeyCode::Left => {
                manager.cycle(true);
                AppCommand::None
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                manager.cycle(false);
                AppCommand::None
            }
            KeyCode::Backspace => {
                if let Some(value) = manager.text_mut() {
                    value.pop();
                }
                AppCommand::None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match manager.stage_current() {
                    Ok(message) => manager.results = vec![format!("PASS: {message}")],
                    Err(error) => self.model.last_error = Some(error),
                }
                AppCommand::None
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match manager.remove_last_staged() {
                    Ok(message) => manager.results = vec![format!("PASS: {message}")],
                    Err(error) => self.model.last_error = Some(error),
                }
                AppCommand::None
            }
            KeyCode::Char('v') => {
                match manager.validate() {
                    Ok(()) => manager.preview = !manager.preview,
                    Err(error) => self.model.last_error = Some(error),
                }
                AppCommand::None
            }
            KeyCode::Char('i') => {
                manager.transfer = Some((true, "./virtty-preseed.json".into()));
                AppCommand::None
            }
            KeyCode::Char('o') => {
                manager.transfer = Some((false, "./virtty-preseed.json".into()));
                AppCommand::None
            }
            KeyCode::Char('a') => {
                match manager.validate() {
                    Ok(()) => manager.apply_input = Some(String::new()),
                    Err(error) => self.model.last_error = Some(error),
                }
                AppCommand::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(value) = manager.text_mut() {
                    value.push(c);
                }
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_settings_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_metadata = self.supports_extension("metadata_configuration");
        let can_server_logging = self.supports_extension("server_logging");
        let can_scriptlets = self.supports_extension("instances_placement_scriptlet")
            || self.supports_extension("authorization_scriptlet");
        let extensions = self
            .model
            .server
            .as_ref()
            .map(|server| server.api_extensions.clone())
            .unwrap_or_default();
        let counts = [
            self.model.warnings.len(),
            self.model.certificates.len(),
            self.model.cluster_members.len(),
        ];
        let manager = self
            .settings_manager
            .as_mut()
            .expect("settings manager checked before handling key");
        if let Some(filter) = manager.capability_filter.as_mut() {
            let needle = filter.to_ascii_lowercase();
            let visible = extensions
                .iter()
                .filter(|extension| extension.to_ascii_lowercase().contains(&needle))
                .count();
            return match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    manager.capability_filter = None;
                    manager.capability_selected = 0;
                    AppCommand::None
                }
                KeyCode::Down if visible > 0 => {
                    manager.capability_selected = (manager.capability_selected + 1) % visible;
                    AppCommand::None
                }
                KeyCode::Up if visible > 0 => {
                    manager.capability_selected =
                        (manager.capability_selected + visible - 1) % visible;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    filter.pop();
                    manager.capability_selected = 0;
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    filter.push(c);
                    manager.capability_selected = 0;
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.cluster_detail.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.cluster_detail = None;
            }
            return AppCommand::None;
        }
        if let Some(input) = manager.cluster_rename_input.as_mut() {
            let current = self
                .model
                .cluster_members
                .get(manager.selected)
                .map(|m| m.server_name.clone())
                .unwrap_or_default();
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !input.trim().is_empty() && input.trim() != current => {
                    AppCommand::RenameClusterMember {
                        name: current,
                        new_name: input.trim().into(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some("new member name must be non-empty and different".into());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.cluster_config.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_config = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 6 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 6) % 7;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 6 => {
                    form.enabled = !form.enabled;
                    AppCommand::None
                }
                KeyCode::Backspace if form.field < 6 => {
                    form.fields[form.field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if form.field < 6 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    form.fields[form.field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ConfigureCluster {
                    value: serde_json::json!({
                        "enabled": form.enabled, "server_name": form.fields[0].trim(), "server_address": form.fields[1].trim(),
                        "cluster_address": form.fields[2].trim(), "cluster_token": form.fields[3].trim(),
                        "cluster_certificate": form.fields[4], "cluster_password": form.fields[5],
                    }),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.cluster_certificate.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_certificate = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter if form.field == 0 => {
                    form.field = 1;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if form.field == 0 {
                        form.certificate_path.pop();
                    } else {
                        form.key_path.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if form.field == 0 {
                        form.certificate_path.push(c);
                    } else {
                        form.key_path.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Enter
                    if !form.certificate_path.trim().is_empty()
                        && !form.key_path.trim().is_empty() =>
                {
                    AppCommand::UpdateClusterCertificate {
                        certificate_path: form.certificate_path.trim().into(),
                        key_path: form.key_path.trim().into(),
                    }
                }
                _ => AppCommand::None,
            };
        }
        if manager.trust_token.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.trust_token = None;
            }
            return AppCommand::None;
        }
        if let Some(name) = manager.cluster_join_name.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_join_name = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    name.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    name.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if name.trim().is_empty() => {
                    self.model.last_error = Some("new cluster member name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::GenerateClusterJoinToken {
                    name: name.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.certificate_form.as_mut() {
            let last = if form.mode == "certificate-file" {
                4
            } else {
                3
            };
            return match key.code {
                KeyCode::Esc => {
                    manager.certificate_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < last => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + last) % (last + 1);
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 0 => {
                    form.mode = if form.mode == "trust-token" {
                        "certificate-file"
                    } else {
                        "trust-token"
                    }
                    .to_owned();
                    if form.mode == "trust-token" && form.field > 3 {
                        form.field = 3;
                    }
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => {
                    form.restricted = !form.restricted;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match form.field {
                        1 => {
                            form.name.pop();
                        }
                        3 => {
                            form.projects.pop();
                        }
                        4 => {
                            form.certificate_path.pop();
                        }
                        _ => {}
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match form.field {
                        1 => form.name.push(c),
                        3 => form.projects.push(c),
                        4 => form.certificate_path.push(c),
                        _ => {}
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.name.trim().is_empty() => {
                    self.model.last_error = Some("certificate name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter
                    if form.mode == "certificate-file"
                        && form.certificate_path.trim().is_empty() =>
                {
                    self.model.last_error = Some("PEM certificate path is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let projects = form
                        .projects
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToOwned::to_owned)
                        .collect();
                    if form.mode == "trust-token" {
                        AppCommand::GenerateCertificateToken {
                            name: form.name.trim().to_owned(),
                            restricted: form.restricted,
                            projects,
                        }
                    } else {
                        AppCommand::AddCertificateFromFile {
                            name: form.name.trim().to_owned(),
                            restricted: form.restricted,
                            projects,
                            path: form.certificate_path.trim().to_owned(),
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.certificate_edit.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.certificate_edit = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 3 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 1 => {
                    form.restricted = !form.restricted;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if form.field == 0 {
                        form.name.pop();
                    } else if form.field == 2 {
                        form.projects.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if form.field == 0 {
                        form.name.push(c);
                    } else if form.field == 2 {
                        form.projects.push(c);
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.name.trim().is_empty() => {
                    self.model.last_error = Some("certificate name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::UpdateCertificate {
                    fingerprint: form.fingerprint.clone(),
                    name: form.name.trim().to_owned(),
                    restricted: form.restricted,
                    projects: form
                        .projects
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToOwned::to_owned)
                        .collect(),
                },
                _ => AppCommand::None,
            };
        }
        let tab_index = match manager.tab {
            SettingsTab::Warnings => 0,
            SettingsTab::Certificates => 1,
            SettingsTab::Cluster => 2,
        };
        let selected_id = match manager.tab {
            SettingsTab::Warnings => self
                .model
                .warnings
                .get(manager.selected)
                .map(|warning| warning.uuid.clone()),
            SettingsTab::Certificates => self
                .model
                .certificates
                .get(manager.selected)
                .map(|certificate| certificate.fingerprint.clone()),
            SettingsTab::Cluster => None,
        };
        let selected_member = self.model.cluster_members.get(manager.selected).cloned();
        let selected_certificate = self.model.certificates.get(manager.selected).cloned();

        if let Some((action, input)) = manager.cluster_action.as_mut() {
            let member = selected_member
                .as_ref()
                .map(|m| m.server_name.as_str())
                .unwrap_or_default();
            let expected = format!("{action} cluster member {member}");
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_action = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let action = action.clone();
                    manager.cluster_action = None;
                    if action == "delete" {
                        AppCommand::DeleteClusterMember {
                            name: member.to_owned(),
                        }
                    } else {
                        AppCommand::ChangeClusterMemberState {
                            name: member.to_owned(),
                            action,
                        }
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.cluster_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.cluster_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 2 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match form.field {
                        0 => {
                            form.description.pop();
                        }
                        1 => {
                            form.roles.pop();
                        }
                        2 => {
                            form.groups.pop();
                        }
                        _ => {
                            form.failure_domain.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match form.field {
                        0 => form.description.push(c),
                        1 => form.roles.push(c),
                        2 => form.groups.push(c),
                        _ => form.failure_domain.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let split = |value: &str| {
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(ToOwned::to_owned)
                            .collect()
                    };
                    selected_member
                        .as_ref()
                        .map(|member| AppCommand::UpdateClusterMember {
                            name: member.server_name.clone(),
                            description: form.description.trim().to_owned(),
                            roles: split(&form.roles),
                            groups: split(&form.groups),
                            failure_domain: form.failure_domain.trim().into(),
                            config: member.config.clone(),
                        })
                        .unwrap_or(AppCommand::None)
                }
                _ => AppCommand::None,
            };
        }

        if manager.delete_input.is_some() {
            let kind = if manager.tab == SettingsTab::Warnings {
                "warning"
            } else {
                "certificate"
            };
            let expected = selected_id
                .as_ref()
                .map(|id| format!("delete {kind} {id}"))
                .unwrap_or_default();
            let input = manager
                .delete_input
                .as_mut()
                .expect("settings confirmation remains present");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    manager.delete_input = None;
                    match manager.tab {
                        SettingsTab::Warnings => AppCommand::DeleteWarning {
                            uuid: selected_id.unwrap_or_default(),
                        },
                        SettingsTab::Certificates => AppCommand::DeleteCertificate {
                            fingerprint: selected_id.unwrap_or_default(),
                        },
                        SettingsTab::Cluster => AppCommand::None,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.settings_manager = None;
                AppCommand::None
            }
            KeyCode::Char('Q') => {
                manager.capability_filter = Some(String::new());
                manager.capability_selected = 0;
                AppCommand::None
            }
            KeyCode::Char('M') if can_metadata => AppCommand::ReadMetadataConfiguration,
            KeyCode::Char('M') => {
                self.model.last_error =
                    Some("metadata schema unavailable: server lacks metadata_configuration".into());
                AppCommand::None
            }
            KeyCode::Char('L') if can_server_logging => AppCommand::ReadLoggingTargets,
            KeyCode::Char('L') => {
                self.model.last_error =
                    Some("remote logging unavailable: server lacks server_logging".into());
                AppCommand::None
            }
            KeyCode::Char('T') if can_scriptlets => AppCommand::ReadScriptlets,
            KeyCode::Char('T') => {
                self.model.last_error = Some(
                    "scriptlets unavailable: server lacks placement and authorization scriptlet extensions"
                        .into(),
                );
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Right => {
                manager.tab = match manager.tab {
                    SettingsTab::Warnings => SettingsTab::Certificates,
                    SettingsTab::Certificates => SettingsTab::Cluster,
                    SettingsTab::Cluster => SettingsTab::Warnings,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::BackTab | KeyCode::Left => {
                manager.tab = match manager.tab {
                    SettingsTab::Warnings => SettingsTab::Cluster,
                    SettingsTab::Certificates => SettingsTab::Warnings,
                    SettingsTab::Cluster => SettingsTab::Certificates,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if counts[tab_index] > 0 => {
                manager.selected = (manager.selected + 1) % counts[tab_index];
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if counts[tab_index] > 0 => {
                manager.selected = (manager.selected + counts[tab_index] - 1) % counts[tab_index];
                AppCommand::None
            }
            KeyCode::Char('a') if manager.tab == SettingsTab::Warnings => selected_id
                .map(|uuid| AppCommand::AcknowledgeWarning { uuid })
                .unwrap_or(AppCommand::None),
            KeyCode::Char('n') if manager.tab == SettingsTab::Certificates => {
                manager.certificate_form = Some(CertificateForm {
                    mode: "trust-token".to_owned(),
                    name: String::new(),
                    restricted: false,
                    projects: String::new(),
                    certificate_path: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == SettingsTab::Cluster => {
                manager.cluster_join_name = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == SettingsTab::Certificates && selected_certificate.is_some() =>
            {
                let certificate = selected_certificate.expect("selected certificate");
                manager.certificate_edit = Some(CertificateEditForm {
                    fingerprint: certificate.fingerprint,
                    name: certificate.name,
                    restricted: certificate.restricted,
                    projects: certificate.projects.join(","),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                let member = selected_member.expect("selected cluster member");
                manager.cluster_form = Some(ClusterMemberForm {
                    description: member.description,
                    roles: member.roles.join(","),
                    groups: member.groups.join(","),
                    failure_domain: member.failure_domain,
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('x')
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                manager.cluster_action = Some(("evacuate".to_owned(), String::new()));
                AppCommand::None
            }
            KeyCode::Char('r')
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                manager.cluster_action = Some(("restore".to_owned(), String::new()));
                AppCommand::None
            }
            KeyCode::Char('R')
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                manager.cluster_rename_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('S')
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                AppCommand::ReadClusterMemberState {
                    name: selected_member.expect("member").server_name,
                }
            }
            KeyCode::Char('I') if manager.tab == SettingsTab::Cluster => {
                AppCommand::ReadClusterConfiguration
            }
            KeyCode::Char('C') if manager.tab == SettingsTab::Cluster => {
                manager.cluster_certificate = Some(ClusterCertificateForm {
                    certificate_path: String::new(),
                    key_path: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if manager.tab != SettingsTab::Cluster && selected_id.is_some() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('d')
                if manager.tab == SettingsTab::Cluster && selected_member.is_some() =>
            {
                manager.cluster_action = Some(("delete".to_owned(), String::new()));
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_device_manager(&mut self) -> AppCommand {
        let Some(instance) = self.model.instances.get(self.selected_instance) else {
            return AppCommand::None;
        };
        AppCommand::ReadDeviceManager {
            kind: ResourceKind::Instance,
            name: instance.name.clone(),
            project: instance.project.clone(),
        }
    }

    fn open_profile_device_manager(&mut self) -> AppCommand {
        let Some(profile) = self.model.profiles.get(self.selected_profile) else {
            return AppCommand::None;
        };
        AppCommand::ReadDeviceManager {
            kind: ResourceKind::Profile,
            name: profile.name.clone(),
            project: profile.project.clone(),
        }
    }

    fn handle_device_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .device_manager
            .as_mut()
            .expect("device manager checked before handling key");

        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase().unwrap_or_default();
            let input = manager
                .delete_input
                .as_mut()
                .expect("device confirmation remains present");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    manager.delete_input = None;
                    if manager.selected < manager.devices.len() {
                        manager.devices.remove(manager.selected);
                    }
                    manager.selected = manager
                        .selected
                        .min(manager.devices.len().saturating_sub(1));
                    let definition = manager.definition_with_devices();
                    AppCommand::UpdateResource {
                        value: definition.value.clone(),
                        definition,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        if let Some(editor) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if editor.field + 1 < editor.fields.len() => {
                    editor.field = (editor.field + 1) % editor.fields.len();
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    editor.field = (editor.field + editor.fields.len() - 1) % editor.fields.len();
                    AppCommand::None
                }
                KeyCode::Left => {
                    editor.cycle(true);
                    AppCommand::None
                }
                KeyCode::Right | KeyCode::Char(' ') => {
                    editor.cycle(false);
                    AppCommand::None
                }
                KeyCode::Backspace if editor.fields[editor.field].choices.is_empty() => {
                    editor.fields[editor.field].value.pop();
                    AppCommand::None
                }
                KeyCode::Char(character)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && editor.fields[editor.field].choices.is_empty() =>
                {
                    editor.fields[editor.field].value.push(character);
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let (name, config) = editor.device();
                    if name.is_empty() {
                        self.model.last_error = Some("device name is required".to_owned());
                        return AppCommand::None;
                    }
                    if !config.contains_key("type") {
                        self.model.last_error = Some("device type is required".to_owned());
                        return AppCommand::None;
                    }
                    if let Some(original) = &editor.original_name {
                        manager.devices.retain(|(existing, _)| existing != original);
                    }
                    manager.devices.retain(|(existing, _)| existing != &name);
                    manager.devices.push((name, config));
                    manager.devices.sort_by(|left, right| left.0.cmp(&right.0));
                    manager.editor = None;
                    let definition = manager.definition_with_devices();
                    AppCommand::UpdateResource {
                        value: definition.value.clone(),
                        definition,
                    }
                }
                _ => AppCommand::None,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.device_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.devices.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.devices.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.devices.is_empty() => {
                manager.selected =
                    (manager.selected + manager.devices.len() - 1) % manager.devices.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(DeviceForm::new(None, None));
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.devices.is_empty() => {
                let (name, config) = &manager.devices[manager.selected];
                manager.editor = Some(DeviceForm::new(Some(name.clone()), Some(config)));
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.devices.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_bucket_manager(&mut self) -> AppCommand {
        let Some(pool) = self.model.storage_pools.get(self.selected_storage_pool) else {
            return AppCommand::None;
        };
        let pool = pool.name.clone();
        self.bucket_manager = Some(BucketManager::new(pool.clone()));
        AppCommand::ReadStorageBuckets { pool }
    }

    fn handle_bucket_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self.bucket_manager.as_mut().expect("bucket manager");
        if let Some(form) = manager.backups.as_mut() {
            let selected = form.selected_name().unwrap_or_default().to_owned();
            if let Some((rename, input)) = form.name_input.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.name_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if !*rename => {
                        form.compression = match form.compression.as_str() {
                            "gzip" => "zstd",
                            "zstd" => "none",
                            _ => "gzip",
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if input.trim().is_empty() => {
                        self.model.last_error = Some("backup name is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let command = if *rename {
                            AppCommand::RenameStorageBucketBackup {
                                pool: manager.pool.clone(),
                                project: form.bucket.project.clone(),
                                bucket: form.bucket.name.clone(),
                                backup: selected,
                                new_name: input.trim().to_owned(),
                            }
                        } else {
                            AppCommand::CreateStorageBucketBackup {
                                pool: manager.pool.clone(),
                                project: form.bucket.project.clone(),
                                bucket: form.bucket.name.clone(),
                                name: input.trim().to_owned(),
                                compression: form.compression.clone(),
                            }
                        };
                        form.name_input = None;
                        command
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(input) = form.delete_input.as_mut() {
                let expected = format!("delete bucket backup {selected}");
                return match key.code {
                    KeyCode::Esc => {
                        form.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        form.delete_input = None;
                        AppCommand::DeleteStorageBucketBackup {
                            pool: manager.pool.clone(),
                            project: form.bucket.project.clone(),
                            bucket: form.bucket.name.clone(),
                            backup: selected,
                        }
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(path) = form.export_path.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.export_path = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        path.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        path.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if path.trim().is_empty() => {
                        self.model.last_error = Some("export path is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => AppCommand::ExportStorageBucketBackup {
                        pool: manager.pool.clone(),
                        project: form.bucket.project.clone(),
                        bucket: form.bucket.name.clone(),
                        backup: selected,
                        path: path.trim().to_owned(),
                    },
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.backups = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !form.backups.is_empty() => {
                    form.selected = (form.selected + 1) % form.backups.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !form.backups.is_empty() => {
                    form.selected = (form.selected + form.backups.len() - 1) % form.backups.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    form.name_input = Some((false, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('r') if !form.backups.is_empty() => {
                    form.name_input = Some((true, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('o') if !form.backups.is_empty() => {
                    form.export_path = Some(format!("./{selected}.tar.gz"));
                    AppCommand::None
                }
                KeyCode::Char('d') if !form.backups.is_empty() => {
                    form.delete_input = Some(String::new());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(keys) = manager.keys.as_mut() {
            if keys.credentials.is_some() {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    keys.credentials = None;
                }
                return AppCommand::None;
            }
            if let Some(input) = keys.delete_input.as_mut() {
                let expected = format!(
                    "delete bucket key {}",
                    keys.keys
                        .get(keys.selected)
                        .map(|k| k.name.as_str())
                        .unwrap_or_default()
                );
                return match key.code {
                    KeyCode::Esc => {
                        keys.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        let name = keys
                            .keys
                            .get(keys.selected)
                            .map(|k| k.name.clone())
                            .unwrap_or_default();
                        keys.delete_input = None;
                        AppCommand::DeleteStorageBucketKey {
                            pool: manager.pool.clone(),
                            project: keys.bucket.project.clone(),
                            bucket: keys.bucket.name.clone(),
                            name,
                        }
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(editor) = keys.editor.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        keys.editor = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if editor.field < 2 => {
                        editor.field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        editor.field = (editor.field + 2) % 3;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if editor.field == 2 => {
                        editor.role = if editor.role == "admin" {
                            "read-only"
                        } else {
                            "admin"
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Backspace if editor.field < 2 => {
                        if editor.field == 0 {
                            editor.name.pop();
                        } else {
                            editor.description.pop();
                        }
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if editor.field < 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if editor.field == 0 {
                            editor.name.push(c);
                        } else {
                            editor.description.push(c);
                        }
                        AppCommand::None
                    }
                    KeyCode::Enter if editor.name.trim().is_empty() => {
                        self.model.last_error = Some("key name is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        if let Some(name) = &editor.original_name {
                            AppCommand::UpdateStorageBucketKey {
                                pool: manager.pool.clone(),
                                project: keys.bucket.project.clone(),
                                bucket: keys.bucket.name.clone(),
                                name: name.clone(),
                                description: editor.description.trim().to_owned(),
                                role: editor.role.clone(),
                            }
                        } else {
                            AppCommand::CreateStorageBucketKey {
                                pool: manager.pool.clone(),
                                project: keys.bucket.project.clone(),
                                bucket: keys.bucket.name.clone(),
                                name: editor.name.trim().to_owned(),
                                description: editor.description.trim().to_owned(),
                                role: editor.role.clone(),
                            }
                        }
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.keys = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !keys.keys.is_empty() => {
                    keys.selected = (keys.selected + 1) % keys.keys.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !keys.keys.is_empty() => {
                    keys.selected = (keys.selected + keys.keys.len() - 1) % keys.keys.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    keys.editor = Some(BucketKeyEditor {
                        original_name: None,
                        name: String::new(),
                        description: String::new(),
                        role: "admin".to_owned(),
                        field: 0,
                    });
                    AppCommand::None
                }
                KeyCode::Char('e') | KeyCode::Enter if !keys.keys.is_empty() => {
                    let key = keys.keys[keys.selected].clone();
                    keys.editor = Some(BucketKeyEditor {
                        original_name: Some(key.name.clone()),
                        name: key.name,
                        description: key.description,
                        role: key.role,
                        field: 0,
                    });
                    AppCommand::None
                }
                KeyCode::Char('d') if !keys.keys.is_empty() => {
                    keys.delete_input = Some(String::new());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.delete_input.as_mut() {
            let expected = format!(
                "delete bucket {}",
                manager
                    .buckets
                    .get(manager.selected)
                    .map(|b| b.name.as_str())
                    .unwrap_or_default()
            );
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let bucket = manager.buckets.get(manager.selected).cloned();
                    manager.delete_input = None;
                    bucket
                        .map(|b| AppCommand::DeleteStorageBucket {
                            pool: manager.pool.clone(),
                            project: b.project,
                            name: b.name,
                        })
                        .unwrap_or(AppCommand::None)
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(editor) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if editor.field < 3 => {
                    editor.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    editor.field = (editor.field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match editor.field {
                        0 => {
                            editor.name.pop();
                        }
                        1 => {
                            editor.project.pop();
                        }
                        2 => {
                            editor.description.pop();
                        }
                        _ => {
                            editor.size.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match editor.field {
                        0 => editor.name.push(c),
                        1 => editor.project.push(c),
                        2 => editor.description.push(c),
                        _ => editor.size.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter if editor.name.trim().is_empty() => {
                    self.model.last_error = Some("bucket name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter if editor.editing => {
                    let bucket = &manager.buckets[manager.selected];
                    AppCommand::UpdateStorageBucket {
                        pool: manager.pool.clone(),
                        project: bucket.project.clone(),
                        name: bucket.name.clone(),
                        description: editor.description.trim().to_owned(),
                        size: editor.size.trim().to_owned(),
                        config: bucket.config.clone(),
                    }
                }
                KeyCode::Enter => AppCommand::CreateStorageBucket {
                    pool: manager.pool.clone(),
                    project: editor.project.trim().to_owned(),
                    name: editor.name.trim().to_owned(),
                    description: editor.description.trim().to_owned(),
                    size: editor.size.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.import_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.import_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 2 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match form.field {
                        0 => {
                            form.path.pop();
                        }
                        1 => {
                            form.project.pop();
                        }
                        _ => {
                            form.name.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match form.field {
                        0 => form.path.push(c),
                        1 => form.project.push(c),
                        _ => form.name.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.path.trim().is_empty() || form.name.trim().is_empty() => {
                    self.model.last_error =
                        Some("archive path and bucket name are required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ImportStorageBucket {
                    pool: manager.pool.clone(),
                    project: form.project.trim().to_owned(),
                    name: form.name.trim().to_owned(),
                    path: form.path.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.bucket_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.buckets.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.buckets.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.buckets.is_empty() => {
                manager.selected =
                    (manager.selected + manager.buckets.len() - 1) % manager.buckets.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(BucketEditor {
                    editing: false,
                    name: String::new(),
                    project: "default".to_owned(),
                    description: String::new(),
                    size: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('i') => {
                manager.import_form = Some(BucketImportForm {
                    path: "./bucket-backup.tar.gz".to_owned(),
                    project: "default".to_owned(),
                    name: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.buckets.is_empty() => {
                let b = &manager.buckets[manager.selected];
                manager.editor = Some(BucketEditor {
                    editing: true,
                    name: b.name.clone(),
                    project: b.project.clone(),
                    description: b.description.clone(),
                    size: b.config.get("size").cloned().unwrap_or_default(),
                    field: 2,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.buckets.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('K') if !manager.buckets.is_empty() => {
                let b = &manager.buckets[manager.selected];
                AppCommand::ReadStorageBucketKeys {
                    pool: manager.pool.clone(),
                    project: b.project.clone(),
                    bucket: b.name.clone(),
                }
            }
            KeyCode::Char('B') if !manager.buckets.is_empty() => {
                let b = &manager.buckets[manager.selected];
                AppCommand::ReadStorageBucketBackups {
                    pool: manager.pool.clone(),
                    project: b.project.clone(),
                    bucket: b.name.clone(),
                }
            }
            _ => AppCommand::None,
        }
    }

    fn open_volume_manager(&mut self) -> AppCommand {
        let Some(pool) = self.model.storage_pools.get(self.selected_storage_pool) else {
            return AppCommand::None;
        };
        let pool = pool.name.clone();
        self.volume_manager = Some(VolumeManager::new(pool.clone()));
        self.model.last_error = None;
        AppCommand::ReadStorageVolumes { pool }
    }

    fn handle_volume_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_sftp = self.supports_extension("custom_volume_sftp");
        let can_nbd = self.supports_extension("storage_volume_nbd");
        let can_files = self.supports_extension("file_storage_volume");
        let can_state = self.supports_extension("storage_volume_state");
        let can_initial_owner = self.supports_extension("storage_initial_owner");
        let can_shared_block = self.supports_extension("shared_custom_block_volumes");
        let can_live_migration = self.supports_extension("storage_live_migration");
        let can_refresh_exclude =
            self.supports_extension("custom_volume_refresh_exclude_older_snapshots");
        let manager = self
            .volume_manager
            .as_mut()
            .expect("volume manager checked before handling key");
        if let Some(form) = manager.bitmaps.as_mut() {
            if let Some(input) = form.delete_input.as_mut() {
                let bitmap = form
                    .bitmaps
                    .get(form.selected)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                let expected = format!("delete bitmap {bitmap}");
                return match key.code {
                    KeyCode::Esc => {
                        form.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => AppCommand::DeleteStorageVolumeBitmap {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        bitmap,
                    },
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some((name, granularity, persistent, disabled, field)) = form.create.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.create = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if *field < 3 => {
                        *field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        *field = (*field + 3) % 4;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                        *persistent = !*persistent;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 3 => {
                        *disabled = !*disabled;
                        AppCommand::None
                    }
                    KeyCode::Backspace if *field < 2 => {
                        if *field == 0 {
                            name.pop();
                        } else {
                            granularity.pop();
                        }
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if *field == 0 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        name.push(c);
                        AppCommand::None
                    }
                    KeyCode::Char(c) if *field == 1 && c.is_ascii_digit() => {
                        granularity.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if name.trim().is_empty() => {
                        self.model.last_error = Some("bitmap name is required".into());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let Ok(granularity) = granularity.parse::<i64>() else {
                            self.model.last_error =
                                Some("bitmap granularity must be a positive byte count".into());
                            return AppCommand::None;
                        };
                        if granularity <= 0 || granularity & (granularity - 1) != 0 {
                            self.model.last_error =
                                Some("bitmap granularity must be a positive power of two".into());
                            return AppCommand::None;
                        }
                        AppCommand::CreateStorageVolumeBitmap {
                            pool: manager.pool.clone(),
                            project: form.volume.project.clone(),
                            volume: form.volume.name.clone(),
                            spec: StorageVolumeBitmapSpec {
                                name: name.trim().into(),
                                persistent: *persistent,
                                disabled: *disabled,
                                granularity,
                            },
                        }
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.bitmaps = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !form.bitmaps.is_empty() => {
                    form.selected = (form.selected + 1) % form.bitmaps.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !form.bitmaps.is_empty() => {
                    form.selected = (form.selected + form.bitmaps.len() - 1) % form.bitmaps.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    form.create = Some((String::new(), "32768".into(), true, false, 0));
                    AppCommand::None
                }
                KeyCode::Char('d') if !form.bitmaps.is_empty() => {
                    form.delete_input = Some(String::new());
                    AppCommand::None
                }
                KeyCode::Char('r') => AppCommand::ReadStorageVolumeBitmaps {
                    pool: manager.pool.clone(),
                    project: form.volume.project.clone(),
                    volume: form.volume.name.clone(),
                },
                _ => AppCommand::None,
            };
        }
        if manager.detail.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.detail = None;
            }
            return AppCommand::None;
        }
        if let Some(form) = manager.file_form.as_mut() {
            if let Some(input) = form.delete_input.as_mut() {
                let expected = format!("delete volume file {}", form.path);
                return match key.code {
                    KeyCode::Esc => {
                        form.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => AppCommand::DeleteStorageVolumeFile {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                    },
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some((upload, local_path)) = form.transfer.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.transfer = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        local_path.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        local_path.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *upload => AppCommand::PushStorageVolumeFile {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                        local_path: local_path.trim().into(),
                    },
                    KeyCode::Enter => AppCommand::PullStorageVolumeFile {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                        local_path: local_path.trim().into(),
                    },
                    _ => AppCommand::None,
                };
            }
            if form.browsing {
                return match key.code {
                    KeyCode::Esc => {
                        form.browsing = false;
                        AppCommand::None
                    }
                    KeyCode::Char('j') | KeyCode::Down if !form.entries.is_empty() => {
                        form.selected = (form.selected + 1) % form.entries.len();
                        AppCommand::None
                    }
                    KeyCode::Char('k') | KeyCode::Up if !form.entries.is_empty() => {
                        form.selected =
                            (form.selected + form.entries.len() - 1) % form.entries.len();
                        AppCommand::None
                    }
                    KeyCode::Enter if !form.entries.is_empty() => {
                        let child = form.entries[form.selected].clone();
                        form.path = if form.path == "/" {
                            format!("/{child}")
                        } else {
                            format!("{}/{}", form.path.trim_end_matches('/'), child)
                        };
                        form.browsing = false;
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.file_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    form.field = 1 - form.field;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if form.field == 0 {
                        form.path.pop();
                    } else {
                        form.contents.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    AppCommand::WriteStorageVolumeFile {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                        content: form.contents.clone(),
                    }
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    AppCommand::ListStorageVolumeDirectory {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                    }
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    AppCommand::ReadStorageVolumeFile {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        path: form.path.clone(),
                    }
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.transfer = Some((
                        false,
                        format!("./{}", form.path.rsplit('/').next().unwrap_or("file")),
                    ));
                    AppCommand::None
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.transfer = Some((true, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.delete_input = Some(String::new());
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if form.field == 0 {
                        form.path.push(c);
                    } else {
                        form.contents.push(c);
                    }
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.proxy_address.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.proxy_address = None;
            }
            return AppCommand::None;
        }
        if let Some((protocol, port, field)) = manager.protocol_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.protocol_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter if *field == 0 => {
                    *field = 1;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 0 => {
                    *protocol = if protocol == "sftp" {
                        "nbd".into()
                    } else {
                        "sftp".into()
                    };
                    AppCommand::None
                }
                KeyCode::Backspace if *field == 1 => {
                    port.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if *field == 1 && c.is_ascii_digit() => {
                    port.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if protocol == "sftp" && !can_sftp => {
                    self.model.last_error =
                        Some("volume SFTP unavailable: server lacks custom_volume_sftp".into());
                    AppCommand::None
                }
                KeyCode::Enter if protocol == "nbd" && !can_nbd => {
                    self.model.last_error =
                        Some("volume NBD unavailable: server lacks storage_volume_nbd".into());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let volume = manager.volumes[manager.selected].clone();
                    AppCommand::StartStorageVolumeProtocolProxy {
                        pool: manager.pool.clone(),
                        project: volume.project,
                        volume: volume.name,
                        protocol: protocol.clone(),
                        port: port.parse().unwrap_or(0),
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.copy_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.copy_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 6 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 6) % 7;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    if form.field == 0 && !self.model.storage_pools.is_empty() =>
                {
                    let current = self
                        .model
                        .storage_pools
                        .iter()
                        .position(|pool| pool.name == form.target_pool)
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Left {
                        (current + self.model.storage_pools.len() - 1)
                            % self.model.storage_pools.len()
                    } else {
                        (current + 1) % self.model.storage_pools.len()
                    };
                    form.target_pool = self.model.storage_pools[next].name.clone();
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 3 => {
                    form.volume_only = !form.volume_only;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 4 => {
                    form.refresh = !form.refresh;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    if form.field == 5 && form.supports_live =>
                {
                    form.live = !form.live;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    if form.field == 6 && form.supports_refresh_exclude =>
                {
                    form.refresh_exclude_older = !form.refresh_exclude_older;
                    AppCommand::None
                }
                KeyCode::Backspace if form.field < 3 => {
                    match form.field {
                        0 => form.target_pool.pop(),
                        1 => form.target_project.pop(),
                        _ => form.target_name.pop(),
                    };
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if form.field < 3 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    match form.field {
                        0 => form.target_pool.push(c),
                        1 => form.target_project.push(c),
                        _ => form.target_name.push(c),
                    };
                    AppCommand::None
                }
                KeyCode::Enter
                    if form.target_pool.trim().is_empty() || form.target_name.trim().is_empty() =>
                {
                    self.model.last_error = Some("target pool and volume name are required".into());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let source = manager.volumes[manager.selected].clone();
                    AppCommand::CopyStorageVolume {
                        source_pool: manager.pool.clone(),
                        source_project: source.project,
                        source_name: source.name,
                        target_pool: form.target_pool.trim().into(),
                        target_project: form.target_project.trim().into(),
                        target_name: form.target_name.trim().into(),
                        volume_only: form.volume_only,
                        refresh: form.refresh,
                        live: form.live && can_live_migration,
                        refresh_exclude_older: form.refresh
                            && form.refresh_exclude_older
                            && can_refresh_exclude,
                    }
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.backups.as_mut() {
            let selected = form.selected_name().unwrap_or_default().to_owned();
            if let Some((rename, input)) = form.name_input.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.name_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char('v')
                        if !*rename && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        form.create_options.0 = !form.create_options.0;
                        AppCommand::None
                    }
                    KeyCode::Char('o')
                        if !*rename && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        form.create_options.1 = !form.create_options.1;
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if input.trim().is_empty() => {
                        self.model.last_error = Some("backup name is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let value = input.trim().to_owned();
                        let command = if *rename {
                            AppCommand::RenameStorageVolumeBackup {
                                pool: manager.pool.clone(),
                                project: form.volume.project.clone(),
                                volume: form.volume.name.clone(),
                                backup: selected,
                                new_name: value,
                            }
                        } else {
                            AppCommand::CreateStorageVolumeBackup {
                                pool: manager.pool.clone(),
                                project: form.volume.project.clone(),
                                volume: form.volume.name.clone(),
                                name: value,
                                volume_only: form.create_options.0,
                                optimized_storage: form.create_options.1,
                            }
                        };
                        form.name_input = None;
                        command
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(input) = form.delete_input.as_mut() {
                let expected = format!("delete volume backup {selected}");
                return match key.code {
                    KeyCode::Esc => {
                        form.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        form.delete_input = None;
                        AppCommand::DeleteStorageVolumeBackup {
                            pool: manager.pool.clone(),
                            project: form.volume.project.clone(),
                            volume: form.volume.name.clone(),
                            backup: selected,
                        }
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(path) = form.export_path.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.export_path = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        path.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        path.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if path.trim().is_empty() => {
                        self.model.last_error = Some("export path is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => AppCommand::ExportStorageVolumeBackup {
                        pool: manager.pool.clone(),
                        project: form.volume.project.clone(),
                        volume: form.volume.name.clone(),
                        backup: selected,
                        path: path.trim().to_owned(),
                    },
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.backups = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !form.backups.is_empty() => {
                    form.selected = (form.selected + 1) % form.backups.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !form.backups.is_empty() => {
                    form.selected = (form.selected + form.backups.len() - 1) % form.backups.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    form.create_options = (false, false);
                    form.name_input = Some((false, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('r') if !form.backups.is_empty() => {
                    form.name_input = Some((true, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('o') if !form.backups.is_empty() => {
                    form.export_path = Some(format!("./{selected}.tar.gz"));
                    AppCommand::None
                }
                KeyCode::Char('d') if !form.backups.is_empty() => {
                    form.delete_input = Some(String::new());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.snapshots.as_mut() {
            let selected_snapshot = form.selected_name().unwrap_or_default().to_owned();
            if let Some((rename, input)) = form.name_input.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        form.name_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(character);
                        AppCommand::None
                    }
                    KeyCode::Enter if input.trim().is_empty() => {
                        self.model.last_error = Some("snapshot name is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let pool = manager.pool.clone();
                        let project = form.volume.project.clone();
                        let volume = form.volume.name.clone();
                        let value = input.trim().to_owned();
                        let old = selected_snapshot.clone();
                        let rename = *rename;
                        form.name_input = None;
                        if rename {
                            AppCommand::RenameStorageVolumeSnapshot {
                                pool,
                                project,
                                volume,
                                snapshot: old,
                                new_name: value,
                            }
                        } else {
                            AppCommand::CreateStorageVolumeSnapshot {
                                pool,
                                project,
                                volume,
                                snapshot: value,
                            }
                        }
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some((restore, input)) = form.confirmation.as_mut() {
                let expected = format!(
                    "{} volume snapshot {}",
                    if *restore { "restore" } else { "delete" },
                    selected_snapshot
                );
                return match key.code {
                    KeyCode::Esc => {
                        form.confirmation = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(character);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        let pool = manager.pool.clone();
                        let project = form.volume.project.clone();
                        let volume = form.volume.name.clone();
                        let snapshot = selected_snapshot.clone();
                        let restore = *restore;
                        form.confirmation = None;
                        if restore {
                            AppCommand::RestoreStorageVolumeSnapshot {
                                pool,
                                project,
                                volume,
                                snapshot,
                            }
                        } else {
                            AppCommand::DeleteStorageVolumeSnapshot {
                                pool,
                                project,
                                volume,
                                snapshot,
                            }
                        }
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.snapshots = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !form.snapshots.is_empty() => {
                    form.selected = (form.selected + 1) % form.snapshots.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !form.snapshots.is_empty() => {
                    form.selected =
                        (form.selected + form.snapshots.len() - 1) % form.snapshots.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    form.name_input = Some((false, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('R') if form.selected_name().is_some() => {
                    form.name_input = Some((true, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('r') if form.selected_name().is_some() => {
                    form.confirmation = Some((true, String::new()));
                    AppCommand::None
                }
                KeyCode::Char('d') if form.selected_name().is_some() => {
                    form.confirmation = Some((false, String::new()));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase().unwrap_or_default();
            let input = manager
                .delete_input
                .as_mut()
                .expect("volume deletion remains present");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let volume = manager.volumes.get(manager.selected).cloned();
                    manager.delete_input = None;
                    volume
                        .map(|volume| AppCommand::DeleteStorageVolume {
                            pool: manager.pool.clone(),
                            project: volume.project,
                            name: volume.name,
                        })
                        .unwrap_or(AppCommand::None)
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        if manager.rebuild_input.is_some() {
            let expected = manager.rebuild_phrase().unwrap_or_default();
            let input = manager
                .rebuild_input
                .as_mut()
                .expect("volume rebuild confirmation");
            return match key.code {
                KeyCode::Esc => {
                    manager.rebuild_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let volume = manager.volumes.get(manager.selected).cloned();
                    manager.rebuild_input = None;
                    volume
                        .map(|volume| AppCommand::RebuildStorageVolume {
                            pool: manager.pool.clone(),
                            project: volume.project,
                            name: volume.name,
                        })
                        .unwrap_or(AppCommand::None)
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.detach_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.detach_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field == 0 => {
                    form.field = 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = 1 - form.field;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Up if form.field == 0 && !form.instances.is_empty() => {
                    form.instance =
                        (form.instance + form.instances.len() - 1) % form.instances.len();
                    AppCommand::None
                }
                KeyCode::Right | KeyCode::Down | KeyCode::Char(' ')
                    if form.field == 0 && !form.instances.is_empty() =>
                {
                    form.instance = (form.instance + 1) % form.instances.len();
                    AppCommand::None
                }
                KeyCode::Backspace if form.field == 1 => {
                    form.device_name.pop();
                    AppCommand::None
                }
                KeyCode::Char(character)
                    if form.field == 1 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    form.device_name.push(character);
                    AppCommand::None
                }
                KeyCode::Enter
                    if form.instances.is_empty() || form.device_name.trim().is_empty() =>
                {
                    self.model.last_error =
                        Some("instance and device name are required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    match (
                        manager.volumes.get(manager.selected),
                        form.instances.get(form.instance),
                    ) {
                        (Some(volume), Some((instance_name, instance_project))) => {
                            AppCommand::DetachStorageVolume(VolumeDetachmentSpec {
                                pool: manager.pool.clone(),
                                volume_name: volume.name.clone(),
                                instance_name: instance_name.clone(),
                                instance_project: instance_project.clone(),
                                device_name: form.device_name.trim().to_owned(),
                                target_kind: form.target_kind,
                            })
                        }
                        _ => AppCommand::None,
                    }
                }
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.attach_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.attach_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 2 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Up if form.field == 0 && !form.instances.is_empty() => {
                    form.instance =
                        (form.instance + form.instances.len() - 1) % form.instances.len();
                    AppCommand::None
                }
                KeyCode::Right | KeyCode::Down | KeyCode::Char(' ')
                    if form.field == 0 && !form.instances.is_empty() =>
                {
                    form.instance = (form.instance + 1) % form.instances.len();
                    AppCommand::None
                }
                KeyCode::Backspace if form.field > 0 => {
                    if form.field == 1 {
                        form.device_name.pop();
                    } else {
                        form.path.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(character)
                    if form.field > 0 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    if form.field == 1 {
                        form.device_name.push(character);
                    } else {
                        form.path.push(character);
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.instances.is_empty() => {
                    self.model.last_error =
                        Some("no instance exists in the volume project".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter
                    if form.device_name.trim().is_empty() || form.path.trim().is_empty() =>
                {
                    self.model.last_error =
                        Some("device name and mount path are required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let volume = manager.volumes.get(manager.selected);
                    let instance = form.instances.get(form.instance);
                    match (volume, instance) {
                        (Some(volume), Some((instance_name, instance_project))) => {
                            AppCommand::AttachStorageVolume(VolumeAttachmentSpec {
                                pool: manager.pool.clone(),
                                volume_project: volume.project.clone(),
                                volume_name: volume.name.clone(),
                                instance_name: instance_name.clone(),
                                instance_project: instance_project.clone(),
                                device_name: form.device_name.trim().to_owned(),
                                path: form.path.trim().to_owned(),
                                target_kind: form.target_kind,
                            })
                        }
                        _ => AppCommand::None,
                    }
                }
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.edit_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.edit_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field == 0 => {
                    form.field = 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = 1 - form.field;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    if form.field == 0 {
                        form.description.pop();
                    } else {
                        form.size.pop();
                    }
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if form.field == 0 {
                        form.description.push(character);
                    } else {
                        form.size.push(character);
                    }
                    AppCommand::None
                }
                KeyCode::Enter => manager
                    .volumes
                    .get(manager.selected)
                    .map(|volume| {
                        AppCommand::UpdateStorageVolume(StorageVolumeUpdateSpec {
                            pool: manager.pool.clone(),
                            project: volume.project.clone(),
                            name: volume.name.clone(),
                            description: form.description.trim().to_owned(),
                            size: form.size.trim().to_owned(),
                            config: volume.config.clone(),
                        })
                    })
                    .unwrap_or(AppCommand::None),
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.create_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.create_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 8 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 8) % 9;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => {
                    form.content_type = match form.content_type.as_str() {
                        "filesystem" => "block",
                        "block" => "iso",
                        _ => "filesystem",
                    }
                    .to_owned();
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 8 => {
                    form.shared = !form.shared;
                    AppCommand::None
                }
                KeyCode::Backspace if !matches!(form.field, 2 | 8) => {
                    match form.field {
                        0 => {
                            form.name.pop();
                        }
                        1 => {
                            form.project.pop();
                        }
                        3 => {
                            form.description.pop();
                        }
                        4 => {
                            form.size.pop();
                        }
                        5 => {
                            form.initial_uid.pop();
                        }
                        6 => {
                            form.initial_gid.pop();
                        }
                        7 => {
                            form.initial_mode.pop();
                        }
                        _ => {}
                    }
                    AppCommand::None
                }
                KeyCode::Char(character)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !matches!(form.field, 2 | 8) =>
                {
                    match form.field {
                        0 => form.name.push(character),
                        1 => form.project.push(character),
                        3 => form.description.push(character),
                        4 => form.size.push(character),
                        5 => form.initial_uid.push(character),
                        6 => form.initial_gid.push(character),
                        7 => form.initial_mode.push(character),
                        _ => {}
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.name.trim().is_empty() => {
                    self.model.last_error = Some("volume name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter
                    if !can_initial_owner
                        && (!form.initial_uid.trim().is_empty()
                            || !form.initial_gid.trim().is_empty()
                            || !form.initial_mode.trim().is_empty()) =>
                {
                    self.model.last_error = Some(
                        "initial ownership unavailable: server lacks storage_initial_owner".into(),
                    );
                    AppCommand::None
                }
                KeyCode::Enter if form.shared && !can_shared_block => {
                    self.model.last_error = Some(
                        "shared block volumes unavailable: server lacks shared_custom_block_volumes"
                            .into(),
                    );
                    AppCommand::None
                }
                KeyCode::Enter if form.shared && form.content_type != "block" => {
                    self.model.last_error =
                        Some("shared mode is only valid for block volumes".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::CreateStorageVolume(StorageVolumeSpec {
                    pool: manager.pool.clone(),
                    project: form.project.trim().to_owned(),
                    name: form.name.trim().to_owned(),
                    content_type: form.content_type.clone(),
                    description: form.description.trim().to_owned(),
                    size: form.size.trim().to_owned(),
                    initial_uid: form.initial_uid.trim().to_owned(),
                    initial_gid: form.initial_gid.trim().to_owned(),
                    initial_mode: form.initial_mode.trim().to_owned(),
                    shared: form.shared,
                }),
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.import_form.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.import_form = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 3 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 3 => {
                    form.import_type = if form.import_type == "backup" {
                        "iso"
                    } else {
                        "backup"
                    }
                    .to_owned();
                    AppCommand::None
                }
                KeyCode::Backspace if form.field < 3 => {
                    match form.field {
                        0 => {
                            form.local_path.pop();
                        }
                        1 => {
                            form.project.pop();
                        }
                        _ => {
                            form.name.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if form.field < 3 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    match form.field {
                        0 => form.local_path.push(c),
                        1 => form.project.push(c),
                        _ => form.name.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter
                    if form.local_path.trim().is_empty() || form.name.trim().is_empty() =>
                {
                    self.model.last_error =
                        Some("archive path and volume name are required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::ImportStorageVolume {
                    pool: manager.pool.clone(),
                    project: form.project.trim().to_owned(),
                    name: form.name.trim().to_owned(),
                    import_type: form.import_type.clone(),
                    path: form.local_path.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.volume_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.volumes.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.volumes.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.volumes.is_empty() => {
                manager.selected =
                    (manager.selected + manager.volumes.len() - 1) % manager.volumes.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.create_form = Some(VolumeCreateForm {
                    name: String::new(),
                    project: "default".to_owned(),
                    content_type: "filesystem".to_owned(),
                    description: String::new(),
                    size: String::new(),
                    initial_uid: String::new(),
                    initial_gid: String::new(),
                    initial_mode: String::new(),
                    shared: false,
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('i') => {
                manager.import_form = Some(VolumeImportForm {
                    local_path: "./volume-backup.tar.gz".to_owned(),
                    project: "default".to_owned(),
                    name: String::new(),
                    import_type: "backup".to_owned(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.volumes.is_empty() => {
                let volume = &manager.volumes[manager.selected];
                manager.edit_form = Some(VolumeEditForm {
                    description: volume.description.clone(),
                    size: volume.config.get("size").cloned().unwrap_or_default(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('a') if !manager.volumes.is_empty() => {
                let project = manager.volumes[manager.selected].project.clone();
                let instances = self
                    .model
                    .instances
                    .iter()
                    .filter(|instance| instance.project == project)
                    .map(|instance| (instance.name.clone(), instance.project.clone()))
                    .collect();
                manager.attach_form = Some(VolumeAttachForm {
                    instances,
                    instance: 0,
                    device_name: "data".to_owned(),
                    path: "/mnt/data".to_owned(),
                    field: 0,
                    target_kind: ResourceKind::Instance,
                });
                AppCommand::None
            }
            KeyCode::Char('A') if !manager.volumes.is_empty() => {
                let project = manager.volumes[manager.selected].project.clone();
                let instances = self
                    .model
                    .profiles
                    .iter()
                    .filter(|profile| profile.project == project)
                    .map(|profile| (profile.name.clone(), profile.project.clone()))
                    .collect();
                manager.attach_form = Some(VolumeAttachForm {
                    instances,
                    instance: 0,
                    device_name: "data".to_owned(),
                    path: "/mnt/data".to_owned(),
                    field: 0,
                    target_kind: ResourceKind::Profile,
                });
                AppCommand::None
            }
            KeyCode::Char('t') if !manager.volumes.is_empty() => {
                let project = manager.volumes[manager.selected].project.clone();
                let instances = self
                    .model
                    .instances
                    .iter()
                    .filter(|instance| instance.project == project)
                    .map(|instance| (instance.name.clone(), instance.project.clone()))
                    .collect();
                manager.detach_form = Some(VolumeDetachForm {
                    instances,
                    instance: 0,
                    device_name: "data".to_owned(),
                    field: 0,
                    target_kind: ResourceKind::Instance,
                });
                AppCommand::None
            }
            KeyCode::Char('T') if !manager.volumes.is_empty() => {
                let project = manager.volumes[manager.selected].project.clone();
                let instances = self
                    .model
                    .profiles
                    .iter()
                    .filter(|profile| profile.project == project)
                    .map(|profile| (profile.name.clone(), profile.project.clone()))
                    .collect();
                manager.detach_form = Some(VolumeDetachForm {
                    instances,
                    instance: 0,
                    device_name: "data".to_owned(),
                    field: 0,
                    target_kind: ResourceKind::Profile,
                });
                AppCommand::None
            }
            KeyCode::Char('s') if !manager.volumes.is_empty() => {
                let volume = manager.volumes[manager.selected].clone();
                manager.snapshots = Some(VolumeSnapshotsForm {
                    volume: volume.clone(),
                    snapshots: Vec::new(),
                    selected: 0,
                    name_input: None,
                    confirmation: None,
                });
                AppCommand::ReadStorageVolumeSnapshots {
                    pool: manager.pool.clone(),
                    project: volume.project,
                    volume: volume.name,
                }
            }
            KeyCode::Char('B') if !manager.volumes.is_empty() => {
                let volume = manager.volumes[manager.selected].clone();
                AppCommand::ReadStorageVolumeBackups {
                    pool: manager.pool.clone(),
                    project: volume.project,
                    volume: volume.name,
                }
            }
            KeyCode::Char('D') if !manager.volumes.is_empty() && can_nbd => {
                let volume = manager.volumes[manager.selected].clone();
                manager.bitmaps = Some(VolumeBitmapsForm {
                    volume: volume.clone(),
                    bitmaps: Vec::new(),
                    selected: 0,
                    create: None,
                    delete_input: None,
                });
                AppCommand::ReadStorageVolumeBitmaps {
                    pool: manager.pool.clone(),
                    project: volume.project,
                    volume: volume.name,
                }
            }
            KeyCode::Char('D') if !manager.volumes.is_empty() => {
                self.model.last_error = Some(
                    "dirty bitmap management unavailable: server lacks storage_volume_nbd".into(),
                );
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.volumes.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R') if !manager.volumes.is_empty() => {
                manager.rebuild_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('C') if !manager.volumes.is_empty() => {
                let source = &manager.volumes[manager.selected];
                let target_pool = self
                    .model
                    .storage_pools
                    .iter()
                    .find(|pool| pool.name != manager.pool)
                    .map(|pool| pool.name.clone())
                    .unwrap_or_else(|| manager.pool.clone());
                manager.copy_form = Some(VolumeCopyForm {
                    target_pool,
                    target_project: source.project.clone(),
                    target_name: source.name.clone(),
                    volume_only: false,
                    refresh: false,
                    live: false,
                    refresh_exclude_older: false,
                    supports_live: can_live_migration,
                    supports_refresh_exclude: can_refresh_exclude,
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('S') if !manager.volumes.is_empty() && can_state => {
                let volume = manager.volumes[manager.selected].clone();
                AppCommand::ReadStorageVolumeState {
                    pool: manager.pool.clone(),
                    project: volume.project,
                    volume: volume.name,
                }
            }
            KeyCode::Char('S') if !manager.volumes.is_empty() => {
                self.model.last_error =
                    Some("volume state unavailable: server lacks storage_volume_state".into());
                AppCommand::None
            }
            KeyCode::Char('P') if !manager.volumes.is_empty() && (can_sftp || can_nbd) => {
                manager.protocol_form = Some((
                    if can_sftp {
                        "sftp".into()
                    } else {
                        "nbd".into()
                    },
                    "0".into(),
                    0,
                ));
                AppCommand::None
            }
            KeyCode::Char('P') if !manager.volumes.is_empty() => {
                self.model.last_error = Some(
                    "volume protocol access unavailable: server lacks custom_volume_sftp and storage_volume_nbd"
                        .into(),
                );
                AppCommand::None
            }
            KeyCode::Char('F') if !manager.volumes.is_empty() && can_files => {
                manager.file_form = Some(VolumeFileForm {
                    volume: manager.volumes[manager.selected].clone(),
                    path: "/".into(),
                    contents: String::new(),
                    entries: Vec::new(),
                    selected: 0,
                    browsing: false,
                    transfer: None,
                    delete_input: None,
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('F') if !manager.volumes.is_empty() => {
                self.model.last_error =
                    Some("volume file access unavailable: server lacks file_storage_volume".into());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_address_set_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .address_set_manager
            .as_mut()
            .expect("address-set manager");
        if let Some(input) = manager.rename_input.as_mut() {
            let Some(set) = manager.sets.get(manager.selected) else {
                return AppCommand::None;
            };
            return match key.code {
                KeyCode::Esc => {
                    manager.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !input.trim().is_empty() && input.trim() != set.name => {
                    AppCommand::RenameNetworkAddressSet {
                        project: set.project.clone(),
                        name: set.name.clone(),
                        new_name: input.trim().into(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error = Some("new name must be non-empty and different".into());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.delete_input.as_mut() {
            let expected = format!(
                "delete address set {}",
                manager
                    .sets
                    .get(manager.selected)
                    .map(|s| s.name.as_str())
                    .unwrap_or_default()
            );
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let set = manager.sets.get(manager.selected).cloned();
                    manager.delete_input = None;
                    set.map(|s| AppCommand::DeleteNetworkAddressSet {
                        project: s.project,
                        name: s.name,
                    })
                    .unwrap_or(AppCommand::None)
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 3 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 3) % 4;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match form.field {
                        0 => {
                            form.name.pop();
                        }
                        1 => {
                            form.project.pop();
                        }
                        2 => {
                            form.description.pop();
                        }
                        _ => {
                            form.addresses.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match form.field {
                        0 => form.name.push(c),
                        1 => form.project.push(c),
                        2 => form.description.push(c),
                        _ => form.addresses.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.name.trim().is_empty() => {
                    self.model.last_error = Some("address-set name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let addresses = form
                        .addresses
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToOwned::to_owned)
                        .collect();
                    if form.editing {
                        let set = &manager.sets[manager.selected];
                        AppCommand::UpdateNetworkAddressSet {
                            project: set.project.clone(),
                            name: set.name.clone(),
                            description: form.description.trim().to_owned(),
                            addresses,
                            config: set.config.clone(),
                        }
                    } else {
                        AppCommand::CreateNetworkAddressSet {
                            project: form.project.trim().to_owned(),
                            name: form.name.trim().to_owned(),
                            description: form.description.trim().to_owned(),
                            addresses,
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.address_set_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.sets.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.sets.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.sets.is_empty() => {
                manager.selected = (manager.selected + manager.sets.len() - 1) % manager.sets.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(AddressSetEditor {
                    editing: false,
                    name: String::new(),
                    project: "default".to_owned(),
                    description: String::new(),
                    addresses: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.sets.is_empty() => {
                let set = &manager.sets[manager.selected];
                manager.editor = Some(AddressSetEditor {
                    editing: true,
                    name: set.name.clone(),
                    project: set.project.clone(),
                    description: set.description.clone(),
                    addresses: set.addresses.join(","),
                    field: 2,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.sets.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R') if !manager.sets.is_empty() => {
                manager.rename_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_integration_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .integration_manager
            .as_mut()
            .expect("integration manager");
        if let Some(input) = manager.rename_input.as_mut() {
            let current = manager
                .integrations
                .get(manager.selected)
                .map(|v| v.name.clone())
                .unwrap_or_default();
            return match key.code {
                KeyCode::Esc => {
                    manager.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !input.trim().is_empty() && input.trim() != current => {
                    AppCommand::RenameNetworkIntegration {
                        name: current,
                        new_name: input.trim().into(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error = Some("new name must be non-empty and different".into());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase();
            let input = manager.delete_input.as_mut().expect("delete input");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let name = manager
                        .integrations
                        .get(manager.selected)
                        .map(|v| v.name.clone())
                        .unwrap_or_default();
                    manager.delete_input = None;
                    AppCommand::DeleteNetworkIntegration { name }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(editor) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if editor.field < 8 => {
                    editor.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    editor.field = (editor.field + 8) % 9;
                    AppCommand::None
                }
                KeyCode::Backspace if editor.field != 1 => {
                    editor.fields[editor.field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c)
                    if editor.field != 1 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    editor.fields[editor.field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter if editor.fields[0].trim().is_empty() => {
                    self.model.last_error = Some("integration name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let mut config = if editor.editing {
                        manager.integrations[manager.selected].config.clone()
                    } else {
                        std::collections::BTreeMap::new()
                    };
                    for (key, value) in [
                        ("ovn.northbound_connection", &editor.fields[3]),
                        ("ovn.southbound_connection", &editor.fields[4]),
                        ("ovn.ca_cert", &editor.fields[5]),
                        ("ovn.client_cert", &editor.fields[6]),
                        ("ovn.client_key", &editor.fields[7]),
                        ("ovn.transit.pattern", &editor.fields[8]),
                    ] {
                        if value.trim().is_empty() {
                            config.remove(key);
                        } else {
                            config.insert(key.to_owned(), value.trim().to_owned());
                        }
                    }
                    if editor.editing {
                        AppCommand::UpdateNetworkIntegration {
                            name: manager.integrations[manager.selected].name.clone(),
                            description: editor.fields[2].trim().to_owned(),
                            config,
                        }
                    } else {
                        AppCommand::CreateNetworkIntegration {
                            name: editor.fields[0].trim().to_owned(),
                            integration_type: editor.fields[1].clone(),
                            description: editor.fields[2].trim().to_owned(),
                            config,
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.integration_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.integrations.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.integrations.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.integrations.is_empty() => {
                manager.selected = (manager.selected + manager.integrations.len() - 1)
                    % manager.integrations.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(IntegrationEditor {
                    editing: false,
                    fields: [
                        String::new(),
                        "ovn".to_owned(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ],
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.integrations.is_empty() => {
                let value = &manager.integrations[manager.selected];
                manager.editor = Some(IntegrationEditor {
                    editing: true,
                    fields: [
                        value.name.clone(),
                        value.integration_type.clone(),
                        value.description.clone(),
                        value
                            .config
                            .get("ovn.northbound_connection")
                            .cloned()
                            .unwrap_or_default(),
                        value
                            .config
                            .get("ovn.southbound_connection")
                            .cloned()
                            .unwrap_or_default(),
                        value.config.get("ovn.ca_cert").cloned().unwrap_or_default(),
                        value
                            .config
                            .get("ovn.client_cert")
                            .cloned()
                            .unwrap_or_default(),
                        value
                            .config
                            .get("ovn.client_key")
                            .cloned()
                            .unwrap_or_default(),
                        value
                            .config
                            .get("ovn.transit.pattern")
                            .cloned()
                            .unwrap_or_default(),
                    ],
                    field: 2,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.integrations.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R') if !manager.integrations.is_empty() => {
                manager.rename_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_bgp_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self.bgp_form.as_mut().expect("BGP form");
        match key.code {
            KeyCode::Esc => {
                self.bgp_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter if form.field < 9 => {
                form.field += 1;
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.field = (form.field + 9) % 10;
                AppCommand::None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if form.field == 6 || form.field == 7 =>
            {
                form.fields[form.field] = if form.fields[form.field] == "true" {
                    "false"
                } else {
                    "true"
                }
                .to_owned();
                AppCommand::None
            }
            KeyCode::Backspace if form.field != 6 && form.field != 7 => {
                form.fields[form.field].pop();
                AppCommand::None
            }
            KeyCode::Char(c)
                if form.field != 6
                    && form.field != 7
                    && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                form.fields[form.field].push(c);
                AppCommand::None
            }
            KeyCode::Enter
                if !form.fields[0].trim().is_empty() && form.fields[3].trim().is_empty() =>
            {
                self.model.last_error =
                    Some("peer ASN is required when a peer name is set".to_owned());
                AppCommand::None
            }
            KeyCode::Enter
                if !form.fields[0].trim().is_empty()
                    && form.fields[1].trim().is_empty()
                    && form.fields[2].trim().is_empty() =>
            {
                self.model.last_error = Some("peer address or interface is required".to_owned());
                AppCommand::None
            }
            KeyCode::Enter => {
                let definition = form.updated_definition();
                AppCommand::UpdateResource {
                    value: definition.value.clone(),
                    definition,
                }
            }
            _ => AppCommand::None,
        }
    }

    fn handle_zone_manager_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self.zone_manager.as_mut().expect("zone manager");
        if let Some(records) = manager.records.as_mut() {
            if let Some(input) = records.delete_input.as_mut() {
                let expected = format!(
                    "delete zone record {}",
                    records
                        .records
                        .get(records.selected)
                        .map(|r| r.name.as_str())
                        .unwrap_or_default()
                );
                return match key.code {
                    KeyCode::Esc => {
                        records.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        let name = records
                            .records
                            .get(records.selected)
                            .map(|r| r.name.clone())
                            .unwrap_or_default();
                        records.delete_input = None;
                        AppCommand::DeleteNetworkZoneRecord {
                            project: records.zone.project.clone(),
                            zone: records.zone.name.clone(),
                            name,
                        }
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(form) = records.editor.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        records.editor = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if form.field < 4 => {
                        form.field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        form.field = (form.field + 4) % 5;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => {
                        form.entry_type = match form.entry_type.as_str() {
                            "A" => "AAAA",
                            "AAAA" => "CNAME",
                            "CNAME" => "MX",
                            "MX" => "TXT",
                            _ => "A",
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Backspace if form.field != 2 => {
                        match form.field {
                            0 => {
                                form.name.pop();
                            }
                            1 => {
                                form.description.pop();
                            }
                            3 => {
                                form.value.pop();
                            }
                            4 => {
                                form.ttl.pop();
                            }
                            _ => {}
                        }
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if form.field != 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        match form.field {
                            0 => form.name.push(c),
                            1 => form.description.push(c),
                            3 => form.value.push(c),
                            4 => form.ttl.push(c),
                            _ => {}
                        }
                        AppCommand::None
                    }
                    KeyCode::Enter
                        if form.name.trim().is_empty() || form.value.trim().is_empty() =>
                    {
                        self.model.last_error =
                            Some("record name and value are required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let entry = NetworkZoneRecordEntry {
                            entry_type: form.entry_type.clone(),
                            value: form.value.trim().to_owned(),
                            ttl: form.ttl.trim().parse().unwrap_or(300),
                        };
                        if let Some(original) = form.original {
                            let current = &records.records[original];
                            let mut entries = current.entries.clone();
                            if entries.is_empty() {
                                entries.push(entry);
                            } else {
                                entries[0] = entry;
                            }
                            AppCommand::UpdateNetworkZoneRecord {
                                project: records.zone.project.clone(),
                                zone: records.zone.name.clone(),
                                name: current.name.clone(),
                                description: form.description.trim().to_owned(),
                                entries,
                            }
                        } else {
                            AppCommand::CreateNetworkZoneRecord {
                                project: records.zone.project.clone(),
                                zone: records.zone.name.clone(),
                                name: form.name.trim().to_owned(),
                                description: form.description.trim().to_owned(),
                                entry,
                            }
                        }
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.records = None;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !records.records.is_empty() => {
                    records.selected = (records.selected + 1) % records.records.len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !records.records.is_empty() => {
                    records.selected =
                        (records.selected + records.records.len() - 1) % records.records.len();
                    AppCommand::None
                }
                KeyCode::Char('n') => {
                    records.editor = Some(ZoneRecordEditor {
                        original: None,
                        name: String::new(),
                        description: String::new(),
                        entry_type: "A".to_owned(),
                        value: String::new(),
                        ttl: "300".to_owned(),
                        field: 0,
                    });
                    AppCommand::None
                }
                KeyCode::Char('e') | KeyCode::Enter if !records.records.is_empty() => {
                    let record = &records.records[records.selected];
                    let entry = record.entries.first().cloned().unwrap_or_default();
                    records.editor = Some(ZoneRecordEditor {
                        original: Some(records.selected),
                        name: record.name.clone(),
                        description: record.description.clone(),
                        entry_type: if entry.entry_type.is_empty() {
                            "A".to_owned()
                        } else {
                            entry.entry_type
                        },
                        value: entry.value,
                        ttl: if entry.ttl == 0 {
                            "300".to_owned()
                        } else {
                            entry.ttl.to_string()
                        },
                        field: 1,
                    });
                    AppCommand::None
                }
                KeyCode::Char('d') if !records.records.is_empty() => {
                    records.delete_input = Some(String::new());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.rename_input.as_mut() {
            let Some(zone) = manager.zones.get(manager.selected) else {
                return AppCommand::None;
            };
            return match key.code {
                KeyCode::Esc => {
                    manager.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !input.trim().is_empty() && input.trim() != zone.name => {
                    AppCommand::RenameNetworkZone {
                        project: zone.project.clone(),
                        name: zone.name.clone(),
                        new_name: input.trim().into(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error = Some("new name must be non-empty and different".into());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(input) = manager.delete_input.as_mut() {
            let expected = format!(
                "delete zone {}",
                manager
                    .zones
                    .get(manager.selected)
                    .map(|z| z.name.as_str())
                    .unwrap_or_default()
            );
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let zone = manager.zones.get(manager.selected).cloned();
                    manager.delete_input = None;
                    zone.map(|z| AppCommand::DeleteNetworkZone {
                        project: z.project,
                        name: z.name,
                    })
                    .unwrap_or(AppCommand::None)
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(form) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if form.field < 2 => {
                    form.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    form.field = (form.field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    match form.field {
                        0 => {
                            form.name.pop();
                        }
                        1 => {
                            form.project.pop();
                        }
                        _ => {
                            form.description.pop();
                        }
                    }
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match form.field {
                        0 => form.name.push(c),
                        1 => form.project.push(c),
                        _ => form.description.push(c),
                    }
                    AppCommand::None
                }
                KeyCode::Enter if form.name.trim().is_empty() => {
                    self.model.last_error = Some("zone name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    if form.editing {
                        let zone = &manager.zones[manager.selected];
                        AppCommand::UpdateNetworkZone {
                            project: zone.project.clone(),
                            name: zone.name.clone(),
                            description: form.description.trim().to_owned(),
                            config: zone.config.clone(),
                        }
                    } else {
                        AppCommand::CreateNetworkZone {
                            project: form.project.trim().to_owned(),
                            name: form.name.trim().to_owned(),
                            description: form.description.trim().to_owned(),
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.zone_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.zones.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.zones.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.zones.is_empty() => {
                manager.selected =
                    (manager.selected + manager.zones.len() - 1) % manager.zones.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(ZoneEditor {
                    editing: false,
                    name: String::new(),
                    project: "default".to_owned(),
                    description: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') if !manager.zones.is_empty() => {
                let zone = &manager.zones[manager.selected];
                manager.editor = Some(ZoneEditor {
                    editing: true,
                    name: zone.name.clone(),
                    project: zone.project.clone(),
                    description: zone.description.clone(),
                    field: 2,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.zones.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R') if !manager.zones.is_empty() => {
                manager.rename_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Enter if !manager.zones.is_empty() => {
                let z = &manager.zones[manager.selected];
                AppCommand::ReadNetworkZoneRecords {
                    project: z.project.clone(),
                    zone: z.name.clone(),
                }
            }
            _ => AppCommand::None,
        }
    }

    fn open_network_services(&mut self) -> AppCommand {
        let Some(network) = self.model.networks.get(self.selected_network) else {
            return AppCommand::None;
        };
        if !network.managed {
            self.model.last_error = Some(format!(
                "network '{}' is unmanaged and has no Incus service controls",
                network.name
            ));
            return AppCommand::None;
        }
        let name = network.name.clone();
        let project = network.project.clone();
        self.network_services = Some(NetworkServicesManager::new(name.clone(), project.clone()));
        self.model.last_error = None;
        AppCommand::ReadNetworkServices {
            network: name,
            project,
        }
    }

    fn handle_network_services_key(&mut self, key: KeyEvent) -> AppCommand {
        let can_state = self.supports_extension("network_state");
        let can_acl_log = self.supports_extension("network_acl_log");
        let can_lb_state = self.supports_extension("network_load_balancer_state");
        let can_lb_health = self.supports_extension("network_load_balancer_health_check");
        let manager = self
            .network_services
            .as_mut()
            .expect("network services checked before handling key");
        if manager.detail.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                manager.detail = None;
            }
            return AppCommand::None;
        }
        if let Some(input) = manager.rename_input.as_mut() {
            let current = manager
                .acls
                .get(manager.selected)
                .map(|acl| acl.name.clone())
                .unwrap_or_default();
            return match key.code {
                KeyCode::Esc => {
                    manager.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if input.trim().is_empty() || input.trim() == current => {
                    self.model.last_error =
                        Some("new ACL name must be non-empty and different".into());
                    AppCommand::None
                }
                KeyCode::Enter => AppCommand::RenameNetworkAcl {
                    network: manager.network.clone(),
                    project: manager.project.clone(),
                    name: current,
                    new_name: input.trim().into(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(rules) = manager.acl_rules.as_mut() {
            if let Some(input) = rules.delete_input.as_mut() {
                let expected = format!(
                    "delete {} rule {}",
                    if rules.ingress { "ingress" } else { "egress" },
                    rules.selected + 1
                );
                return match key.code {
                    KeyCode::Esc => {
                        rules.delete_input = None;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter if *input == expected => {
                        let selected = rules.selected;
                        rules.rules_mut().remove(selected);
                        rules.selected = rules.selected.min(rules.rules().len().saturating_sub(1));
                        rules.delete_input = None;
                        AppCommand::UpdateNetworkAcl(rules.acl.clone())
                    }
                    KeyCode::Enter => {
                        self.model.last_error =
                            Some(format!("confirmation must exactly match: {expected}"));
                        AppCommand::None
                    }
                    _ => AppCommand::None,
                };
            }
            if let Some(editor) = rules.editor.as_mut() {
                return match key.code {
                    KeyCode::Esc => {
                        rules.editor = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if editor.field < 9 => {
                        editor.field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        editor.field = (editor.field + 9) % 10;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if editor.field == 0 => {
                        editor.fields[0] = match editor.fields[0].as_str() {
                            "allow" => "drop",
                            "drop" => "reject",
                            _ => "allow",
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if editor.field == 3 => {
                        editor.fields[3] = match editor.fields[3].as_str() {
                            "tcp" => "udp",
                            "udp" => "icmp4",
                            "icmp4" => "icmp6",
                            _ => "tcp",
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if editor.field == 9 => {
                        editor.fields[9] = match editor.fields[9].as_str() {
                            "enabled" => "logged",
                            "logged" => "disabled",
                            _ => "enabled",
                        }
                        .to_owned();
                        AppCommand::None
                    }
                    KeyCode::Backspace if !matches!(editor.field, 0 | 3 | 9) => {
                        editor.fields[editor.field].pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if !matches!(editor.field, 0 | 3 | 9)
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        editor.fields[editor.field].push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let rule = editor.rule();
                        let original = editor.original;
                        rules.editor = None;
                        if let Some(index) = original {
                            rules.rules_mut()[index] = rule;
                        } else {
                            rules.rules_mut().push(rule);
                            rules.selected = rules.rules().len() - 1;
                        }
                        AppCommand::UpdateNetworkAcl(rules.acl.clone())
                    }
                    _ => AppCommand::None,
                };
            }
            return match key.code {
                KeyCode::Esc => {
                    manager.acl_rules = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                    rules.ingress = !rules.ingress;
                    rules.selected = 0;
                    AppCommand::None
                }
                KeyCode::Char('j') | KeyCode::Down if !rules.rules().is_empty() => {
                    rules.selected = (rules.selected + 1) % rules.rules().len();
                    AppCommand::None
                }
                KeyCode::Char('k') | KeyCode::Up if !rules.rules().is_empty() => {
                    rules.selected =
                        (rules.selected + rules.rules().len() - 1) % rules.rules().len();
                    AppCommand::None
                }
                KeyCode::Char('a') => {
                    rules.editor = Some(AclRuleEditor::from_rule(None, None));
                    AppCommand::None
                }
                KeyCode::Char('e') | KeyCode::Enter if !rules.rules().is_empty() => {
                    rules.editor = Some(AclRuleEditor::from_rule(
                        Some(rules.selected),
                        rules.rules().get(rules.selected),
                    ));
                    AppCommand::None
                }
                KeyCode::Char('d') if !rules.rules().is_empty() => {
                    rules.delete_input = Some(String::new());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase().unwrap_or_default();
            let input = manager
                .delete_input
                .as_mut()
                .expect("network service deletion remains present");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    manager.delete_input = None;
                    match manager.tab {
                        NetworkServicesTab::Forwards => manager
                            .forwards
                            .get(manager.selected)
                            .map(|forward| AppCommand::DeleteNetworkForward {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                listen_address: forward.listen_address.clone(),
                            })
                            .unwrap_or(AppCommand::None),
                        NetworkServicesTab::Acls => manager
                            .acls
                            .get(manager.selected)
                            .map(|acl| AppCommand::DeleteNetworkAcl {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                name: acl.name.clone(),
                            })
                            .unwrap_or(AppCommand::None),
                        NetworkServicesTab::LoadBalancers => manager
                            .load_balancers
                            .get(manager.selected)
                            .map(|item| AppCommand::DeleteNetworkLoadBalancer {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                listen_address: item.listen_address.clone(),
                            })
                            .unwrap_or(AppCommand::None),
                        NetworkServicesTab::Peers => manager
                            .peers
                            .get(manager.selected)
                            .map(|item| AppCommand::DeleteNetworkPeer {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                name: item.name.clone(),
                            })
                            .unwrap_or(AppCommand::None),
                        NetworkServicesTab::Leases => AppCommand::None,
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }

        if let Some(form) = manager.form.as_mut() {
            return match form {
                NetworkServiceForm::Forward {
                    fields,
                    field,
                    editing,
                } => match key.code {
                    KeyCode::Esc => {
                        manager.form = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if *field < 5 => {
                        *field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        *field = (*field + 5) % 6;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                        fields[2] = if fields[2] == "tcp" { "udp" } else { "tcp" }.to_owned();
                        AppCommand::None
                    }
                    KeyCode::Backspace if *field != 2 => {
                        fields[*field].pop();
                        AppCommand::None
                    }
                    KeyCode::Char(character)
                        if !key.modifiers.contains(KeyModifiers::CONTROL) && *field != 2 =>
                    {
                        fields[*field].push(character);
                        AppCommand::None
                    }
                    KeyCode::Enter if fields[0].trim().is_empty() => {
                        self.model.last_error =
                            Some("forward listen address is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let port = NetworkForwardPort {
                            protocol: fields[2].clone(),
                            listen_port: fields[3].trim().to_owned(),
                            target_address: fields[4].trim().to_owned(),
                            target_port: fields[5].trim().to_owned(),
                        };
                        if *editing {
                            let current = &manager.forwards[manager.selected];
                            let mut ports = current.ports.clone();
                            if ports.is_empty() {
                                ports.push(port);
                            } else {
                                ports[0] = port;
                            }
                            AppCommand::UpdateNetworkForward(NetworkForwardUpdateSpec {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                listen_address: current.listen_address.clone(),
                                description: fields[1].trim().to_owned(),
                                ports,
                                config: current.config.clone(),
                            })
                        } else {
                            AppCommand::CreateNetworkForward(NetworkForwardSpec {
                                network: manager.network.clone(),
                                project: manager.project.clone(),
                                listen_address: fields[0].trim().to_owned(),
                                description: fields[1].trim().to_owned(),
                                port,
                            })
                        }
                    }
                    _ => AppCommand::None,
                },
                NetworkServiceForm::Acl {
                    name,
                    description,
                    field,
                } => match key.code {
                    KeyCode::Esc => {
                        manager.form = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if *field == 0 => {
                        *field = 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        *field = 1 - *field;
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        if *field == 0 {
                            name.pop();
                        } else {
                            description.pop();
                        }
                        AppCommand::None
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if *field == 0 {
                            name.push(character);
                        } else {
                            description.push(character);
                        }
                        AppCommand::None
                    }
                    KeyCode::Enter if name.trim().is_empty() => {
                        self.model.last_error = Some("ACL name is required".to_owned());
                        AppCommand::None
                    }
                    KeyCode::Enter => AppCommand::CreateNetworkAcl(NetworkAclSpec {
                        project: manager.project.clone(),
                        name: name.trim().to_owned(),
                        description: description.trim().to_owned(),
                    }),
                    _ => AppCommand::None,
                },
                NetworkServiceForm::LoadBalancer {
                    fields,
                    field,
                    editing,
                } => match key.code {
                    KeyCode::Esc => {
                        manager.form = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if *field < 11 => {
                        *field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        *field = (*field + 11) % 12;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                        fields[2] = if fields[2] == "tcp" { "udp" } else { "tcp" }.to_owned();
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 7 => {
                        fields[7] = if fields[7] == "true" {
                            "false".to_owned()
                        } else {
                            "true".to_owned()
                        };
                        AppCommand::None
                    }
                    KeyCode::Backspace if !matches!(*field, 2 | 7) => {
                        fields[*field].pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if !matches!(*field, 2 | 7)
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        fields[*field].push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter
                        if fields[0].trim().is_empty()
                            || fields[4].trim().is_empty()
                            || fields[5].trim().is_empty() =>
                    {
                        self.model.last_error = Some(
                            "listen address, backend name, and target address are required"
                                .to_owned(),
                        );
                        AppCommand::None
                    }
                    KeyCode::Enter if fields[7] == "true" && !can_lb_health => {
                        self.model.last_error = Some(
                            "load-balancer health checks unavailable: server lacks network_load_balancer_health_check"
                                .into(),
                        );
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let health_config = std::collections::BTreeMap::from([
                            ("healthcheck".into(), fields[7].clone()),
                            ("healthcheck.interval".into(), fields[8].trim().into()),
                            ("healthcheck.timeout".into(), fields[9].trim().into()),
                            ("healthcheck.failure_count".into(), fields[10].trim().into()),
                            ("healthcheck.success_count".into(), fields[11].trim().into()),
                        ]);
                        let spec = NetworkLoadBalancerSpec {
                            network: manager.network.clone(),
                            project: manager.project.clone(),
                            listen_address: fields[0].trim().to_owned(),
                            description: fields[1].trim().to_owned(),
                            backend: NetworkLoadBalancerBackend {
                                name: fields[4].trim().to_owned(),
                                target_address: fields[5].trim().to_owned(),
                                target_port: fields[6].trim().to_owned(),
                            },
                            port: NetworkLoadBalancerPort {
                                protocol: fields[2].clone(),
                                listen_port: fields[3].trim().to_owned(),
                                target_backend: vec![fields[4].trim().to_owned()],
                            },
                            config: health_config,
                        };
                        if *editing {
                            let current = &manager.load_balancers[manager.selected];
                            let mut backends = current.backends.clone();
                            if backends.is_empty() {
                                backends.push(spec.backend);
                            } else {
                                backends[0] = spec.backend;
                            }
                            let mut ports = current.ports.clone();
                            if ports.is_empty() {
                                ports.push(spec.port);
                            } else {
                                ports[0] = spec.port;
                            }
                            AppCommand::UpdateNetworkLoadBalancer(NetworkLoadBalancerUpdateSpec {
                                network: spec.network,
                                project: spec.project,
                                listen_address: current.listen_address.clone(),
                                description: spec.description,
                                backends,
                                ports,
                                config: current
                                    .config
                                    .clone()
                                    .into_iter()
                                    .chain(spec.config)
                                    .collect(),
                            })
                        } else {
                            AppCommand::CreateNetworkLoadBalancer(spec)
                        }
                    }
                    _ => AppCommand::None,
                },
                NetworkServiceForm::Peer {
                    fields,
                    field,
                    editing,
                } => match key.code {
                    KeyCode::Esc => {
                        manager.form = None;
                        AppCommand::None
                    }
                    KeyCode::Tab | KeyCode::Enter if *field < 5 => {
                        *field += 1;
                        AppCommand::None
                    }
                    KeyCode::BackTab => {
                        *field = (*field + 5) % 6;
                        AppCommand::None
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *field == 2 => {
                        fields[2] = if fields[2] == "remote" {
                            "local".to_owned()
                        } else {
                            "remote".to_owned()
                        };
                        AppCommand::None
                    }
                    KeyCode::Backspace => {
                        fields[*field].pop();
                        AppCommand::None
                    }
                    KeyCode::Char(c)
                        if *field != 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        fields[*field].push(c);
                        AppCommand::None
                    }
                    KeyCode::Enter
                        if fields[0].trim().is_empty()
                            || (fields[2] == "local" && fields[4].trim().is_empty())
                            || (fields[2] == "remote" && fields[5].trim().is_empty()) =>
                    {
                        self.model.last_error = Some(
                            "peer name and the target network (local) or integration (remote) are required"
                                .to_owned(),
                        );
                        AppCommand::None
                    }
                    KeyCode::Enter => {
                        let spec = NetworkPeerSpec {
                            network: manager.network.clone(),
                            project: manager.project.clone(),
                            name: fields[0].trim().to_owned(),
                            description: fields[1].trim().to_owned(),
                            peer_type: fields[2].clone(),
                            target_project: fields[3].trim().to_owned(),
                            target_network: fields[4].trim().to_owned(),
                            target_integration: fields[5].trim().to_owned(),
                        };
                        if *editing {
                            let current = &manager.peers[manager.selected];
                            AppCommand::UpdateNetworkPeer(NetworkPeerUpdateSpec {
                                network: spec.network,
                                project: spec.project,
                                name: current.name.clone(),
                                description: spec.description,
                                peer_type: spec.peer_type,
                                target_project: spec.target_project,
                                target_network: spec.target_network,
                                target_integration: spec.target_integration,
                                config: current.config.clone(),
                            })
                        } else {
                            AppCommand::CreateNetworkPeer(spec)
                        }
                    }
                    _ => AppCommand::None,
                },
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.network_services = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Right => {
                manager.tab = match manager.tab {
                    NetworkServicesTab::Leases => NetworkServicesTab::Forwards,
                    NetworkServicesTab::Forwards => NetworkServicesTab::Acls,
                    NetworkServicesTab::Acls => NetworkServicesTab::LoadBalancers,
                    NetworkServicesTab::LoadBalancers => NetworkServicesTab::Peers,
                    NetworkServicesTab::Peers => NetworkServicesTab::Leases,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::BackTab | KeyCode::Left => {
                manager.tab = match manager.tab {
                    NetworkServicesTab::Leases => NetworkServicesTab::Peers,
                    NetworkServicesTab::Forwards => NetworkServicesTab::Leases,
                    NetworkServicesTab::Acls => NetworkServicesTab::Forwards,
                    NetworkServicesTab::LoadBalancers => NetworkServicesTab::Acls,
                    NetworkServicesTab::Peers => NetworkServicesTab::LoadBalancers,
                };
                manager.selected = 0;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if manager.len() > 0 => {
                manager.selected = (manager.selected + 1) % manager.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if manager.len() > 0 => {
                manager.selected = (manager.selected + manager.len() - 1) % manager.len();
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == NetworkServicesTab::Forwards => {
                manager.form = Some(NetworkServiceForm::Forward {
                    fields: [
                        String::new(),
                        String::new(),
                        "tcp".to_owned(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ],
                    field: 0,
                    editing: false,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == NetworkServicesTab::Forwards && !manager.forwards.is_empty() =>
            {
                let item = &manager.forwards[manager.selected];
                let port = item.ports.first().cloned().unwrap_or_default();
                manager.form = Some(NetworkServiceForm::Forward {
                    fields: [
                        item.listen_address.clone(),
                        item.description.clone(),
                        if port.protocol.is_empty() {
                            "tcp".to_owned()
                        } else {
                            port.protocol
                        },
                        port.listen_port,
                        port.target_address,
                        port.target_port,
                    ],
                    field: 1,
                    editing: true,
                });
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == NetworkServicesTab::Acls => {
                manager.form = Some(NetworkServiceForm::Acl {
                    name: String::new(),
                    description: String::new(),
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == NetworkServicesTab::Acls && !manager.acls.is_empty() =>
            {
                manager.acl_rules = Some(AclRulesForm {
                    acl: manager.acls[manager.selected].clone(),
                    ingress: true,
                    selected: 0,
                    editor: None,
                    delete_input: None,
                });
                AppCommand::None
            }
            KeyCode::Char('L')
                if manager.tab == NetworkServicesTab::Acls
                    && !manager.acls.is_empty()
                    && can_acl_log =>
            {
                AppCommand::ReadNetworkAclLog {
                    network: manager.network.clone(),
                    project: manager.project.clone(),
                    name: manager.acls[manager.selected].name.clone(),
                }
            }
            KeyCode::Char('L')
                if manager.tab == NetworkServicesTab::Acls && !manager.acls.is_empty() =>
            {
                self.model.last_error =
                    Some("ACL logs unavailable: server lacks network_acl_log".into());
                AppCommand::None
            }
            KeyCode::Char('R')
                if manager.tab == NetworkServicesTab::Acls && !manager.acls.is_empty() =>
            {
                manager.rename_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == NetworkServicesTab::LoadBalancers => {
                manager.form = Some(NetworkServiceForm::LoadBalancer {
                    fields: [
                        String::new(),
                        String::new(),
                        "tcp".to_owned(),
                        String::new(),
                        "backend1".to_owned(),
                        String::new(),
                        String::new(),
                        "false".to_owned(),
                        "10s".to_owned(),
                        "5s".to_owned(),
                        "3".to_owned(),
                        "2".to_owned(),
                    ],
                    field: 0,
                    editing: false,
                });
                AppCommand::None
            }
            KeyCode::Char('n') if manager.tab == NetworkServicesTab::Peers => {
                manager.form = Some(NetworkServiceForm::Peer {
                    fields: [
                        String::new(),
                        String::new(),
                        "local".to_owned(),
                        manager.project.clone(),
                        String::new(),
                        String::new(),
                    ],
                    field: 0,
                    editing: false,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == NetworkServicesTab::LoadBalancers
                    && !manager.load_balancers.is_empty() =>
            {
                let item = &manager.load_balancers[manager.selected];
                let backend = item.backends.first().cloned().unwrap_or_default();
                let port = item.ports.first().cloned().unwrap_or_default();
                manager.form = Some(NetworkServiceForm::LoadBalancer {
                    fields: [
                        item.listen_address.clone(),
                        item.description.clone(),
                        port.protocol,
                        port.listen_port,
                        backend.name,
                        backend.target_address,
                        backend.target_port,
                        item.config
                            .get("healthcheck")
                            .cloned()
                            .unwrap_or_else(|| "false".into()),
                        item.config
                            .get("healthcheck.interval")
                            .cloned()
                            .unwrap_or_else(|| "10s".into()),
                        item.config
                            .get("healthcheck.timeout")
                            .cloned()
                            .unwrap_or_else(|| "5s".into()),
                        item.config
                            .get("healthcheck.failure_count")
                            .cloned()
                            .unwrap_or_else(|| "3".into()),
                        item.config
                            .get("healthcheck.success_count")
                            .cloned()
                            .unwrap_or_else(|| "2".into()),
                    ],
                    field: 1,
                    editing: true,
                });
                AppCommand::None
            }
            KeyCode::Char('S')
                if manager.tab == NetworkServicesTab::LoadBalancers
                    && !manager.load_balancers.is_empty()
                    && can_lb_state =>
            {
                AppCommand::ReadNetworkLoadBalancerState {
                    network: manager.network.clone(),
                    project: manager.project.clone(),
                    address: manager.load_balancers[manager.selected]
                        .listen_address
                        .clone(),
                }
            }
            KeyCode::Char('S')
                if manager.tab == NetworkServicesTab::LoadBalancers
                    && !manager.load_balancers.is_empty() =>
            {
                self.model.last_error = Some(
                    "load-balancer state unavailable: server lacks network_load_balancer_state"
                        .into(),
                );
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter
                if manager.tab == NetworkServicesTab::Peers && !manager.peers.is_empty() =>
            {
                let item = &manager.peers[manager.selected];
                manager.form = Some(NetworkServiceForm::Peer {
                    fields: [
                        item.name.clone(),
                        item.description.clone(),
                        if item.peer_type.is_empty() {
                            "local".to_owned()
                        } else {
                            item.peer_type.clone()
                        },
                        item.target_project.clone(),
                        item.target_network.clone(),
                        item.target_integration.clone(),
                    ],
                    field: 1,
                    editing: true,
                });
                AppCommand::None
            }
            KeyCode::Char('N') if can_state => AppCommand::ReadNetworkState {
                network: manager.network.clone(),
                project: manager.project.clone(),
            },
            KeyCode::Char('N') => {
                self.model.last_error =
                    Some("network state unavailable: server lacks network_state".into());
                AppCommand::None
            }
            KeyCode::Char('d')
                if manager.tab != NetworkServicesTab::Leases && manager.len() > 0 =>
            {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_file_form(&mut self) {
        if let Some(instance) = self.model.instances.get(self.selected_instance) {
            self.file_form = Some(FileForm::new(instance));
            self.model.last_error = None;
        }
    }

    fn handle_file_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self
            .file_form
            .as_mut()
            .expect("file form checked before handling key");
        let delete_phrase = form.delete_phrase();
        let delete_name = form.instance_name.clone();
        let delete_project = form.project.clone();
        let delete_path = form.path.clone();
        if let Some(transfer) = form.transfer.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    form.transfer = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    transfer.local_path.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    transfer.local_path.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if transfer.local_path.trim().is_empty() => {
                    self.model.last_error = Some("local transfer path is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter if transfer.upload => AppCommand::PushFile {
                    name: form.instance_name.clone(),
                    project: form.project.clone(),
                    path: form.path.clone(),
                    local_path: transfer.local_path.trim().to_owned(),
                },
                KeyCode::Enter => AppCommand::PullFile {
                    name: form.instance_name.clone(),
                    project: form.project.clone(),
                    path: form.path.clone(),
                    local_path: transfer.local_path.trim().to_owned(),
                },
                _ => AppCommand::None,
            };
        }
        if let Some(confirmation) = form.delete_confirmation.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    form.delete_confirmation = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    confirmation.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    confirmation.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if *confirmation == delete_phrase => AppCommand::DeleteFile {
                    name: delete_name,
                    project: delete_project,
                    path: delete_path,
                },
                KeyCode::Enter => {
                    self.model.last_error = Some(format!(
                        "confirmation must exactly match: {}",
                        delete_phrase
                    ));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if form.browsing_directory {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('b') => {
                    form.browsing_directory = false;
                    AppCommand::None
                }
                KeyCode::Down | KeyCode::Char('j') if !form.directory_entries.is_empty() => {
                    form.directory_selected =
                        (form.directory_selected + 1) % form.directory_entries.len();
                    AppCommand::None
                }
                KeyCode::Up | KeyCode::Char('k') if !form.directory_entries.is_empty() => {
                    form.directory_selected =
                        (form.directory_selected + form.directory_entries.len() - 1)
                            % form.directory_entries.len();
                    AppCommand::None
                }
                KeyCode::Enter if !form.directory_entries.is_empty() => {
                    let child = &form.directory_entries[form.directory_selected];
                    form.path = if form.path == "/" {
                        format!("/{child}")
                    } else {
                        format!("{}/{}", form.path.trim_end_matches('/'), child)
                    };
                    form.browsing_directory = false;
                    form.loaded = false;
                    form.contents.clear();
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.file_form = None;
                AppCommand::None
            }
            KeyCode::Tab => {
                form.focus = if form.focus == FileFocus::Path {
                    FileFocus::Contents
                } else {
                    FileFocus::Path
                };
                AppCommand::None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                AppCommand::WriteFile {
                    name: form.instance_name.clone(),
                    project: form.project.clone(),
                    path: form.path.clone(),
                    contents: form.contents.clone(),
                }
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let file_name = std::path::Path::new(&form.path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("instance-file");
                form.transfer = Some(FileTransfer {
                    upload: false,
                    local_path: format!("./{file_name}"),
                });
                AppCommand::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.transfer = Some(FileTransfer {
                    upload: true,
                    local_path: "./file".to_owned(),
                });
                AppCommand::None
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.delete_confirmation = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('b') if form.focus == FileFocus::Path => AppCommand::ListDirectory {
                name: form.instance_name.clone(),
                project: form.project.clone(),
                path: form.path.clone(),
            },
            KeyCode::Enter if form.focus == FileFocus::Path => AppCommand::ReadFile {
                name: form.instance_name.clone(),
                project: form.project.clone(),
                path: form.path.clone(),
            },
            KeyCode::Enter => {
                form.contents.push('\n');
                AppCommand::None
            }
            KeyCode::Backspace => {
                if form.focus == FileFocus::Path {
                    form.path.pop();
                } else {
                    form.contents.pop();
                }
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if form.focus == FileFocus::Path {
                    form.path.push(character);
                } else {
                    form.contents.push(character);
                }
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn open_operation_confirmation(&mut self) {
        if let Some(operation) = self.model.operations.get(self.selected_operation) {
            if !operation.may_cancel {
                self.model.last_error = Some("selected operation cannot be cancelled".to_owned());
                return;
            }
            let description = if operation.description.is_empty() {
                operation.class.clone()
            } else {
                operation.description.clone()
            };
            self.operation_confirmation = Some(OperationConfirmation::new(
                operation.id.clone(),
                description,
            ));
            self.model.last_error = None;
        }
    }

    fn open_operation_detail(&mut self) {
        if let Some(operation) = self.model.operations.get(self.selected_operation) {
            self.operation_detail = Some(OperationDetailForm {
                operation: operation.clone(),
            });
            self.model.last_error = None;
        }
    }

    fn handle_operation_detail_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.operation_detail = None;
                AppCommand::None
            }
            KeyCode::Char('c') => {
                let operation = self
                    .operation_detail
                    .take()
                    .expect("operation detail remains present");
                if operation.operation.may_cancel {
                    let description = if operation.operation.description.is_empty() {
                        operation.operation.class.clone()
                    } else {
                        operation.operation.description.clone()
                    };
                    self.operation_confirmation = Some(OperationConfirmation::new(
                        operation.operation.id,
                        description,
                    ));
                } else {
                    self.model.last_error =
                        Some("selected operation cannot be cancelled".to_owned());
                }
                AppCommand::None
            }
            KeyCode::Char('g') => {
                let resource = self
                    .operation_detail
                    .as_ref()
                    .and_then(|form| form.operation.resources.values().flatten().next())
                    .cloned();
                self.operation_detail = None;
                if let Some(resource) = resource {
                    self.jump_to_resource(&resource);
                } else {
                    self.model.last_error =
                        Some("operation reports no affected resource".to_owned());
                }
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn jump_to_resource(&mut self, resource: &str) {
        let (path, query) = resource.split_once('?').unwrap_or((resource, ""));
        let segments = path
            .split('/')
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let Some(kind_index) = segments
            .iter()
            .position(|value| *value == "1.0")
            .map(|index| index + 1)
        else {
            self.model.last_error = Some(format!("cannot parse affected resource: {resource}"));
            return;
        };
        let kind = segments.get(kind_index).copied().unwrap_or_default();
        let name = segments.get(kind_index + 1).copied().unwrap_or_default();
        let project = url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "project")
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        let matched = match kind {
            "instances" | "containers" | "virtual-machines" => {
                self.page = Page::Instances;
                self.model
                    .instances
                    .iter()
                    .position(|value| {
                        value.name == name && (project.is_empty() || value.project == project)
                    })
                    .map(|index| self.selected_instance = index)
                    .is_some()
            }
            "operations" => {
                self.page = Page::Operations;
                self.model
                    .operations
                    .iter()
                    .position(|value| value.id == name)
                    .map(|index| self.selected_operation = index)
                    .is_some()
            }
            "networks" => {
                self.page = Page::Networks;
                self.model
                    .networks
                    .iter()
                    .position(|value| {
                        value.name == name && (project.is_empty() || value.project == project)
                    })
                    .map(|index| self.selected_network = index)
                    .is_some()
            }
            "storage-pools" => {
                self.page = Page::Storage;
                self.model
                    .storage_pools
                    .iter()
                    .position(|value| value.name == name)
                    .map(|index| self.selected_storage_pool = index)
                    .is_some()
            }
            "images" => {
                self.page = Page::Images;
                self.model
                    .images
                    .iter()
                    .position(|value| {
                        value.fingerprint.starts_with(name)
                            && (project.is_empty() || value.project == project)
                    })
                    .map(|index| self.selected_image = index)
                    .is_some()
            }
            "projects" => {
                self.page = Page::Projects;
                self.model
                    .projects
                    .iter()
                    .position(|value| value.name == name)
                    .map(|index| self.selected_project = index)
                    .is_some()
            }
            "profiles" => {
                self.page = Page::Projects;
                self.model
                    .profiles
                    .iter()
                    .position(|value| {
                        value.name == name && (project.is_empty() || value.project == project)
                    })
                    .map(|index| self.selected_profile = index)
                    .is_some()
            }
            _ => false,
        };
        if !matched {
            self.model.last_error = Some(format!("affected resource is not loaded: {resource}"));
        } else {
            self.model.last_error = None;
        }
    }

    fn handle_operation_confirmation_key(&mut self, key: KeyEvent) -> AppCommand {
        let confirmation = self
            .operation_confirmation
            .as_mut()
            .expect("operation confirmation checked before handling key");
        match key.code {
            KeyCode::Esc => {
                self.operation_confirmation = None;
                AppCommand::None
            }
            KeyCode::Backspace => {
                confirmation.input.pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                confirmation.input.push(character);
                AppCommand::None
            }
            KeyCode::Enter if confirmation.input == confirmation.expected_phrase() => {
                let id = self
                    .operation_confirmation
                    .take()
                    .expect("confirmation remains present until enter")
                    .operation_id;
                AppCommand::CancelOperation { id }
            }
            KeyCode::Enter => {
                self.model.last_error = Some(format!(
                    "confirmation must exactly match: {}",
                    confirmation.expected_phrase()
                ));
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_connection_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self.connection_form.as_mut().expect("connection form");
        match key.code {
            KeyCode::Esc => {
                self.connection_form = None;
                AppCommand::None
            }
            KeyCode::Char('m') | KeyCode::Char(' ') if form.field == 0 => {
                form.remote = !form.remote;
                form.fields[0] = if form.remote {
                    "https://127.0.0.1:8443".to_owned()
                } else {
                    "/var/lib/incus/unix.socket".to_owned()
                };
                form.fields[4] = if form.remote { "remote" } else { "local" }.to_owned();
                AppCommand::None
            }
            KeyCode::Tab => {
                form.field = (form.field + 1) % 5;
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.field = (form.field + 4) % 5;
                AppCommand::None
            }
            KeyCode::Up if !form.saved.is_empty() => {
                form.selected_saved =
                    (form.selected_saved + form.saved.len() - 1) % form.saved.len();
                form.load_selected();
                AppCommand::None
            }
            KeyCode::Down if !form.saved.is_empty() => {
                form.selected_saved = (form.selected_saved + 1) % form.saved.len();
                form.load_selected();
                AppCommand::None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let name = form.fields[4].trim();
                if name.is_empty() {
                    self.model.last_error = Some("saved remote name is required".into());
                    return AppCommand::None;
                }
                let entry = SavedRemote {
                    name: name.to_owned(),
                    transport: form.transport(),
                };
                if let Some(existing) = form.saved.iter_mut().find(|saved| saved.name == name) {
                    *existing = entry;
                } else {
                    form.saved.push(entry);
                    form.selected_saved = form.saved.len() - 1;
                }
                self.saved_remotes = form.saved.clone();
                AppCommand::PersistRemotes(form.saved.clone())
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL) && !form.saved.is_empty() =>
            {
                form.saved.remove(form.selected_saved);
                form.selected_saved = form.selected_saved.min(form.saved.len().saturating_sub(1));
                self.saved_remotes = form.saved.clone();
                AppCommand::PersistRemotes(form.saved.clone())
            }
            KeyCode::Backspace => {
                form.fields[form.field].pop();
                AppCommand::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.field].push(c);
                AppCommand::None
            }
            KeyCode::Enter if form.fields[0].trim().is_empty() => {
                self.model.last_error = Some("connection endpoint is required".to_owned());
                AppCommand::None
            }
            KeyCode::Enter
                if form.remote && (form.fields[1].is_empty() != form.fields[2].is_empty()) =>
            {
                self.model.last_error =
                    Some("remote TLS requires both client certificate and key".to_owned());
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::Connect(form.transport()),
            _ => AppCommand::None,
        }
    }

    fn handle_cluster_group_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self
            .cluster_group_manager
            .as_mut()
            .expect("cluster-group manager");
        if let Some(input) = manager.rename_input.as_mut() {
            let current = manager
                .groups
                .get(manager.selected)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            return match key.code {
                KeyCode::Esc => {
                    manager.rename_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if !input.trim().is_empty() && input.trim() != current => {
                    AppCommand::RenameClusterGroup {
                        name: current,
                        new_name: input.trim().into(),
                    }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some("new group name must be non-empty and different".into());
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if manager.delete_input.is_some() {
            let expected = manager.delete_phrase();
            let input = manager.delete_input.as_mut().expect("delete input");
            return match key.code {
                KeyCode::Esc => {
                    manager.delete_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter if *input == expected => {
                    let name = manager
                        .groups
                        .get(manager.selected)
                        .map(|v| v.name.clone())
                        .unwrap_or_default();
                    manager.delete_input = None;
                    AppCommand::DeleteClusterGroup { name }
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        if let Some(editor) = manager.editor.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.editor = None;
                    AppCommand::None
                }
                KeyCode::Tab | KeyCode::Enter if editor.field < 2 => {
                    editor.field += 1;
                    AppCommand::None
                }
                KeyCode::BackTab => {
                    editor.field = (editor.field + 2) % 3;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    editor.fields[editor.field].pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    editor.fields[editor.field].push(c);
                    AppCommand::None
                }
                KeyCode::Enter if editor.fields[0].trim().is_empty() => {
                    self.model.last_error = Some("cluster-group name is required".to_owned());
                    AppCommand::None
                }
                KeyCode::Enter => {
                    let members = editor.fields[2]
                        .split(',')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToOwned::to_owned)
                        .collect();
                    if editor.editing {
                        AppCommand::UpdateClusterGroup {
                            name: manager.groups[manager.selected].name.clone(),
                            description: editor.fields[1].trim().to_owned(),
                            members,
                        }
                    } else {
                        AppCommand::CreateClusterGroup {
                            name: editor.fields[0].trim().to_owned(),
                            description: editor.fields[1].trim().to_owned(),
                            members,
                        }
                    }
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc => {
                self.cluster_group_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.groups.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.groups.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.groups.is_empty() => {
                manager.selected =
                    (manager.selected + manager.groups.len() - 1) % manager.groups.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.editor = Some(ClusterGroupEditor {
                    editing: false,
                    fields: [String::new(), String::new(), String::new()],
                    field: 0,
                });
                AppCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter if !manager.groups.is_empty() => {
                let group = &manager.groups[manager.selected];
                manager.editor = Some(ClusterGroupEditor {
                    editing: true,
                    fields: [
                        group.name.clone(),
                        group.description.clone(),
                        group.members.join(","),
                    ],
                    field: 1,
                });
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.groups.is_empty() => {
                manager.delete_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('R') if !manager.groups.is_empty() => {
                manager.rename_input = Some(String::new());
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_move_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self.move_form.as_mut().expect("move form");
        match key.code {
            KeyCode::Esc => {
                self.move_form = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter if form.field < 4 => {
                form.field += 1;
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.field = (form.field + 4) % 5;
                AppCommand::None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 4 => {
                form.spec.live = !form.spec.live;
                AppCommand::None
            }
            KeyCode::Backspace if form.field < 4 => {
                match form.field {
                    0 => {
                        form.spec.new_name.pop();
                    }
                    1 => {
                        form.spec.target_project.pop();
                    }
                    2 => {
                        form.spec.target_member.pop();
                    }
                    _ => {
                        form.spec.target_pool.pop();
                    }
                }
                AppCommand::None
            }
            KeyCode::Char(c)
                if form.field < 4 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                match form.field {
                    0 => form.spec.new_name.push(c),
                    1 => form.spec.target_project.push(c),
                    2 => form.spec.target_member.push(c),
                    _ => form.spec.target_pool.push(c),
                }
                AppCommand::None
            }
            KeyCode::Enter if form.spec.new_name.trim().is_empty() => {
                self.model.last_error = Some("migration target name is required".to_owned());
                AppCommand::None
            }
            KeyCode::Enter => AppCommand::MoveInstance(form.spec.clone()),
            _ => AppCommand::None,
        }
    }

    fn handle_project_usage_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.project_usage = None;
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::ReadProjectUsage {
                project: self
                    .project_usage
                    .as_ref()
                    .expect("project usage")
                    .project
                    .clone(),
            },
            _ => AppCommand::None,
        }
    }

    fn handle_project_access_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.project_access = None;
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::ReadProjectAccess {
                project: self
                    .project_access
                    .as_ref()
                    .expect("project access")
                    .project
                    .clone(),
            },
            _ => AppCommand::None,
        }
    }

    fn handle_instance_access_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.instance_access = None;
                AppCommand::None
            }
            KeyCode::Char('r') => {
                let form = self.instance_access.as_ref().expect("instance access");
                AppCommand::ReadInstanceAccess {
                    name: form.name.clone(),
                    project: form.project.clone(),
                }
            }
            _ => AppCommand::None,
        }
    }

    fn open_action_menu(&mut self) {
        let entries = match self.page {
            Page::Instances => vec![
                ("Details", KeyCode::Enter),
                ("Create", KeyCode::Char('n')),
                ("Run command", KeyCode::Char('e')),
                ("Interactive shell", KeyCode::Char('I')),
                ("Serial console", KeyCode::Char('V')),
                ("VGA console", KeyCode::Char('C')),
                ("Configure", KeyCode::Char('E')),
                ("Snapshot policy", KeyCode::Char('O')),
                ("Placement eligibility", KeyCode::Char('y')),
                ("Start", KeyCode::Char('s')),
                ("Stop", KeyCode::Char('X')),
                ("Restart", KeyCode::Char('R')),
                ("Freeze", KeyCode::Char('f')),
                ("Unfreeze", KeyCode::Char('u')),
                ("Console", KeyCode::Char('v')),
                ("Files", KeyCode::Char('o')),
                ("Snapshots", KeyCode::Char('p')),
                ("Backups", KeyCode::Char('b')),
                ("Artifacts", KeyCode::Char('m')),
                ("Rename", KeyCode::Char('M')),
                ("Move / migrate", KeyCode::Char('T')),
                ("Devices", KeyCode::Char('g')),
                ("Publish image", KeyCode::Char('P')),
                ("Delete", KeyCode::Char('d')),
            ],
            Page::Operations => vec![
                ("Details", KeyCode::Enter),
                ("Cancel", KeyCode::Char('c')),
                ("Export audit", KeyCode::Char('X')),
            ],
            Page::Events => vec![("Export audit", KeyCode::Char('X'))],
            Page::Storage => vec![
                ("Create pool", KeyCode::Char('n')),
                ("Configure pool", KeyCode::Char('E')),
                ("Volumes", KeyCode::Char('v')),
                ("Buckets", KeyCode::Char('b')),
                ("Delete pool", KeyCode::Char('d')),
            ],
            Page::Networks => vec![
                ("Create network", KeyCode::Char('n')),
                ("Configure", KeyCode::Char('E')),
                ("Services", KeyCode::Char('X')),
                ("DNS zones", KeyCode::Char('z')),
                ("BGP", KeyCode::Char('B')),
                ("Address sets", KeyCode::Char('A')),
                ("Integrations", KeyCode::Char('N')),
                ("Delete network", KeyCode::Char('d')),
            ],
            Page::Images => vec![
                ("Launch", KeyCode::Enter),
                ("Edit", KeyCode::Char('e')),
                ("Auto-update policy", KeyCode::Char('O')),
                ("Cache retention preview", KeyCode::Char('V')),
                ("Import", KeyCode::Char('i')),
                ("Export", KeyCode::Char('o')),
                ("Add alias", KeyCode::Char('a')),
                ("Manage aliases", KeyCode::Char('A')),
                ("Delete", KeyCode::Char('d')),
            ],
            Page::Projects => vec![
                ("Create project", KeyCode::Char('n')),
                ("Configure project", KeyCode::Char('E')),
                ("Project resource usage", KeyCode::Char('U')),
                ("Delete project", KeyCode::Char('d')),
                ("Create profile", KeyCode::Char('N')),
                ("Configure profile", KeyCode::Char('P')),
                ("Profile devices", KeyCode::Char('G')),
                ("Delete profile", KeyCode::Char('D')),
            ],
            Page::Settings => vec![
                ("Connect target", KeyCode::Char('c')),
                ("Authentication & authorization", KeyCode::Char('A')),
                ("Local action journal", KeyCode::Char('J')),
                ("Fleet health & drift", KeyCode::Char('F')),
                ("OpenFGA relationships", KeyCode::Char('G')),
                ("Image refresh policy", KeyCode::Char('I')),
                ("Cluster groups", KeyCode::Char('K')),
                ("Server configuration", KeyCode::Char('E')),
                ("Administration", KeyCode::Char('s')),
            ],
            _ => vec![("Refresh", KeyCode::Char('r'))],
        };
        self.action_menu = Some(ActionMenu {
            title: format!("{} actions", self.page.title()),
            entries: entries
                .into_iter()
                .map(|(label, key)| ActionMenuEntry { label, key })
                .collect(),
            selected: 0,
        });
    }

    fn handle_action_menu_key(&mut self, key: KeyEvent) -> AppCommand {
        let menu = self.action_menu.as_mut().expect("action menu");
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => {
                self.action_menu = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !menu.entries.is_empty() => {
                menu.selected = (menu.selected + 1) % menu.entries.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !menu.entries.is_empty() => {
                menu.selected = (menu.selected + menu.entries.len() - 1) % menu.entries.len();
                AppCommand::None
            }
            KeyCode::Enter if !menu.entries.is_empty() => {
                let action = menu.entries[menu.selected].key;
                self.action_menu = None;
                self.handle_key(KeyEvent::new(action, KeyModifiers::NONE))
            }
            _ => AppCommand::None,
        }
    }

    fn handle_audit_export_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self.audit_export.as_mut().expect("audit export form");
        match key.code {
            KeyCode::Esc => {
                self.audit_export = None;
                AppCommand::None
            }
            KeyCode::Tab | KeyCode::Enter if form.field < 4 => {
                form.field += 1;
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.field = (form.field + 4) % 5;
                AppCommand::None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field >= 2 => {
                match form.field {
                    2 => form.include_events = !form.include_events,
                    3 => form.include_operations = !form.include_operations,
                    _ => form.redact = !form.redact,
                }
                AppCommand::None
            }
            KeyCode::Backspace if form.field < 2 => {
                if form.field == 0 {
                    form.path.pop();
                } else {
                    form.filter.pop();
                }
                AppCommand::None
            }
            KeyCode::Char(c)
                if form.field < 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if form.field == 0 {
                    form.path.push(c);
                } else {
                    form.filter.push(c);
                }
                AppCommand::None
            }
            KeyCode::Enter if form.path.trim().is_empty() => {
                self.model.last_error = Some("audit export destination is required".into());
                AppCommand::None
            }
            KeyCode::Enter if !form.include_events && !form.include_operations => {
                self.model.last_error = Some("select events, operations, or both".into());
                AppCommand::None
            }
            KeyCode::Enter => {
                let value = build_audit_export(&self.model, form);
                AppCommand::ExportAudit {
                    path: form.path.trim().into(),
                    value,
                }
            }
            _ => AppCommand::None,
        }
    }

    fn handle_journal_key(&mut self, key: KeyEvent) -> AppCommand {
        let len = self.journal_rows().len();
        let viewer = self.journal_viewer.as_mut().expect("journal viewer");
        if viewer.filtering {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => viewer.filtering = false,
                KeyCode::Backspace => {
                    viewer.filter.pop();
                    viewer.selected = 0;
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    viewer.filter.push(character);
                    viewer.selected = 0;
                }
                _ => {}
            }
            return AppCommand::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => self.journal_viewer = None,
            KeyCode::Char('/') => viewer.filtering = true,
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                viewer.selected = (viewer.selected + 1) % len;
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                viewer.selected = (viewer.selected + len - 1) % len;
            }
            _ => {}
        }
        AppCommand::None
    }

    fn handle_fleet_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self.fleet_manager.as_mut().expect("fleet manager");
        let len = manager.rows.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => self.fleet_manager = None,
            KeyCode::Char('r') => {
                manager.loading = true;
                return AppCommand::ReadFleetHealth(self.saved_remotes.clone());
            }
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                manager.selected = (manager.selected + 1) % len;
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                manager.selected = (manager.selected + len - 1) % len;
            }
            KeyCode::Enter if len > 0 => {
                let name = manager.rows[manager.selected].name.clone();
                if let Some(remote) = self.saved_remotes.iter().find(|remote| remote.name == name) {
                    return AppCommand::Connect(remote.transport.clone());
                }
            }
            _ => {}
        }
        AppCommand::None
    }

    fn handle_placement_inspector_key(&mut self, key: KeyEvent) -> AppCommand {
        let inspector = self
            .placement_inspector
            .as_mut()
            .expect("placement inspector");
        let len = inspector.members.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => self.placement_inspector = None,
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                inspector.selected = (inspector.selected + 1) % len;
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                inspector.selected = (inspector.selected + len - 1) % len;
            }
            _ => {}
        }
        AppCommand::None
    }

    fn handle_backup_schedule_key(&mut self, key: KeyEvent) -> AppCommand {
        let form = self.backup_schedule_form.as_mut().expect("backup schedule");
        match key.code {
            KeyCode::Esc => {
                self.backup_schedule_form = None;
                AppCommand::None
            }
            KeyCode::Delete => {
                let instance = form.schedule.instance.clone();
                let project = form.schedule.project.clone();
                self.backup_schedules.retain(|schedule| {
                    schedule.instance != instance || schedule.project != project
                });
                self.backup_schedule_form = None;
                AppCommand::PersistSchedules(self.backup_schedules.clone())
            }
            KeyCode::Enter if form.field == 5 => {
                if form.schedule.name_pattern.trim().is_empty() {
                    self.model.last_error = Some("backup name pattern is required".into());
                    return AppCommand::None;
                }
                let schedule = form.schedule.clone();
                self.backup_schedules.retain(|existing| {
                    existing.instance != schedule.instance || existing.project != schedule.project
                });
                self.backup_schedules.push(schedule);
                self.backup_schedule_form = None;
                AppCommand::PersistSchedules(self.backup_schedules.clone())
            }
            KeyCode::Tab | KeyCode::Enter => {
                form.field = (form.field + 1) % 6;
                AppCommand::None
            }
            KeyCode::BackTab => {
                form.field = (form.field + 5) % 6;
                AppCommand::None
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                match form.field {
                    0 => form.schedule.enabled = !form.schedule.enabled,
                    1 => {
                        form.schedule.interval_seconds = match form.schedule.interval_seconds {
                            3600 => 21600,
                            21600 => 86400,
                            86400 => 604800,
                            _ => 3600,
                        }
                    }
                    3 => form.schedule.instance_only = !form.schedule.instance_only,
                    4 => form.schedule.optimized_storage = !form.schedule.optimized_storage,
                    _ => {}
                }
                AppCommand::None
            }
            KeyCode::Backspace if form.field == 2 => {
                form.schedule.name_pattern.pop();
                AppCommand::None
            }
            KeyCode::Char(character)
                if form.field == 2 && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                form.schedule.name_pattern.push(character);
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_openfga_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self.openfga_manager.as_mut().expect("OpenFGA manager");
        let tuple = || TupleKey {
            user: format!("{}:{}", manager.fields[0].trim(), manager.fields[1].trim()),
            relation: manager.fields[2].trim().to_owned(),
            object: format!("{}:{}", manager.fields[3].trim(), manager.fields[4].trim()),
        };
        let expected = format!("{} relationship", manager.operation());
        if manager.confirming {
            return match key.code {
                KeyCode::Esc => {
                    manager.confirmation.clear();
                    manager.confirming = false;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    manager.confirmation.pop();
                    AppCommand::None
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    manager.confirmation.push(character);
                    AppCommand::None
                }
                KeyCode::Enter if manager.confirmation == expected => {
                    let command = AppCommand::MutateOpenFga {
                        tuple: tuple(),
                        delete: manager.operation == 2,
                    };
                    manager.confirmation.clear();
                    manager.confirming = false;
                    command
                }
                KeyCode::Enter => {
                    self.model.last_error =
                        Some(format!("confirmation must exactly match: {expected}"));
                    AppCommand::None
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => {
                self.openfga_manager = None;
                AppCommand::None
            }
            KeyCode::Char('r') => AppCommand::RefreshOpenFga,
            KeyCode::F(2) => {
                manager.operation = (manager.operation + 1) % 3;
                AppCommand::None
            }
            KeyCode::Tab => {
                manager.field = (manager.field + 1) % 5;
                AppCommand::None
            }
            KeyCode::BackTab => {
                manager.field = (manager.field + 4) % 5;
                AppCommand::None
            }
            KeyCode::Down | KeyCode::Char('j') if !manager.tuples.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.tuples.len();
                AppCommand::None
            }
            KeyCode::Up | KeyCode::Char('k') if !manager.tuples.is_empty() => {
                manager.selected =
                    (manager.selected + manager.tuples.len() - 1) % manager.tuples.len();
                AppCommand::None
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                if matches!(manager.field, 0 | 2 | 3) =>
            {
                let choices: &[&str] = match manager.field {
                    0 => &["user", "group"],
                    2 => &[
                        "viewer",
                        "user",
                        "operator",
                        "admin",
                        "member",
                        "can_exec",
                        "can_access_console",
                        "can_access_files",
                        "can_manage_backups",
                        "can_manage_snapshots",
                        "can_update_state",
                        "can_edit",
                    ],
                    _ => &[
                        "server",
                        "project",
                        "instance",
                        "image",
                        "network",
                        "storage_pool",
                        "storage_volume",
                        "profile",
                        "certificate",
                        "group",
                    ],
                };
                let current = choices
                    .iter()
                    .position(|choice| *choice == manager.fields[manager.field])
                    .unwrap_or(0);
                manager.fields[manager.field] = choices[(current + 1) % choices.len()].into();
                AppCommand::None
            }
            KeyCode::Backspace => {
                manager.fields[manager.field].pop();
                AppCommand::None
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                manager.fields[manager.field].push(character);
                AppCommand::None
            }
            KeyCode::Enter if manager.fields.iter().any(|field| field.trim().is_empty()) => {
                self.model.last_error = Some("all relationship fields are required".into());
                AppCommand::None
            }
            KeyCode::Enter if manager.operation == 0 => AppCommand::CheckOpenFga { tuple: tuple() },
            KeyCode::Enter => {
                manager.confirmation.clear();
                manager.confirming = true;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_image_retention_key(&mut self, key: KeyEvent) -> AppCommand {
        let preview = self
            .image_retention_preview
            .as_mut()
            .expect("image retention preview");
        let len = preview.rows.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => self.image_retention_preview = None,
            KeyCode::Char('r') => return AppCommand::ReadImageRetention,
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                preview.selected = (preview.selected + 1) % len
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                preview.selected = (preview.selected + len - 1) % len
            }
            _ => {}
        }
        AppCommand::None
    }

    pub async fn export_audit(&mut self, path: String, value: serde_json::Value) {
        let result = async {
            let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
            tokio::fs::write(&path, bytes)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        match result {
            Ok(()) => {
                self.audit_export = None;
                self.model.last_error = None;
            }
            Err(error) => {
                self.model.last_error = Some(format!("cannot export audit data: {error}"))
            }
        }
    }

    fn handle_saved_view_key(&mut self, key: KeyEvent) -> AppCommand {
        let manager = self.view_manager.as_mut().expect("saved view manager");
        if let Some(input) = manager.name_input.as_mut() {
            return match key.code {
                KeyCode::Esc => {
                    manager.name_input = None;
                    AppCommand::None
                }
                KeyCode::Backspace => {
                    input.pop();
                    AppCommand::None
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    AppCommand::None
                }
                KeyCode::Enter | KeyCode::Char('s')
                    if key.code == KeyCode::Enter
                        || key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    let name = input.trim();
                    if name.is_empty() {
                        self.model.last_error = Some("saved view name is required".into());
                        return AppCommand::None;
                    }
                    let view = SavedView {
                        name: name.into(),
                        page: self.page.title().into(),
                        filter: self.filter_query.clone(),
                    };
                    if let Some(existing) = manager
                        .views
                        .iter_mut()
                        .find(|existing| existing.name == name)
                    {
                        *existing = view;
                    } else {
                        manager.views.push(view);
                        manager.selected = manager.views.len() - 1;
                    }
                    manager.name_input = None;
                    self.saved_views = manager.views.clone();
                    AppCommand::PersistViews(manager.views.clone())
                }
                _ => AppCommand::None,
            };
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('\'') => {
                self.view_manager = None;
                AppCommand::None
            }
            KeyCode::Char('j') | KeyCode::Down if !manager.views.is_empty() => {
                manager.selected = (manager.selected + 1) % manager.views.len();
                AppCommand::None
            }
            KeyCode::Char('k') | KeyCode::Up if !manager.views.is_empty() => {
                manager.selected =
                    (manager.selected + manager.views.len() - 1) % manager.views.len();
                AppCommand::None
            }
            KeyCode::Char('n') => {
                manager.name_input = Some(String::new());
                AppCommand::None
            }
            KeyCode::Char('d') if !manager.views.is_empty() => {
                manager.views.remove(manager.selected);
                manager.selected = manager.selected.min(manager.views.len().saturating_sub(1));
                self.saved_views = manager.views.clone();
                AppCommand::PersistViews(manager.views.clone())
            }
            KeyCode::Enter if !manager.views.is_empty() => {
                let view = manager.views[manager.selected].clone();
                if let Some(page) = Page::ALL.into_iter().find(|page| page.title() == view.page) {
                    self.page = page;
                }
                self.filter_query = view.filter;
                self.select_first_match();
                self.view_manager = None;
                AppCommand::None
            }
            _ => AppCommand::None,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> AppCommand {
        match key.code {
            KeyCode::Esc => self.filter_form = None,
            KeyCode::Enter => {
                self.filter_query = self
                    .filter_form
                    .take()
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                self.select_first_match();
            }
            KeyCode::Backspace => {
                self.filter_form.as_mut().expect("filter form").pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_form.as_mut().expect("filter form").push(c);
            }
            _ => {}
        }
        AppCommand::None
    }

    fn select_first_match(&mut self) {
        match self.page {
            Page::Instances => {
                self.selected_instance = self
                    .model
                    .instances
                    .iter()
                    .position(|v| {
                        self.matches_filter([
                            v.name.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                            v.instance_type.as_str(),
                        ])
                    })
                    .unwrap_or(0)
            }
            Page::Operations => {
                self.selected_operation = self
                    .model
                    .operations
                    .iter()
                    .position(|v| {
                        self.matches_filter([
                            v.id.as_str(),
                            v.description.as_str(),
                            v.status.as_str(),
                            v.location.as_str(),
                        ])
                    })
                    .unwrap_or(0)
            }
            Page::Storage => {
                self.selected_storage_pool = self
                    .model
                    .storage_pools
                    .iter()
                    .position(|v| {
                        self.matches_filter([
                            v.name.as_str(),
                            v.driver.as_str(),
                            v.status.as_str(),
                            v.description.as_str(),
                        ])
                    })
                    .unwrap_or(0)
            }
            Page::Networks => {
                self.selected_network = self
                    .model
                    .networks
                    .iter()
                    .position(|v| {
                        self.matches_filter([
                            v.name.as_str(),
                            v.network_type.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                        ])
                    })
                    .unwrap_or(0)
            }
            Page::Images => {
                self.selected_image = self
                    .model
                    .images
                    .iter()
                    .position(|v| {
                        self.matches_filter([
                            v.fingerprint.as_str(),
                            v.architecture.as_str(),
                            v.image_type.as_str(),
                            v.project.as_str(),
                        ]) || v
                            .aliases
                            .iter()
                            .any(|a| self.matches_filter([a.name.as_str()]))
                    })
                    .unwrap_or(0)
            }
            Page::Projects => {
                self.selected_project = self
                    .model
                    .projects
                    .iter()
                    .position(|v| self.matches_filter([v.name.as_str(), v.description.as_str()]))
                    .unwrap_or(0)
            }
            _ => {}
        }
    }

    fn select_next(&mut self) {
        match self.page {
            Page::Operations if !self.model.operations.is_empty() => {
                self.selected_operation = cycle_matching(
                    self.selected_operation,
                    self.model.operations.len(),
                    true,
                    |i| {
                        let v = &self.model.operations[i];
                        self.matches_filter([
                            v.id.as_str(),
                            v.description.as_str(),
                            v.status.as_str(),
                            v.location.as_str(),
                        ])
                    },
                );
            }
            Page::Storage if !self.model.storage_pools.is_empty() => {
                self.selected_storage_pool = cycle_matching(
                    self.selected_storage_pool,
                    self.model.storage_pools.len(),
                    true,
                    |i| {
                        let v = &self.model.storage_pools[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.driver.as_str(),
                            v.status.as_str(),
                            v.description.as_str(),
                        ])
                    },
                );
            }
            Page::Networks if !self.model.networks.is_empty() => {
                self.selected_network = cycle_matching(
                    self.selected_network,
                    self.model.networks.len(),
                    true,
                    |i| {
                        let v = &self.model.networks[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.network_type.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                        ])
                    },
                );
            }
            Page::Projects if !self.model.projects.is_empty() => {
                self.selected_project = cycle_matching(
                    self.selected_project,
                    self.model.projects.len(),
                    true,
                    |i| {
                        let v = &self.model.projects[i];
                        self.matches_filter([v.name.as_str(), v.description.as_str()])
                    },
                );
            }
            Page::Images if !self.model.images.is_empty() => {
                self.selected_image =
                    cycle_matching(self.selected_image, self.model.images.len(), true, |i| {
                        let v = &self.model.images[i];
                        self.matches_filter([
                            v.fingerprint.as_str(),
                            v.architecture.as_str(),
                            v.image_type.as_str(),
                            v.project.as_str(),
                        ]) || v
                            .aliases
                            .iter()
                            .any(|a| self.matches_filter([a.name.as_str()]))
                    });
            }
            _ if !self.model.instances.is_empty() => {
                self.selected_instance = cycle_matching(
                    self.selected_instance,
                    self.model.instances.len(),
                    true,
                    |i| {
                        let v = &self.model.instances[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                            v.instance_type.as_str(),
                        ])
                    },
                );
            }
            _ => {}
        }
    }

    fn select_previous(&mut self) {
        match self.page {
            Page::Operations if !self.model.operations.is_empty() => {
                self.selected_operation = cycle_matching(
                    self.selected_operation,
                    self.model.operations.len(),
                    false,
                    |i| {
                        let v = &self.model.operations[i];
                        self.matches_filter([
                            v.id.as_str(),
                            v.description.as_str(),
                            v.status.as_str(),
                            v.location.as_str(),
                        ])
                    },
                );
            }
            Page::Storage if !self.model.storage_pools.is_empty() => {
                self.selected_storage_pool = cycle_matching(
                    self.selected_storage_pool,
                    self.model.storage_pools.len(),
                    false,
                    |i| {
                        let v = &self.model.storage_pools[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.driver.as_str(),
                            v.status.as_str(),
                            v.description.as_str(),
                        ])
                    },
                );
            }
            Page::Networks if !self.model.networks.is_empty() => {
                self.selected_network = cycle_matching(
                    self.selected_network,
                    self.model.networks.len(),
                    false,
                    |i| {
                        let v = &self.model.networks[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.network_type.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                        ])
                    },
                );
            }
            Page::Projects if !self.model.projects.is_empty() => {
                self.selected_project = cycle_matching(
                    self.selected_project,
                    self.model.projects.len(),
                    false,
                    |i| {
                        let v = &self.model.projects[i];
                        self.matches_filter([v.name.as_str(), v.description.as_str()])
                    },
                );
            }
            Page::Images if !self.model.images.is_empty() => {
                self.selected_image =
                    cycle_matching(self.selected_image, self.model.images.len(), false, |i| {
                        let v = &self.model.images[i];
                        self.matches_filter([
                            v.fingerprint.as_str(),
                            v.architecture.as_str(),
                            v.image_type.as_str(),
                            v.project.as_str(),
                        ]) || v
                            .aliases
                            .iter()
                            .any(|a| self.matches_filter([a.name.as_str()]))
                    });
            }
            _ if !self.model.instances.is_empty() => {
                self.selected_instance = cycle_matching(
                    self.selected_instance,
                    self.model.instances.len(),
                    false,
                    |i| {
                        let v = &self.model.instances[i];
                        self.matches_filter([
                            v.name.as_str(),
                            v.status.as_str(),
                            v.project.as_str(),
                            v.location.as_str(),
                            v.instance_type.as_str(),
                        ])
                    },
                );
            }
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_instance = self
            .selected_instance
            .min(self.model.instances.len().saturating_sub(1));
        self.selected_operation = self
            .selected_operation
            .min(self.model.operations.len().saturating_sub(1));
        self.selected_storage_pool = self
            .selected_storage_pool
            .min(self.model.storage_pools.len().saturating_sub(1));
        self.selected_network = self
            .selected_network
            .min(self.model.networks.len().saturating_sub(1));
        self.selected_project = self
            .selected_project
            .min(self.model.projects.len().saturating_sub(1));
        self.selected_profile = self
            .selected_profile
            .min(self.model.profiles.len().saturating_sub(1));
        self.selected_image = self
            .selected_image
            .min(self.model.images.len().saturating_sub(1));
    }
}

fn cycle_matching(
    current: usize,
    len: usize,
    forward: bool,
    matches: impl Fn(usize) -> bool,
) -> usize {
    if len == 0 {
        return 0;
    }
    for offset in 1..=len {
        let index = if forward {
            (current + offset) % len
        } else {
            (current + len - (offset % len)) % len
        };
        if matches(index) {
            return index;
        }
    }
    current.min(len - 1)
}

fn action_is_allowed(status: &str, action: InstanceAction) -> bool {
    match action {
        InstanceAction::Start => status == "Stopped",
        InstanceAction::Stop => matches!(status, "Running" | "Frozen"),
        InstanceAction::Restart | InstanceAction::Freeze => status == "Running",
        InstanceAction::Unfreeze => status == "Frozen",
        InstanceAction::Delete => status == "Stopped",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::incus::{
        BackupSummary, ClusterGroupSummary, ClusterMemberSummary, EventEntry, ImageAlias,
        ImagePublishSpec, ImageSummary, ImageUpdateSpec, InstanceAction, InstanceSummary,
        MoveInstanceSpec, NetworkAclRule, NetworkAclSummary, NetworkForwardPort,
        NetworkForwardSummary, NetworkLoadBalancerBackend, NetworkLoadBalancerPort,
        NetworkLoadBalancerSpec, NetworkLoadBalancerSummary, NetworkPeerSummary, NetworkSummary,
        Operation, ProfileSummary, ResourceDefinition, ResourceKind, ServerInfo, SnapshotSummary,
        StorageBucketKeySummary, StorageBucketSummary, StoragePoolSummary, StorageVolumeBitmap,
        StorageVolumeBitmapSpec, StorageVolumeSpec, StorageVolumeSummary, StorageVolumeUpdateSpec,
        Transport, WarningSummary,
    };
    use crate::model::AppModel;
    use crate::openfga::TupleKey;
    use crate::remotes::SavedRemote;
    use crate::views::SavedView;

    use super::{
        AclRuleEditor, AclRulesForm, AddressSetManager, App, AppCommand, ArtifactTab,
        ArtifactsForm, AuditExportForm, BgpForm, BucketBackupsForm, BucketKeysForm, BulkManager,
        BulkTarget, ClusterGroupManager, ConfigKey, ConnectionForm, DetailForm, FleetRow,
        ImageForm, InstanceLogManager, InstanceMaintenanceManager, IntegrationManager,
        LoggingTargetForm, LoggingTargetManager, MaintenanceTab, NetworkServiceForm,
        NetworkServicesManager, NetworkServicesTab, Page, PoolSafetyManager, PreseedManager,
        PreseedSpec, QmpManager, ResourceCreateForm, ScriptletManager, VolumeAttachmentSpec,
        VolumeBackupsForm, VolumeDetachmentSpec, VolumeManager, ZoneManager, build_audit_export,
        format_load_balancer_state, format_network_state, metric_rows, parse_cpu_topology,
        parse_rfc3339_timestamp, parse_storage_size,
    };

    #[test]
    fn grouped_navigation_matches_nastty() {
        let mut app = App::new("local".into());
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            AppCommand::None
        );
        assert_eq!(app.page(), Page::Projects);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Settings);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Projects);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Dashboard);
    }

    #[test]
    fn project_switcher_seeds_structured_creation_forms() {
        let mut app = App::new("local".into());
        app.page = Page::Projects;
        app.model.projects = vec![
            crate::incus::ProjectSummary {
                name: "default".into(),
                ..Default::default()
            },
            crate::incus::ProjectSummary {
                name: "ops".into(),
                ..Default::default()
            },
        ];
        app.selected_project = 1;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.active_project(), "ops");
        app.page = Page::Instances;
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.create_form().expect("create form").value(3), "ops");
    }

    #[test]
    fn project_usage_opens_for_selected_project() {
        let mut app = App::new("local".into());
        app.page = Page::Projects;
        app.model.projects = vec![crate::incus::ProjectSummary {
            name: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('U'), KeyModifiers::SHIFT)),
            AppCommand::ReadProjectUsage {
                project: "ops".into()
            }
        );
        assert_eq!(app.project_usage().expect("usage form").project(), "ops");
    }

    #[test]
    fn project_access_opens_for_selected_project() {
        let mut app = App::new("local".into());
        app.page = Page::Projects;
        app.model.projects = vec![crate::incus::ProjectSummary {
            name: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            AppCommand::ReadProjectAccess {
                project: "ops".into()
            }
        );
        assert_eq!(app.project_access().expect("access form").project(), "ops");
    }

    #[test]
    fn instance_access_opens_for_selected_instance() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "web".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            AppCommand::ReadInstanceAccess {
                name: "web".into(),
                project: "ops".into()
            }
        );
        assert_eq!(app.instance_access().expect("access form").name(), "web");
    }

    #[test]
    fn number_keys_jump_to_pages() {
        let mut app = App::new("local".into());
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Operations);
        app.handle_key(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Settings);
        app.handle_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Settings);
    }

    #[test]
    fn help_toggle_does_not_quit_the_application() {
        let mut app = App::new("local".into());
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.help_visible());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.help_visible());
        assert!(!app.should_quit());
    }

    #[test]
    fn selection_wraps_over_instances() {
        let mut app = App::new("local".into());
        app.model.instances = vec![InstanceSummary::default(), InstanceSummary::default()];
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected_instance(), 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_instance(), 0);
    }

    #[test]
    fn slash_filter_selects_and_navigates_only_matching_resources() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![
            InstanceSummary {
                name: "database-1".into(),
                project: "prod".into(),
                ..Default::default()
            },
            InstanceSummary {
                name: "web-1".into(),
                project: "prod".into(),
                ..Default::default()
            },
            InstanceSummary {
                name: "database-2".into(),
                project: "dev".into(),
                ..Default::default()
            },
        ];
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for c in "database".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.filter_query(), "database");
        assert_eq!(app.selected_instance(), 0);
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.selected_instance(), 2);
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.selected_instance(), 0);
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
        assert!(app.filter_query().is_empty());
    }

    #[test]
    fn action_menu_dispatches_structured_page_actions() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.networks = vec![NetworkSummary {
            name: "incus0".into(),
            project: "default".into(),
            managed: true,
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(app.action_menu().is_some());
        for _ in 0..2 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ReadNetworkServices {
                network: "incus0".into(),
                project: "default".into()
            }
        );
    }

    #[test]
    fn lifecycle_actions_require_the_full_confirmation_phrase() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            status: "Stopped".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

        for character in "start demo".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ChangeInstanceState {
                name: "demo".into(),
                project: String::new(),
                action: InstanceAction::Start,
            }
        );
    }

    #[test]
    fn disallows_starting_an_already_running_instance() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            status: "Running".into(),
            ..Default::default()
        }];

        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

        assert!(app.confirmation().is_none());
        assert!(
            app.model
                .last_error
                .as_deref()
                .expect("state error")
                .contains("not available")
        );
    }

    #[test]
    fn create_form_submits_a_vm_spec_after_its_last_field() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        for character in "demo".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        for _ in 0..14 {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        let command = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let AppCommand::CreateInstance(spec) = command else {
            panic!("expected create command");
        };
        assert_eq!(spec.name, "demo");
        assert_eq!(spec.instance_type, "virtual-machine");
        assert_eq!(spec.image, "images:debian/12");
        assert!(spec.start_after_create);
    }

    #[test]
    fn command_form_targets_a_running_instance() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            status: "Running".into(),
            instance_type: "virtual-machine".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        for character in "id".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ExecuteCommand {
                name: "demo".into(),
                project: "ops".into(),
                command: "id".into(),
            }
        );
    }

    #[test]
    fn interactive_shell_and_console_target_selected_instance() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["console_force".into()],
            ..Default::default()
        });
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            status: "Running".into(),
            instance_type: "virtual-machine".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)),
            AppCommand::InteractiveExec {
                name: "demo".into(),
                project: "ops".into(),
                shell: "/bin/sh".into()
            }
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)),
            AppCommand::InteractiveConsole {
                name: "demo".into(),
                project: "ops".into(),
                force: false,
            }
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT)),
            AppCommand::VgaConsole {
                name: "demo".into(),
                project: "ops".into(),
                force: false,
            }
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::CONTROL)),
            AppCommand::InteractiveConsole {
                name: "demo".into(),
                project: "ops".into(),
                force: true,
            }
        );
    }

    #[test]
    fn interactive_exec_rejects_stopped_instances_before_handoff() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            status: "Stopped".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)),
            AppCommand::None
        );
        assert!(
            app.model
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("running")
        );
    }

    #[test]
    fn uppercase_e_opens_an_etag_guarded_instance_configuration_editor() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Instance,
                name: "demo".into(),
                project: "ops".into(),
            }
        );
    }

    #[test]
    fn network_and_storage_rows_open_the_same_safe_configuration_editor() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.networks = vec![NetworkSummary {
            name: "br0".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Network,
                name: "br0".into(),
                project: "ops".into(),
            }
        );

        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::StoragePool,
                name: "fast".into(),
                project: String::new(),
            }
        );
    }

    #[test]
    fn network_creation_uses_structured_fields_and_emits_a_typed_request() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for character in "br0".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        for _ in 0..8 {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }

        let AppCommand::CreateResource {
            kind,
            project,
            value,
        } = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected structured network creation request");
        };
        assert_eq!(kind, ResourceKind::Network);
        assert_eq!(project, "default");
        assert_eq!(value["name"], "br0");
        assert_eq!(value["type"], "bridge");
        assert_eq!(value["config"]["ipv4.nat"], "true");
    }

    #[test]
    fn deleting_a_network_requires_its_full_typed_phrase() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.networks = vec![NetworkSummary {
            name: "br0".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for character in "delete network br0".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteResource {
                kind: ResourceKind::Network,
                name: "br0".into(),
                project: "ops".into(),
            }
        );
    }

    #[test]
    fn image_push_form_builds_a_structured_remote_transfer() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.images = vec![ImageSummary {
            fingerprint: "abc".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        let ImageForm::Push { fields, field, .. } = app.image_form.as_mut().unwrap() else {
            panic!("push form")
        };
        *fields = [
            "https://backup:8443".into(),
            "archive".into(),
            "secret".into(),
            "server.crt".into(),
            "debian,stable".into(),
            "default,hardened".into(),
        ];
        *field = 5;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PushImage {
                fingerprint: "abc".into(),
                project: "ops".into(),
                target: "https://backup:8443".into(),
                target_project: "archive".into(),
                secret: "secret".into(),
                certificate_path: "server.crt".into(),
                aliases: vec!["debian".into(), "stable".into()],
                profiles: vec!["default".into(), "hardened".into()],
            }
        );
    }

    #[test]
    fn image_import_form_builds_extension_gated_alias_headers() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["image_import_alias".into()],
            ..Default::default()
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let ImageForm::Import {
            project,
            path,
            aliases,
            public,
            field,
        } = app.image_form.as_mut().expect("import form")
        else {
            panic!("image import form")
        };
        *project = "ops".into();
        *path = "./debian.tar.gz".into();
        *aliases = "debian/13, stable".into();
        *public = true;
        *field = 3;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ImportImage {
                project: "ops".into(),
                path: "./debian.tar.gz".into(),
                aliases: vec!["debian/13".into(), "stable".into()],
                public: true,
            }
        );
    }

    #[test]
    fn image_refresh_is_extension_gated_and_project_copy_is_structured() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["image_force_refresh".into()],
            ..Default::default()
        });
        app.model.images = vec![ImageSummary {
            fingerprint: "abc".into(),
            project: "ops".into(),
            public: true,
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)),
            AppCommand::RefreshImage {
                fingerprint: "abc".into(),
                project: "ops".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
        let ImageForm::CopyProject {
            target_project,
            field,
            ..
        } = app.image_form.as_mut().unwrap()
        else {
            panic!("copy form")
        };
        *target_project = "archive".into();
        *field = 1;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CopyImageProject {
                fingerprint: "abc".into(),
                source_project: "ops".into(),
                target_project: "archive".into(),
                public: true
            }
        );
    }

    #[test]
    fn selected_image_can_launch_an_instance_and_receive_an_alias() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.images = vec![ImageSummary {
            fingerprint: "abcdef1234567890".into(),
            project: "ops".into(),
            image_type: "virtual-machine".into(),
            aliases: vec![ImageAlias {
                name: "debian-vm".into(),
                ..Default::default()
            }],
            ..Default::default()
        }];

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let create = app.create_form().expect("launch form");
        assert_eq!(create.value(1), "debian-vm");
        assert_eq!(create.value(2), "virtual-machine");

        app.create_form = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for character in "stable".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "Stable VM".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateImageAlias {
                alias: "stable".into(),
                fingerprint: "abcdef1234567890".into(),
                project: "ops".into(),
                description: "Stable VM".into(),
            }
        );
    }

    #[test]
    fn image_alias_manager_builds_rename_and_guarded_delete_actions() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.images = vec![ImageSummary {
            fingerprint: "abcdef".into(),
            project: "ops".into(),
            aliases: vec![ImageAlias {
                name: "old".into(),
                description: "Old alias".into(),
            }],
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(editor) = app
            .image_alias_manager
            .as_mut()
            .and_then(|m| m.editor.as_mut())
        {
            editor.name = "stable".into();
            editor.description = "Stable image".into();
            editor.field = 1;
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::UpdateImageAlias {
                alias: "old".into(),
                new_name: "stable".into(),
                fingerprint: "abcdef".into(),
                project: "ops".into(),
                description: "Stable image".into()
            }
        );
        app.image_alias_manager.as_mut().expect("manager").editor = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for c in "delete image alias old".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteImageAlias {
                alias: "old".into(),
                project: "ops".into()
            }
        );
    }

    #[test]
    fn image_editor_preserves_properties_and_updates_typed_fields() {
        let mut app = App::new("local".into());
        app.page = Page::Images;
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["image_restrictions".into()],
            ..Default::default()
        });
        app.model.images = vec![ImageSummary {
            fingerprint: "abcdef".into(),
            project: "ops".into(),
            properties: BTreeMap::from([
                ("description".into(), "old".into()),
                ("user.vendor".into(), "keep".into()),
            ]),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(super::ImageForm::Edit {
            fields,
            field,
            spec,
        }) = app.image_form.as_mut()
        {
            fields[0] = "new".into();
            fields[5] = "true".into();
            spec.public = true;
            *field = 10;
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::UpdateImage(ImageUpdateSpec {
                fingerprint: "abcdef".into(),
                project: "ops".into(),
                public: true,
                auto_update: false,
                properties: BTreeMap::from([
                    ("description".into(), "new".into()),
                    ("requirements.nesting".into(), "true".into()),
                    ("user.vendor".into(), "keep".into()),
                ]),
            })
        );
    }

    #[test]
    fn running_instance_can_be_published_with_structured_image_options() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            status: "Stopped".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT));
        if let Some(super::ImageForm::Publish { spec, field }) = app.image_form.as_mut() {
            spec.alias = "golden".into();
            spec.description = "Golden image".into();
            spec.public = true;
            *field = 2;
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PublishInstanceImage(ImagePublishSpec {
                instance: "demo".into(),
                project: "ops".into(),
                alias: "golden".into(),
                description: "Golden image".into(),
                public: true,
            })
        );
    }

    #[test]
    fn artifacts_manager_lists_and_confirms_snapshot_deletion() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
            AppCommand::ReadArtifacts {
                name: "demo".into(),
                project: "ops".into(),
            }
        );
        app.artifacts_form
            .as_mut()
            .expect("artifacts form")
            .snapshots = vec![SnapshotSummary {
            name: "before-upgrade".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for character in "delete snapshot before-upgrade".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteSnapshot {
                name: "demo".into(),
                project: "ops".into(),
                snapshot: "before-upgrade".into(),
            }
        );
    }

    #[test]
    fn artifacts_manager_renames_a_snapshot_with_a_structured_input() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        app.artifacts_form
            .as_mut()
            .expect("artifact manager")
            .snapshots = vec![SnapshotSummary {
            name: "before-upgrade".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE));
        for character in "stable".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RenameSnapshot {
                name: "demo".into(),
                project: "ops".into(),
                snapshot: "before-upgrade".into(),
                new_name: "stable".into(),
            }
        );
    }

    #[test]
    fn artifacts_manager_exports_and_imports_backup_archives() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        let form = app.artifacts_form.as_mut().expect("artifact manager");
        form.tab = ArtifactTab::Backups;
        form.backups = vec![BackupSummary {
            name: "nightly".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ExportBackup {
                name: "demo".into(),
                project: "ops".into(),
                backup: "nightly".into(),
                path: "./demo-nightly-backup.tar.gz".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ImportBackup {
                project: "ops".into(),
                path: "./backup.tar.gz".into()
            }
        );
    }

    #[test]
    fn artifact_retention_preview_marks_exact_expired_candidates() {
        assert_eq!(parse_rfc3339_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_timestamp("1970-01-01T01:00:00+01:00"),
            Some(0)
        );
        let mut form = ArtifactsForm::new(&InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        });
        form.snapshots = vec![SnapshotSummary {
            name: "old".into(),
            expires_at: "2000-01-01T00:00:00Z".into(),
            ..Default::default()
        }];
        form.backups = vec![BackupSummary {
            name: "future".into(),
            expires_at: "2999-01-01T00:00:00Z".into(),
            ..Default::default()
        }];
        let rows = form.retention_rows();
        assert!(
            rows.iter()
                .any(|(name, _, expired)| name == "snapshot/old" && *expired)
        );
        assert!(
            rows.iter()
                .any(|(name, _, expired)| name == "backup/future" && !*expired)
        );
        let mut app = App::new("local".into());
        app.artifacts_form = Some(form);
        app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT));
        assert!(app.artifacts_form().unwrap().retention_preview());
    }

    #[test]
    fn saved_view_manager_persists_and_restores_page_and_filter() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.filter_query = "running".into();
        app.handle_key(KeyEvent::new(KeyCode::Char('\''), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "active".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PersistViews(vec![SavedView {
                name: "active".into(),
                page: "Instances".into(),
                filter: "running".into()
            }])
        );
        app.page = Page::Dashboard;
        app.filter_query.clear();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Instances);
        assert_eq!(app.filter_query, "running");
        assert!(app.view_manager.is_none());
    }

    #[test]
    fn audit_export_filters_and_recursively_redacts_secrets() {
        let model = AppModel {
            events: vec![EventEntry {
                event_type: "logging".into(),
                project: "ops".into(),
                metadata: serde_json::json!({"message":"login", "context":{"token":"abc", "safe":"yes"}}),
                ..Default::default()
            }],
            operations: vec![Operation {
                id: "op1".into(),
                description: "backup".into(),
                status: "Success".into(),
                metadata: serde_json::json!({"password":"secret"}),
                ..Default::default()
            }],
            ..Default::default()
        };
        let form = AuditExportForm {
            path: "audit.json".into(),
            filter: "ops".into(),
            include_events: true,
            include_operations: true,
            redact: true,
            field: 4,
        };
        let value = build_audit_export(&model, &form);
        assert_eq!(value["events"].as_array().unwrap().len(), 1);
        assert_eq!(value["operations"].as_array().unwrap().len(), 0);
        assert_eq!(
            value["events"][0]["metadata"]["context"]["token"],
            "[REDACTED]"
        );
        assert_eq!(value["events"][0]["metadata"]["context"]["safe"], "yes");
    }

    #[test]
    fn rename_form_emits_a_project_scoped_instance_rename() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "old-name".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT));
        for character in "new-name".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RenameInstance {
                name: "old-name".into(),
                project: "ops".into(),
                new_name: "new-name".into(),
            }
        );
    }

    #[test]
    fn rebuild_form_requires_a_stopped_instance_and_exact_confirmation() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["instances_rebuild".into()],
            ..Default::default()
        });
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            status: "Stopped".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT));
        for character in "debian/13".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for character in "rebuild demo".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RebuildInstance {
                name: "demo".into(),
                project: "ops".into(),
                image: Some("debian/13".into()),
            }
        );
    }

    #[test]
    fn log_manager_reads_and_guards_deletion_of_selected_log() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)),
            AppCommand::ReadInstanceLogs {
                name: "demo".into(),
                project: "ops".into()
            }
        );
        app.log_manager = Some(InstanceLogManager {
            name: "demo".into(),
            project: "ops".into(),
            logs: vec!["lxc.log".into()],
            selected: 0,
            content: None,
            delete_input: None,
            clear_console_input: None,
            cleanup_exec_input: None,
        });
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ReadInstanceLog {
                name: "demo".into(),
                project: "ops".into(),
                filename: "lxc.log".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for c in "delete log lxc.log".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteInstanceLog {
                name: "demo".into(),
                project: "ops".into(),
                filename: "lxc.log".into()
            }
        );
    }

    #[test]
    fn log_manager_requires_typed_bulk_exec_output_cleanup() {
        let mut app = App::new("local".into());
        app.log_manager = Some(InstanceLogManager {
            name: "demo".into(),
            project: "ops".into(),
            logs: vec![
                "lxc.log".into(),
                "exec-output/exec_1.stdout".into(),
                "exec-output/exec_1.stderr".into(),
            ],
            selected: 0,
            content: None,
            delete_input: None,
            clear_console_input: None,
            cleanup_exec_input: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT));
        for character in "delete 2 exec outputs demo".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CleanupInstanceExecOutputs {
                name: "demo".into(),
                project: "ops".into(),
                filenames: vec!["exec_1.stdout".into(), "exec_1.stderr".into()],
            }
        );
    }

    #[test]
    fn move_form_builds_project_member_pool_and_live_migration() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "default".into(),
            status: "Running".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT));
        if let Some(form) = app.move_form.as_mut() {
            form.spec.new_name = "demo-ops".into();
            form.spec.target_project = "ops".into();
            form.spec.target_member = "node-b".into();
            form.spec.target_pool = "fast".into();
            form.spec.live = true;
            form.field = 4;
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::MoveInstance(MoveInstanceSpec {
                name: "demo".into(),
                project: "default".into(),
                new_name: "demo-ops".into(),
                target_project: "ops".into(),
                target_member: "node-b".into(),
                target_pool: "fast".into(),
                live: true,
            })
        );
    }

    #[test]
    fn device_manager_builds_a_structured_disk_device_update() {
        let mut app = App::new("local".into());
        app.load_device_manager(ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "demo".into(),
            project: "ops".into(),
            value: serde_json::json!({ "config": {}, "devices": {} }),
            etag: Some("etag-1".into()),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for character in "data".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let last = app
            .device_manager
            .as_ref()
            .and_then(|m| m.editor.as_ref())
            .expect("editor")
            .fields
            .len()
            - 1;
        app.device_manager
            .as_mut()
            .and_then(|m| m.editor.as_mut())
            .expect("editor")
            .field = last;
        let AppCommand::UpdateResource { definition, value } =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected structured device update");
        };
        assert_eq!(definition.etag.as_deref(), Some("etag-1"));
        assert_eq!(value["devices"]["data"]["type"], "disk");
    }

    #[test]
    fn device_manager_builds_structured_pci_device() {
        let mut app = App::new("local".into());
        app.load_device_manager(ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "vm".into(),
            project: "ops".into(),
            value: serde_json::json!({ "config": {}, "devices": {} }),
            etag: Some("etag-pci".into()),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let editor = app
            .device_manager
            .as_mut()
            .and_then(|m| m.editor.as_mut())
            .expect("editor");
        editor
            .fields
            .iter_mut()
            .find(|f| f.key == "name")
            .expect("name")
            .value = "accelerator".into();
        editor
            .fields
            .iter_mut()
            .find(|f| f.key == "type")
            .expect("type")
            .value = "pci".into();
        editor
            .fields
            .iter_mut()
            .find(|f| f.key == "address")
            .expect("address")
            .value = "0000:65:00.0".into();
        editor.field = editor.fields.len() - 1;
        let AppCommand::UpdateResource { value, .. } =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected update")
        };
        assert_eq!(value["devices"]["accelerator"]["type"], "pci");
        assert_eq!(value["devices"]["accelerator"]["address"], "0000:65:00.0");
    }

    #[test]
    fn storage_volume_manager_builds_a_structured_block_volume_request() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec![
                "storage_initial_owner".into(),
                "shared_custom_block_volumes".into(),
            ],
            ..Default::default()
        });
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
            AppCommand::ReadStorageVolumes {
                pool: "fast".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for character in "database".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "20GiB".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "1000".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "1000".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "0750".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageVolume(StorageVolumeSpec {
                pool: "fast".into(),
                project: "default".into(),
                name: "database".into(),
                content_type: "block".into(),
                description: String::new(),
                size: "20GiB".into(),
                initial_uid: "1000".into(),
                initial_gid: "1000".into(),
                initial_mode: "0750".into(),
                shared: true,
            })
        );
    }

    #[test]
    fn storage_pool_resources_and_volume_copy_are_structured() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.server = Some(ServerInfo {
            api_extensions: vec![
                "storage_live_migration".into(),
                "custom_volume_refresh_exclude_older_snapshots".into(),
            ],
            ..Default::default()
        });
        app.model.storage_pools = vec![
            StoragePoolSummary {
                name: "fast".into(),
                ..Default::default()
            },
            StoragePoolSummary {
                name: "archive".into(),
                ..Default::default()
            },
        ];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            AppCommand::ReadStoragePoolResources {
                pool: "fast".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().unwrap().volumes = vec![StorageVolumeSummary {
            name: "data".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
        let form = app
            .volume_manager
            .as_mut()
            .unwrap()
            .copy_form
            .as_mut()
            .unwrap();
        form.target_project = "backup".into();
        form.target_name = "data-copy".into();
        form.refresh = true;
        form.live = true;
        form.refresh_exclude_older = true;
        form.field = 6;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CopyStorageVolume {
                source_pool: "fast".into(),
                source_project: "ops".into(),
                source_name: "data".into(),
                target_pool: "archive".into(),
                target_project: "backup".into(),
                target_name: "data-copy".into(),
                volume_only: false,
                refresh: true,
                live: true,
                refresh_exclude_older: true,
            }
        );
    }

    #[test]
    fn storage_pool_growth_validates_size_and_exposes_driver_health() {
        assert_eq!(parse_storage_size("10GiB"), Some(10 * 1024_u128.pow(3)));
        assert_eq!(parse_storage_size("bogus"), None);
        let definition = ResourceDefinition {
            kind: ResourceKind::StoragePool,
            name: "fast".into(),
            project: String::new(),
            etag: Some("pool-etag".into()),
            value: serde_json::json!({
                "name":"fast", "driver":"zfs",
                "config":{"source":"/var/lib/incus/disks/fast.img","size":"10GiB","zfs.pool_name":"fast"}
            }),
        };
        let mut manager = PoolSafetyManager::new(
            definition,
            serde_json::json!({"space":{"total":10737418240_u64,"used":1024}}),
            "zfs".into(),
        );
        manager.new_size = "9GiB".into();
        assert!(manager.growth_value().is_err());
        manager.new_size = "20GiB".into();
        let value = manager.growth_value().expect("larger loop size");
        assert_eq!(value["config"]["size"], "20GiB");
        assert!(
            manager
                .health_rows()
                .iter()
                .any(|(label, _)| label == "Space total")
        );
        assert!(
            manager
                .health_rows()
                .iter()
                .any(|(label, value)| label == "zfs.pool_name" && value == "fast")
        );

        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            driver: "zfs".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            AppCommand::ReadPoolSafety {
                pool: "fast".into()
            }
        );
    }

    #[test]
    fn storage_pool_source_wipe_requires_device_identity_and_full_phrase() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["storage_pool_source_wipe".into()],
            ..Default::default()
        });
        let mut form = ResourceCreateForm::new(ResourceKind::StoragePool);
        form.fields[0].value = "fast".into();
        form.fields[3].value = "/dev/nvme1n1".into();
        form.fields[5].value = "true".into();
        form.field = form.fields.len() - 1;
        app.resource_create_form = Some(form);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::None
        );
        let phrase = "wipe source /dev/nvme1n1 and create pool fast";
        assert_eq!(
            app.resource_create_form
                .as_ref()
                .expect("form")
                .wipe_phrase(),
            phrase
        );
        for c in phrase.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let AppCommand::CreateResource { kind, value, .. } =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected confirmed source-wipe creation");
        };
        assert_eq!(kind, ResourceKind::StoragePool);
        assert_eq!(value["config"]["source"], "/dev/nvme1n1");
        assert_eq!(value["config"]["source.wipe"], "true");
    }

    #[test]
    fn storage_volume_manager_builds_a_structured_resize_request() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().expect("volume manager").volumes = vec![StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            config: BTreeMap::from([("size".into(), "10GiB".into())]),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for _ in 0..5 {
            app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        for character in "20GiB".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::UpdateStorageVolume(StorageVolumeUpdateSpec {
                pool: "fast".into(),
                project: "ops".into(),
                name: "database".into(),
                description: String::new(),
                size: "20GiB".into(),
                config: BTreeMap::from([("size".into(), "10GiB".into())]),
            })
        );
    }

    #[test]
    fn storage_volume_rebuild_requires_full_destructive_phrase() {
        let mut app = App::new("local".into());
        app.volume_manager = Some(VolumeManager::new("fast".into()));
        app.volume_manager.as_mut().expect("manager").volumes = vec![StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
        for c in "rebuild storage volume database".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RebuildStorageVolume {
                pool: "fast".into(),
                project: "ops".into(),
                name: "database".into()
            }
        );
    }

    #[test]
    fn storage_volume_manager_builds_a_project_safe_attachment() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.model.instances = vec![InstanceSummary {
            name: "database-01".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().expect("volume manager").volumes = vec![StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::AttachStorageVolume(VolumeAttachmentSpec {
                pool: "fast".into(),
                volume_project: "ops".into(),
                volume_name: "database".into(),
                instance_name: "database-01".into(),
                instance_project: "ops".into(),
                device_name: "data".into(),
                path: "/mnt/data".into(),
                target_kind: ResourceKind::Instance,
            })
        );
    }

    #[test]
    fn storage_volume_manager_builds_a_safe_detachment() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.model.instances = vec![InstanceSummary {
            name: "database-01".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().expect("volume manager").volumes = vec![StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DetachStorageVolume(VolumeDetachmentSpec {
                pool: "fast".into(),
                volume_name: "database".into(),
                instance_name: "database-01".into(),
                instance_project: "ops".into(),
                device_name: "data".into(),
                target_kind: ResourceKind::Instance,
            })
        );
    }

    #[test]
    fn storage_volume_manager_builds_a_profile_attachment() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.model.profiles = vec![ProfileSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().expect("manager").volumes = vec![StorageVolumeSummary {
            name: "shared-data".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::AttachStorageVolume(VolumeAttachmentSpec {
                pool: "fast".into(),
                volume_project: "ops".into(),
                volume_name: "shared-data".into(),
                instance_name: "database".into(),
                instance_project: "ops".into(),
                device_name: "data".into(),
                path: "/mnt/data".into(),
                target_kind: ResourceKind::Profile,
            })
        );
    }

    #[test]
    fn storage_volume_manager_creates_a_named_volume_snapshot() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.volume_manager.as_mut().expect("volume manager").volumes = vec![StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
            AppCommand::ReadStorageVolumeSnapshots {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "database".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for character in "before-upgrade".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageVolumeSnapshot {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "database".into(),
                snapshot: "before-upgrade".into()
            }
        );
    }

    #[test]
    fn storage_volume_manager_creates_an_optimized_volume_backup() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        let volume = StorageVolumeSummary {
            name: "database".into(),
            project: "ops".into(),
            ..Default::default()
        };
        app.volume_manager.as_mut().expect("volume manager").volumes = vec![volume.clone()];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE)),
            AppCommand::ReadStorageVolumeBackups {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "database".into(),
            }
        );
        app.volume_manager.as_mut().expect("manager").backups = Some(VolumeBackupsForm {
            volume,
            backups: Vec::new(),
            selected: 0,
            name_input: None,
            create_options: (false, false),
            delete_input: None,
            export_path: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "nightly".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageVolumeBackup {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "database".into(),
                name: "nightly".into(),
                volume_only: true,
                optimized_storage: true,
            }
        );
    }

    #[test]
    fn storage_volume_manager_builds_structured_backup_import() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "fast".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let form = app
            .volume_manager
            .as_mut()
            .expect("manager")
            .import_form
            .as_mut()
            .expect("import form");
        form.local_path = "/tmp/database.tar.gz".into();
        form.project = "ops".into();
        form.name = "database-restored".into();
        form.field = 3;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ImportStorageVolume {
                pool: "fast".into(),
                project: "ops".into(),
                name: "database-restored".into(),
                import_type: "backup".into(),
                path: "/tmp/database.tar.gz".into(),
            }
        );
    }

    #[test]
    fn storage_bucket_manager_creates_a_structured_bucket() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "objects".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
            AppCommand::ReadStorageBuckets {
                pool: "objects".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "archive".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageBucket {
                pool: "objects".into(),
                project: "default".into(),
                name: "archive".into(),
                description: String::new(),
                size: String::new()
            }
        );
    }

    #[test]
    fn storage_bucket_manager_opens_key_management() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "objects".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        app.bucket_manager.as_mut().expect("bucket manager").buckets = vec![StorageBucketSummary {
            name: "archive".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            AppCommand::ReadStorageBucketKeys {
                pool: "objects".into(),
                project: "ops".into(),
                bucket: "archive".into()
            }
        );
    }

    #[test]
    fn storage_bucket_key_editor_builds_a_structured_update() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "objects".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        let manager = app.bucket_manager.as_mut().expect("bucket manager");
        manager.keys = Some(BucketKeysForm {
            bucket: StorageBucketSummary {
                name: "archive".into(),
                project: "ops".into(),
                ..Default::default()
            },
            keys: vec![StorageBucketKeySummary {
                name: "backup".into(),
                description: "old".into(),
                role: "read-only".into(),
                ..Default::default()
            }],
            selected: 0,
            editor: None,
            delete_input: None,
            credentials: None,
        });

        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }
        for c in "new".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::UpdateStorageBucketKey {
                pool: "objects".into(),
                project: "ops".into(),
                bucket: "archive".into(),
                name: "backup".into(),
                description: "new".into(),
                role: "admin".into(),
            }
        );
    }

    #[test]
    fn storage_bucket_manager_creates_compressed_backup() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "objects".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        let bucket = StorageBucketSummary {
            name: "archive".into(),
            project: "ops".into(),
            ..Default::default()
        };
        app.bucket_manager.as_mut().expect("manager").buckets = vec![bucket.clone()];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT)),
            AppCommand::ReadStorageBucketBackups {
                pool: "objects".into(),
                project: "ops".into(),
                bucket: "archive".into()
            }
        );
        app.bucket_manager.as_mut().expect("manager").backups = Some(BucketBackupsForm {
            bucket,
            backups: Vec::new(),
            selected: 0,
            name_input: None,
            compression: "gzip".into(),
            delete_input: None,
            export_path: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "nightly".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageBucketBackup {
                pool: "objects".into(),
                project: "ops".into(),
                bucket: "archive".into(),
                name: "nightly".into(),
                compression: "gzip".into(),
            }
        );
    }

    #[test]
    fn storage_bucket_manager_builds_backup_import() {
        let mut app = App::new("local".into());
        app.page = Page::Storage;
        app.model.storage_pools = vec![StoragePoolSummary {
            name: "objects".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let form = app
            .bucket_manager
            .as_mut()
            .expect("manager")
            .import_form
            .as_mut()
            .expect("import");
        form.path = "/tmp/archive.tar.gz".into();
        form.project = "ops".into();
        form.name = "restored".into();
        form.field = 2;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ImportStorageBucket {
                pool: "objects".into(),
                project: "ops".into(),
                name: "restored".into(),
                path: "/tmp/archive.tar.gz".into(),
            }
        );
    }

    #[test]
    fn settings_opens_the_live_server_configuration_editor() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Server,
                name: String::new(),
                project: String::new(),
            }
        );
    }

    #[test]
    fn image_refresh_policy_combines_global_choices_and_per_image_toggle() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Server,
                name: String::new(),
                project: String::new()
            }
        );
        app.load_configuration_form(ResourceDefinition { kind: ResourceKind::Server, name: String::new(), project: String::new(), value: serde_json::json!({"images.auto_update_interval":"24", "images.auto_update_cached":"true", "images.remote_cache_expiry":"72"}), etag: Some("server-policy".into()) });
        let form = app.config_form.as_ref().unwrap();
        assert!(matches!(
            form.fields[form.field].key,
            ConfigKey::Config("images.auto_update_interval")
        ));
        let preview = form.image_policy_preview().unwrap();
        assert!(preview.contains("24 hours"));
        assert!(preview.contains("are refreshed"));
        assert!(preview.contains("72 hours"));

        app.config_form = None;
        app.page = Page::Images;
        app.model.images = vec![ImageSummary {
            fingerprint: "abc".into(),
            project: "default".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT));
        assert!(matches!(
            app.image_form,
            Some(ImageForm::Edit { field: 10, .. })
        ));
    }

    #[test]
    fn cached_image_retention_lists_exact_delete_and_keep_candidates() {
        let mut app = App::new("local".into());
        app.model.images = vec![
            ImageSummary {
                fingerprint: "old".into(),
                cached: true,
                uploaded_at: "2000-01-01T00:00:00Z".into(),
                ..Default::default()
            },
            ImageSummary {
                fingerprint: "future".into(),
                cached: true,
                uploaded_at: "2999-01-01T00:00:00Z".into(),
                ..Default::default()
            },
            ImageSummary {
                fingerprint: "private".into(),
                cached: false,
                uploaded_at: "2000-01-01T00:00:00Z".into(),
                ..Default::default()
            },
        ];
        app.load_image_retention(ResourceDefinition {
            kind: ResourceKind::Server,
            name: String::new(),
            project: String::new(),
            value: serde_json::json!({"images.remote_cache_expiry":"24"}),
            etag: None,
        });
        let preview = app.image_retention_preview.as_ref().unwrap();
        assert_eq!(preview.rows.len(), 2);
        assert!(preview.rows.iter().any(|row| row.0 == "old" && row.2));
        assert!(preview.rows.iter().any(|row| row.0 == "future" && !row.2));
    }

    #[test]
    fn authorization_settings_focus_and_preserve_masked_token() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Server,
                name: String::new(),
                project: String::new()
            }
        );
        app.load_configuration_form(ResourceDefinition {
            kind: ResourceKind::Server,
            name: String::new(),
            project: String::new(),
            value: serde_json::json!({
                "oidc.issuer": "https://identity.example",
                "oidc.client.id": "incus",
                "authorization.openfga.api.url": "https://fga.example",
                "authorization.openfga.api.token": "existing-secret",
                "authorization.openfga.store.id": "store-1"
            }),
            etag: Some("auth-config".into()),
        });
        let form = app.config_form.as_ref().unwrap();
        assert!(matches!(
            form.fields[form.field].key,
            ConfigKey::Config("oidc.issuer")
        ));
        assert!(form.authorization_preview().unwrap().contains("configured"));
        assert_eq!(
            form.value()["authorization.openfga.api.token"],
            "existing-secret"
        );

        let token = form
            .fields
            .iter()
            .position(|field| {
                matches!(
                    field.key,
                    ConfigKey::Config("authorization.openfga.api.token")
                )
            })
            .unwrap();
        let form = app.config_form.as_mut().unwrap();
        form.field = token;
        form.input('n');
        assert_eq!(form.value()["authorization.openfga.api.token"], "n");
    }

    #[test]
    fn openfga_manager_builds_typed_check_and_guarded_mutation() {
        let mut app = App::new("local".into());
        app.load_openfga(
            ResourceDefinition {
                kind: ResourceKind::Server,
                name: String::new(),
                project: String::new(),
                value: serde_json::json!({
                    "authorization.openfga.api.url":"https://fga.example",
                    "authorization.openfga.store.id":"store-1",
                    "authorization.openfga.api.token":"secret"
                }),
                etag: None,
            },
            vec![TupleKey {
                user: "user:anne".into(),
                relation: "viewer".into(),
                object: "project:ops".into(),
            }],
        );
        let manager = app.openfga_manager.as_mut().unwrap();
        manager.fields = [
            "user".into(),
            "bob".into(),
            "viewer".into(),
            "project".into(),
            "ops".into(),
        ];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CheckOpenFga {
                tuple: TupleKey {
                    user: "user:bob".into(),
                    relation: "viewer".into(),
                    object: "project:ops".into()
                }
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for character in "grant relationship".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::MutateOpenFga { delete: false, .. }
        ));
        let (_, _, token) = app.openfga_connection().unwrap();
        assert_eq!(token, "secret");
        assert!(!format!("{:?}", app.openfga_manager().unwrap().tuples()).contains("secret"));
    }

    #[test]
    fn local_action_journal_records_only_payload_free_mutations() {
        let mut app = App::new("production".into());
        let command = AppCommand::ExecuteCommand {
            name: "vm1".into(),
            project: "default".into(),
            command: "echo super-secret".into(),
        };
        let entries = app.record_submitted_action(&command).unwrap();
        assert_eq!(entries[0].action, "ExecuteCommand");
        assert_eq!(entries[0].target, "production");
        assert!(
            !serde_json::to_string(&entries)
                .unwrap()
                .contains("super-secret")
        );
        assert!(app.record_submitted_action(&AppCommand::Refresh).is_none());

        app.page = Page::Settings;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert!(app.journal_viewer.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(app.journal_rows().len(), 1);
    }

    #[test]
    fn fleet_health_manager_loads_navigates_and_switches_saved_targets() {
        let mut app = App::new("local".into());
        app.saved_remotes = vec![
            SavedRemote {
                name: "lab".into(),
                transport: Transport::local("/run/incus/unix.socket"),
            },
            SavedRemote {
                name: "edge".into(),
                transport: Transport::local("/run/incus-edge/unix.socket"),
            },
        ];
        app.page = Page::Settings;
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)),
            AppCommand::ReadFleetHealth(remotes) if remotes.len() == 2
        ));
        app.load_fleet_health(vec![
            FleetRow {
                name: "lab".into(),
                endpoint: "lab socket".into(),
                version: "6.12".into(),
                instances: 4,
                running: 3,
                warnings: 0,
                drift_keys: 0,
                error: None,
            },
            FleetRow {
                name: "edge".into(),
                endpoint: "edge socket".into(),
                version: "6.11".into(),
                instances: 2,
                running: 1,
                warnings: 1,
                drift_keys: 3,
                error: None,
            },
        ]);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.fleet_manager.as_ref().unwrap().selected, 1);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::Connect(Transport::local("/run/incus-edge/unix.socket"))
        );
    }

    #[test]
    fn placement_inspector_explains_observable_member_eligibility() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "vm1".into(),
            architecture: "x86_64".into(),
            location: "node1".into(),
            ..Default::default()
        }];
        app.model.cluster_members = vec![
            ClusterMemberSummary {
                server_name: "node1".into(),
                status: "Online".into(),
                architecture: "x86_64".into(),
                failure_domain: "rack-a".into(),
                ..Default::default()
            },
            ClusterMemberSummary {
                server_name: "node2".into(),
                status: "Offline".into(),
                architecture: "x86_64".into(),
                failure_domain: "rack-b".into(),
                ..Default::default()
            },
        ];
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        let inspector = app.placement_inspector.as_ref().unwrap();
        assert!(inspector.eligibility(&inspector.members[0]).0);
        assert!(!inspector.eligibility(&inspector.members[1]).0);
        assert!(
            inspector
                .eligibility(&inspector.members[1])
                .1
                .contains("Offline")
        );
    }

    #[test]
    fn backup_schedule_form_persists_typed_policy_and_emits_due_backup() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "vm1".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        let form = app.backup_schedule_form.as_mut().unwrap();
        form.schedule.enabled = true;
        form.schedule.interval_seconds = 3600;
        form.schedule.name_pattern = "nightly-{timestamp}".into();
        form.field = 5;
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PersistSchedules(schedules) if schedules.len() == 1
        ));
        let due = app.take_due_backups();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, "vm1");
        assert!(due[0].2.starts_with("nightly-"));
        assert!(app.take_due_backups().is_empty());
    }

    #[test]
    fn settings_logging_targets_are_dynamic_structured_and_preserve_secrets() {
        let definition = ResourceDefinition {
            kind: ResourceKind::Server,
            name: String::new(),
            project: String::new(),
            etag: Some("server-etag".into()),
            value: serde_json::json!({
                "core.https_address": "[::]:8443",
                "logging.primary.target.type": "loki",
                "logging.primary.target.address": "https://loki.example.test",
                "logging.primary.target.password": "existing-secret",
                "logging.primary.types": "lifecycle,logging"
            }),
        };
        let manager = LoggingTargetManager::new(definition.clone(), true);
        assert_eq!(manager.targets(), &["primary"]);
        let mut form = LoggingTargetForm::new(&manager, Some("primary"));
        form.values[0] = "central".into();
        form.values[1] = "webhook".into();
        form.values[2] = "https://events.example.test/hook".into();
        form.values[10] = "lifecycle,network-acl".into();
        form.values[12] = "ops,prod;instance,network".into();

        let value = LoggingTargetManager::value_for(&definition, &form);
        assert!(value.get("logging.primary.target.type").is_none());
        assert_eq!(value["logging.central.target.password"], "existing-secret");
        assert_eq!(value["logging.central.target.type"], "webhook");
        assert_eq!(value["logging.central.lifecycle.projects"], "ops,prod");
        assert_eq!(value["logging.central.lifecycle.types"], "instance,network");

        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["server_logging".into(), "server_logging_webhook".into()],
            ..Default::default()
        });
        app.open_settings_manager();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)),
            AppCommand::ReadLoggingTargets
        );
    }

    #[test]
    fn scriptlet_studio_validates_stages_activates_and_rolls_back() {
        let definition = ResourceDefinition {
            kind: ResourceKind::Server,
            name: String::new(),
            project: String::new(),
            etag: Some("server-etag".into()),
            value: serde_json::json!({
                "instances.placement.scriptlet": "def instance_placement(request, candidate_members):\n    return\n"
            }),
        };
        let mut manager = ScriptletManager::new(definition, true, true);
        manager.template();
        assert!(manager.validate());
        assert!(manager.changed());

        let mut app = App::new("local".into());
        app.scriptlet_manager = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let phrase = "activate placement scriptlet";
        for character in phrase.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let AppCommand::UpdateScriptlet {
            definition,
            value,
            activated_source,
        } = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected staged scriptlet activation");
        };
        assert_eq!(definition.etag.as_deref(), Some("server-etag"));
        assert_eq!(value["instances.placement.scriptlet"], activated_source);
        assert!(activated_source.contains("set_target"));

        let manager = app.scriptlet_manager.as_mut().expect("studio");
        manager.draft = "def broken(:".into();
        manager.cursor = manager.draft.len();
        assert!(!manager.validate());
        assert!(
            manager
                .validation
                .iter()
                .any(|line| line.contains("required entry point"))
        );

        let qemu_definition = ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "vm01".into(),
            project: "ops".into(),
            etag: Some("vm-etag".into()),
            value: serde_json::json!({"config": {}, "devices": {}}),
        };
        let mut qemu = ScriptletManager::new_qemu(qemu_definition);
        qemu.template();
        assert!(qemu.validate());
        let value = qemu.value(qemu.draft());
        assert!(
            value["config"]["raw.qemu.scriptlet"]
                .as_str()
                .expect("QEMU scriptlet")
                .contains("def qemu_hook(instance, stage):")
        );

        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["qemu_scriptlet".into()],
            ..Default::default()
        });
        app.model.instances = vec![InstanceSummary {
            name: "vm01".into(),
            project: "ops".into(),
            instance_type: "virtual-machine".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
            AppCommand::ReadQemuScriptlet {
                kind: ResourceKind::Instance,
                name: "vm01".into(),
                project: "ops".into(),
            }
        );
        let qmp_definition = ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "vm01".into(),
            project: "ops".into(),
            etag: Some("vm-etag".into()),
            value: serde_json::json!({"config":{},"devices":{}}),
        };
        let mut qmp = QmpManager::new(qmp_definition);
        qmp.command = "query-block".into();
        qmp.add_command().expect("allow-listed QMP command");
        qmp.command = "system_reset".into();
        qmp.add_command().expect("allow-listed QMP command");
        let value = qmp.value();
        let commands: serde_json::Value = serde_json::from_str(
            value["config"]["raw.qemu.qmp.pre-start"]
                .as_str()
                .expect("QMP hook list"),
        )
        .expect("QMP JSON list");
        assert_eq!(commands.as_array().expect("commands").len(), 2);

        app.model
            .server
            .as_mut()
            .expect("server")
            .api_extensions
            .push("qemu_raw_qmp".into());
        app.page = Page::Instances;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT)),
            AppCommand::ReadQmpManager {
                name: "vm01".into(),
                project: "ops".into()
            }
        );

        app.page = Page::Projects;
        app.model.profiles = vec![ProfileSummary {
            name: "vm-defaults".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
            AppCommand::ReadQemuScriptlet {
                kind: ResourceKind::Profile,
                name: "vm-defaults".into(),
                project: "ops".into(),
            }
        );
    }

    #[test]
    fn typed_preseed_previews_serializes_and_requires_confirmed_apply() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)),
            AppCommand::None
        );
        let manager = app.preseed_manager.as_mut().expect("typed preseed");
        manager.spec = PreseedSpec {
            https_address: "[::]:8443".into(),
            pool_driver: "zfs".into(),
            pool_source: "tank/incus".into(),
            profile_cpu: "4".into(),
            profile_memory: "8GiB".into(),
            ..PreseedSpec::default()
        };
        assert!(manager.validate().is_ok());
        let encoded = serde_json::to_string(&manager.spec).expect("serialize preseed");
        assert_eq!(
            serde_json::from_str::<PreseedSpec>(&encoded).expect("parse preseed"),
            manager.spec
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        assert!(app.preseed_manager.as_ref().expect("manager").preview);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for c in "apply typed preseed".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let AppCommand::ApplyPreseed(spec) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected confirmed typed preseed apply");
        };
        assert_eq!(spec.pool_driver, "zfs");
        assert_eq!(spec.profile_memory, "8GiB");

        let mut invalid = PreseedManager::new();
        invalid.spec.server_enabled = false;
        invalid.spec.storage_enabled = false;
        invalid.spec.network_enabled = false;
        invalid.spec.profile_enabled = false;
        assert!(invalid.validate().is_err());

        let mut cluster = PreseedManager::new();
        cluster.spec.cluster_enabled = true;
        assert!(cluster.validate().is_err());
        cluster.spec.cluster_server_name = "member02".into();
        cluster.spec.cluster_address = "10.0.0.2:8443".into();
        cluster.spec.cluster_token = "join-secret".into();
        cluster.spec.cluster_group = "gpu".into();
        assert!(cluster.validate().is_ok());
        let encoded = serde_json::to_string(&cluster.spec).expect("cluster preseed");
        assert!(encoded.contains("cluster_certificate_path"));
        assert!(encoded.contains("member02"));

        let mut multi = PreseedManager::new();
        multi.field = 4;
        multi.spec.pool_name = "fast".into();
        multi.spec.pool_driver = "zfs".into();
        multi.spec.pool_source = "tank/fast".into();
        assert_eq!(
            multi.stage_current().expect("stage pool"),
            "staged pool fast"
        );
        multi.spec.pool_name = "bulk".into();
        multi.spec.pool_driver = "btrfs".into();
        assert_eq!(
            multi.stage_current().expect("stage pool"),
            "staged pool bulk"
        );
        multi.field = 8;
        multi.spec.network_name = "frontend".into();
        assert_eq!(
            multi.stage_current().expect("stage network"),
            "staged network frontend"
        );
        multi.field = 24;
        multi.cycle(false);
        assert_eq!(multi.spec.profile_pool, "fast");
        multi.cycle(false);
        assert_eq!(multi.spec.profile_pool, "bulk");
        multi.field = 25;
        multi.cycle(false);
        assert_eq!(multi.spec.profile_network, "frontend");
        multi.field = 13;
        multi.spec.profile_name = "web".into();
        assert_eq!(
            multi.stage_current().expect("stage profile"),
            "staged profile web"
        );
        assert_eq!(multi.spec.additional_pools.len(), 2);
        assert_eq!(multi.spec.additional_networks.len(), 1);
        assert_eq!(multi.spec.additional_profiles[0].pool, "bulk");
        assert_eq!(multi.spec.additional_profiles[0].network, "frontend");
        assert!(multi.validate().is_ok());
        multi.field = 4;
        assert_eq!(
            multi.remove_last_staged().expect("remove pool"),
            "removed staged pool bulk"
        );
    }

    #[test]
    fn settings_connection_form_builds_local_and_remote_tls_transports() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::Connect(Transport::local("/var/lib/incus/unix.socket"))
        );
        app.connection_form = Some(ConnectionForm::new(Vec::new()));
        let form = app.connection_form.as_mut().expect("connection form");
        form.remote = true;
        form.fields = [
            "https://incus.example.test:8443".into(),
            "/tmp/client.crt".into(),
            "/tmp/client.key".into(),
            "/tmp/ca.crt".into(),
            "lab".into(),
        ];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            AppCommand::PersistRemotes(vec![SavedRemote {
                name: "lab".into(),
                transport: Transport::remote_tls(
                    "https://incus.example.test:8443",
                    "/tmp/client.crt",
                    "/tmp/client.key",
                    Some("/tmp/ca.crt")
                ),
            }])
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::Connect(Transport::remote_tls(
                "https://incus.example.test:8443",
                "/tmp/client.crt",
                "/tmp/client.key",
                Some("/tmp/ca.crt")
            ))
        );
    }

    #[test]
    fn snapshot_form_creates_a_named_snapshot() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        for character in "before-upgrade".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateSnapshot {
                name: "demo".into(),
                project: "ops".into(),
                snapshot_name: "before-upgrade".into(),
                stateful: true,
            }
        );
    }

    #[test]
    fn backup_form_creates_an_optimized_backup() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        for character in "before-upgrade".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateBackup {
                name: "demo".into(),
                project: "ops".into(),
                backup_name: "before-upgrade".into(),
                instance_only: false,
                optimized_storage: true,
            }
        );
    }

    #[test]
    fn console_key_opens_the_selected_instance_log() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
            AppCommand::ReadConsole {
                name: "demo".into(),
                project: "ops".into(),
            }
        );
    }

    #[test]
    fn enter_opens_the_selected_instance_definition() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ReadInstanceDetail {
                name: "demo".into(),
                project: "ops".into(),
            }
        );
    }

    #[test]
    fn instance_details_toggle_expanded_profile_diff() {
        assert_eq!(
            parse_cpu_topology("sockets=2,cores=4,threads=2"),
            Ok((2, 4, 2))
        );
        assert!(parse_cpu_topology("sockets=0,cores=4").is_err());
        assert!(parse_cpu_topology("dies=2").is_err());
        let mut app = App::new("local".into());
        app.detail_form = Some(DetailForm::new("demo".into(), "ops".into()));
        assert!(!app.detail_form().expect("details").expanded_diff());
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.detail_form().expect("details").expanded_diff());
        let form = app.detail_form.as_mut().expect("details");
        form.state = Some(serde_json::json!({
            "started_at":"2026-07-11T12:00:00Z", "pid":4242, "processes":87,
            "os_info":{"pretty_name":"Ubuntu 24.04 LTS","kernel_version":"6.8.0","boot_id":"boot-123"}
        }));
        let rows = form.runtime_rows();
        assert!(
            rows.iter()
                .any(|(label, value)| label == "Started at" && value.contains("2026"))
        );
        assert!(
            rows.iter()
                .any(|(label, value)| label == "OS" && value == "Ubuntu 24.04 LTS")
        );
        assert!(
            rows.iter()
                .any(|(label, value)| label == "Guest agent" && value == "available")
        );
    }

    #[test]
    fn file_form_reads_the_selected_path() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ReadFile {
                name: "demo".into(),
                project: "ops".into(),
                path: "/etc/hostname".into(),
            }
        );
    }

    #[test]
    fn file_form_browses_and_selects_directory_entries() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        app.file_form.as_mut().expect("file form").path = "/etc".into();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
            AppCommand::ListDirectory {
                name: "demo".into(),
                project: "ops".into(),
                path: "/etc".into(),
            }
        );
        let form = app.file_form.as_mut().expect("file form");
        form.directory_entries = vec!["hosts".into(), "hostname".into()];
        form.directory_selected = 0;
        form.browsing_directory = true;
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.file_form().expect("file form").path(), "/etc/hostname");
        assert!(!app.file_form().expect("file form").browsing_directory());
    }

    #[test]
    fn file_form_builds_structured_pull_and_push_transfers() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PullFile {
                name: "demo".into(),
                project: "ops".into(),
                path: "/etc/hostname".into(),
                local_path: "./hostname".into()
            }
        );
        app.file_form.as_mut().expect("file form").transfer = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::PushFile {
                name: "demo".into(),
                project: "ops".into(),
                path: "/etc/hostname".into(),
                local_path: "./file".into()
            }
        );
    }

    #[test]
    fn file_delete_requires_a_typed_path_confirmation() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        for character in "delete /etc/hostname".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteFile {
                name: "demo".into(),
                project: "ops".into(),
                path: "/etc/hostname".into(),
            }
        );
    }

    #[test]
    fn operation_cancellation_requires_a_typed_operation_id() {
        let mut app = App::new("local".into());
        app.page = Page::Operations;
        app.model.operations = vec![Operation {
            id: "op-123".into(),
            description: "Create instance".into(),
            may_cancel: true,
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        for character in "cancel op-123".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CancelOperation {
                id: "op-123".into()
            }
        );
    }

    #[test]
    fn enter_opens_structured_operation_details() {
        let mut app = App::new("local".into());
        app.page = Page::Operations;
        app.model.operations = vec![Operation {
            id: "op-1".into(),
            description: "Creating instance".into(),
            status: "Running".into(),
            may_cancel: true,
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::None
        );
        assert_eq!(
            app.operation_detail()
                .expect("operation details")
                .operation()
                .id,
            "op-1"
        );
    }

    #[test]
    fn operation_details_jump_to_affected_project_resource() {
        let mut app = App::new("local".into());
        app.page = Page::Operations;
        app.model.instances = vec![
            InstanceSummary {
                name: "demo".into(),
                project: "default".into(),
                ..Default::default()
            },
            InstanceSummary {
                name: "demo".into(),
                project: "ops".into(),
                ..Default::default()
            },
        ];
        app.model.operations = vec![Operation {
            id: "op".into(),
            resources: BTreeMap::from([(
                "instances".into(),
                vec!["/1.0/instances/demo?project=ops".into()],
            )]),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.page(), Page::Instances);
        assert_eq!(app.selected_instance(), 1);
        assert!(app.operation_detail().is_none());
    }

    #[test]
    fn settings_manager_acknowledges_and_safely_deletes_warnings() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.warnings = vec![WarningSummary {
            uuid: "warning-1".into(),
            status: "new".into(),
            last_message: "storage degraded".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            AppCommand::AcknowledgeWarning {
                uuid: "warning-1".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for character in "delete warning warning-1".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteWarning {
                uuid: "warning-1".into()
            }
        );
    }

    #[test]
    fn settings_capability_matrix_filters_and_navigates_extensions() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.server = Some(ServerInfo {
            api_extensions: vec![
                "network_state".into(),
                "network_acl_log".into(),
                "storage_volume_state".into(),
                "metadata_configuration".into(),
            ],
            ..Default::default()
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT));
        for c in "network".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let manager = app.settings_manager.as_ref().expect("settings");
        assert_eq!(manager.capability_filter(), Some("network"));
        assert_eq!(manager.capability_selected(), 1);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.settings_manager
                .as_ref()
                .expect("settings")
                .capability_filter(),
            None
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT)),
            AppCommand::ReadMetadataConfiguration
        );
    }

    #[test]
    fn settings_manager_builds_a_restricted_trust_token_request() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for c in "ops-laptop".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for c in "ops".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::GenerateCertificateToken {
                name: "ops-laptop".into(),
                restricted: true,
                projects: vec!["ops".into()]
            }
        );
    }

    #[test]
    fn settings_manager_requires_typed_cluster_evacuation() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.cluster_members = vec![ClusterMemberSummary {
            server_name: "node-a".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        for c in "evacuate cluster member node-a".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ChangeClusterMemberState {
                name: "node-a".into(),
                action: "evacuate".into()
            }
        );
    }

    #[test]
    fn settings_manager_requires_typed_cluster_member_deletion() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.cluster_members = vec![ClusterMemberSummary {
            server_name: "node-b".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for c in "delete cluster member node-b".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteClusterMember {
                name: "node-b".into()
            }
        );
    }

    #[test]
    fn cluster_manager_exposes_state_rename_configuration_and_certificate_rotation() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.cluster_members = vec![ClusterMemberSummary {
            server_name: "node1".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)),
            AppCommand::ReadClusterMemberState {
                name: "node1".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
        for c in "node-a".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RenameClusterMember {
                name: "node1".into(),
                new_name: "node-a".into()
            }
        );
        app.settings_manager.as_mut().unwrap().cluster_rename_input = None;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)),
            AppCommand::ReadClusterConfiguration
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
        assert!(
            app.settings_manager()
                .unwrap()
                .cluster_certificate_fields()
                .is_some()
        );
    }

    #[test]
    fn settings_manager_builds_cluster_join_token_request() {
        let mut app = App::new("local".into());
        app.page = Page::Settings;
        app.model.cluster_members = vec![ClusterMemberSummary {
            server_name: "node-a".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "node-c".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::GenerateClusterJoinToken {
                name: "node-c".into()
            }
        );
    }

    #[test]
    fn cluster_group_manager_builds_structured_member_assignment() {
        let mut app = App::new("local".into());
        let mut manager = ClusterGroupManager::new();
        manager.groups = vec![ClusterGroupSummary {
            name: "database".into(),
            description: "DB nodes".into(),
            members: vec!["node-a".into()],
        }];
        app.cluster_group_manager = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(editor) = app
            .cluster_group_manager
            .as_mut()
            .and_then(|m| m.editor.as_mut())
        {
            editor.fields[2] = "node-a,node-b".into();
            editor.field = 2;
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::UpdateClusterGroup {
                name: "database".into(),
                description: "DB nodes".into(),
                members: vec!["node-a".into(), "node-b".into()]
            }
        );
    }

    #[test]
    fn managed_network_opens_structured_services_manager() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.networks = vec![NetworkSummary {
            name: "incus0".into(),
            project: "default".into(),
            managed: true,
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            AppCommand::ReadNetworkServices {
                network: "incus0".into(),
                project: "default".into(),
            }
        );
        assert!(app.network_services().is_some());
    }

    #[test]
    fn load_balancer_health_is_normalized_per_backend_without_raw_json() {
        let state = serde_json::json!({"backends":[
            {"name":"api-1", "address":"10.0.0.2", "status":"up"},
            {"backend":"api-2", "health":"down", "reason":"connection refused"}
        ]});
        let summary = format_load_balancer_state(&state);
        assert!(summary.contains("api-1"));
        assert!(summary.contains("api-2"));
        assert!(summary.contains("connection refused"));
        assert!(!summary.contains('{'));
    }

    #[test]
    fn network_services_exposes_acl_logs_rename_and_load_balancer_state() {
        let mut app = App::new("local".into());
        app.model.server = Some(crate::incus::ServerInfo {
            api_extensions: vec![
                "network_acl_log".into(),
                "network_load_balancer_state".into(),
                "network_state".into(),
            ],
            ..Default::default()
        });
        app.network_services = Some(NetworkServicesManager::new("ovn0".into(), "ops".into()));
        let manager = app.network_services.as_mut().unwrap();
        manager.tab = NetworkServicesTab::Acls;
        manager.acls = vec![NetworkAclSummary {
            name: "web".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)),
            AppCommand::ReadNetworkAclLog {
                network: "ovn0".into(),
                project: "ops".into(),
                name: "web".into()
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT));
        for c in "frontend".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::RenameNetworkAcl {
                network: "ovn0".into(),
                project: "ops".into(),
                name: "web".into(),
                new_name: "frontend".into()
            }
        );
        let manager = app.network_services.as_mut().unwrap();
        manager.rename_input = None;
        manager.tab = NetworkServicesTab::LoadBalancers;
        manager.load_balancers = vec![NetworkLoadBalancerSummary {
            listen_address: "10.0.0.10".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)),
            AppCommand::ReadNetworkLoadBalancerState {
                network: "ovn0".into(),
                project: "ops".into(),
                address: "10.0.0.10".into()
            }
        );
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)),
            AppCommand::ReadNetworkState {
                network: "ovn0".into(),
                project: "ops".into(),
            }
        );
        let state = format_network_state(&serde_json::json!({
            "state":"up", "hwaddr":"00:16:3e:00:00:01", "mtu":1500,
            "addresses":[{"family":"inet","address":"10.0.0.1","netmask":"24","scope":"global"}],
            "counters":{"bytes_received":100,"packets_dropped_inbound":2},
            "bond":{"mode":"802.3ad","members":["eno1","eno2"]},
            "ovn":{"chassis":"node-a","logical_router":"lr-default"}
        }));
        assert!(state.contains("Bond / LACP"));
        assert!(state.contains("OVN router / chassis"));
        assert!(state.contains("packets_dropped_inbound"));
    }

    #[test]
    fn network_services_builds_a_structured_load_balancer() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["network_load_balancer_health_check".into()],
            ..Default::default()
        });
        app.network_services = Some(NetworkServicesManager::new("ovn0".into(), "ops".into()));
        let manager = app.network_services.as_mut().expect("network services");
        manager.tab = NetworkServicesTab::LoadBalancers;
        manager.form = Some(NetworkServiceForm::LoadBalancer {
            fields: [
                "10.0.0.10".into(),
                "web".into(),
                "tcp".into(),
                "443".into(),
                "web1".into(),
                "10.0.1.10".into(),
                "8443".into(),
                "true".into(),
                "15s".into(),
                "4s".into(),
                "4".into(),
                "2".into(),
            ],
            field: 11,
            editing: false,
        });
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateNetworkLoadBalancer(NetworkLoadBalancerSpec {
                network: "ovn0".into(),
                project: "ops".into(),
                listen_address: "10.0.0.10".into(),
                description: "web".into(),
                backend: NetworkLoadBalancerBackend {
                    name: "web1".into(),
                    target_address: "10.0.1.10".into(),
                    target_port: "8443".into()
                },
                port: NetworkLoadBalancerPort {
                    protocol: "tcp".into(),
                    listen_port: "443".into(),
                    target_backend: vec!["web1".into()]
                },
                config: std::collections::BTreeMap::from([
                    ("healthcheck".into(), "true".into()),
                    ("healthcheck.interval".into(), "15s".into()),
                    ("healthcheck.timeout".into(), "4s".into()),
                    ("healthcheck.failure_count".into(), "4".into()),
                    ("healthcheck.success_count".into(), "2".into()),
                ]),
            })
        );
    }

    #[test]
    fn network_forward_edit_preserves_additional_ports_and_config() {
        let mut app = App::new("local".into());
        let mut manager = NetworkServicesManager::new("incus0".into(), "ops".into());
        manager.tab = NetworkServicesTab::Forwards;
        manager.forwards = vec![NetworkForwardSummary {
            listen_address: "192.0.2.10".into(),
            description: "old".into(),
            ports: vec![
                NetworkForwardPort {
                    protocol: "tcp".into(),
                    listen_port: "80".into(),
                    target_address: "10.0.0.2".into(),
                    target_port: "8080".into(),
                },
                NetworkForwardPort {
                    protocol: "tcp".into(),
                    listen_port: "443".into(),
                    target_address: "10.0.0.3".into(),
                    target_port: "8443".into(),
                },
            ],
            config: BTreeMap::from([("user.note".into(), "keep".into())]),
            ..Default::default()
        }];
        app.network_services = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(NetworkServiceForm::Forward { fields, field, .. }) =
            app.network_services.as_mut().and_then(|m| m.form.as_mut())
        {
            fields[1] = "updated".into();
            *field = 5;
        }
        let AppCommand::UpdateNetworkForward(spec) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected forward update")
        };
        assert_eq!(spec.description, "updated");
        assert_eq!(spec.ports.len(), 2);
        assert_eq!(spec.ports[1].listen_port, "443");
        assert_eq!(spec.config["user.note"], "keep");
    }

    #[test]
    fn network_load_balancer_edit_preserves_unedited_entries_and_config() {
        let mut app = App::new("local".into());
        let mut manager = NetworkServicesManager::new("ovn0".into(), "ops".into());
        manager.tab = NetworkServicesTab::LoadBalancers;
        manager.load_balancers = vec![NetworkLoadBalancerSummary {
            listen_address: "10.0.0.10".into(),
            description: "old".into(),
            backends: vec![
                NetworkLoadBalancerBackend {
                    name: "web1".into(),
                    target_address: "10.0.1.10".into(),
                    target_port: "8443".into(),
                },
                NetworkLoadBalancerBackend {
                    name: "web2".into(),
                    target_address: "10.0.1.11".into(),
                    target_port: "8443".into(),
                },
            ],
            ports: vec![
                NetworkLoadBalancerPort {
                    protocol: "tcp".into(),
                    listen_port: "443".into(),
                    target_backend: vec!["web1".into()],
                },
                NetworkLoadBalancerPort {
                    protocol: "tcp".into(),
                    listen_port: "80".into(),
                    target_backend: vec!["web2".into()],
                },
            ],
            config: BTreeMap::from([("user.note".into(), "keep".into())]),
            ..Default::default()
        }];
        app.network_services = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(NetworkServiceForm::LoadBalancer { fields, field, .. }) = app
            .network_services
            .as_mut()
            .and_then(|manager| manager.form.as_mut())
        {
            fields[1] = "updated".into();
            *field = 11;
        }
        let AppCommand::UpdateNetworkLoadBalancer(spec) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected load-balancer update")
        };
        assert_eq!(spec.description, "updated");
        assert_eq!(spec.backends.len(), 2);
        assert_eq!(spec.backends[1].name, "web2");
        assert_eq!(spec.ports.len(), 2);
        assert_eq!(spec.ports[1].listen_port, "80");
        assert_eq!(spec.config["user.note"], "keep");
    }

    #[test]
    fn network_peer_edit_preserves_config() {
        let mut app = App::new("local".into());
        let mut manager = NetworkServicesManager::new("ovn0".into(), "ops".into());
        manager.tab = NetworkServicesTab::Peers;
        manager.peers = vec![NetworkPeerSummary {
            name: "remote".into(),
            description: "old".into(),
            target_project: "ops".into(),
            target_network: "ovn1".into(),
            config: BTreeMap::from([("user.note".into(), "keep".into())]),
            ..Default::default()
        }];
        app.network_services = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        if let Some(NetworkServiceForm::Peer { fields, field, .. }) = app
            .network_services
            .as_mut()
            .and_then(|manager| manager.form.as_mut())
        {
            fields[1] = "updated".into();
            *field = 5;
        }
        let AppCommand::UpdateNetworkPeer(spec) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected peer update")
        };
        assert_eq!(spec.description, "updated");
        assert_eq!(spec.peer_type, "local");
        assert_eq!(spec.config["user.note"], "keep");
    }

    #[test]
    fn network_peer_form_builds_a_remote_integration_peer() {
        let mut app = App::new("local".into());
        let mut manager = NetworkServicesManager::new("ovn0".into(), "ops".into());
        manager.tab = NetworkServicesTab::Peers;
        app.network_services = Some(manager);
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let Some(NetworkServiceForm::Peer { fields, field, .. }) = app
            .network_services
            .as_mut()
            .and_then(|manager| manager.form.as_mut())
        else {
            panic!("expected peer form")
        };
        fields[0] = "region".into();
        fields[2] = "remote".into();
        fields[5] = "ovn-region".into();
        *field = 5;

        let AppCommand::CreateNetworkPeer(spec) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected remote peer creation")
        };
        assert_eq!(spec.peer_type, "remote");
        assert_eq!(spec.target_integration, "ovn-region");
        assert!(spec.target_network.is_empty());
    }

    #[test]
    fn network_allocation_explorer_supports_filtering_and_navigation() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["network_allocations".into()],
            ..Default::default()
        });
        app.page = Page::Networks;
        app.model.network_allocations = vec![
            serde_json::json!({"address":"10.0.0.2","project":"ops"}),
            serde_json::json!({"address":"10.0.0.3","project":"dev"}),
        ];
        app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for c in "ops".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let explorer = app.allocation_explorer().expect("allocation explorer");
        assert_eq!(explorer.filter(), "ops");
        assert_eq!(explorer.selected(), 0);
    }

    #[test]
    fn metrics_explorer_filters_to_the_selected_instance() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["metrics".into()],
            ..Default::default()
        });
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.model.metrics = Some(
            concat!(
                "# HELP incus_cpu_seconds_total CPU\n",
                "incus_cpu_seconds_total{name=\"demo\",project=\"ops\",cpu=\"0\"} 4.2\n",
                "incus_cpu_seconds_total{name=\"other\",project=\"ops\",cpu=\"0\"} 9.1\n",
                "incus_memory_MemTotal_bytes{name=\"demo\",project=\"ops\"} 1024\n",
            )
            .into(),
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT));
        let explorer = app.metrics_explorer().expect("metrics explorer");
        let rows = metric_rows(app.model.metrics.as_deref().unwrap(), explorer);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, "4.2");
    }

    #[test]
    fn maintenance_manager_builds_etag_guarded_metadata_and_template_updates() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec![
                "container_edit_metadata".into(),
                "image_template_permissions".into(),
            ],
            ..Default::default()
        });
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('U'), KeyModifiers::SHIFT)),
            AppCommand::ReadInstanceMaintenance {
                name: "demo".into(),
                project: "ops".into()
            }
        );
        app.maintenance_manager = Some(InstanceMaintenanceManager {
            name: "demo".into(),
            project: "ops".into(),
            tab: MaintenanceTab::Metadata,
            metadata: serde_json::json!({
                "properties":{"os":"Linux"},
                "templates":{"/etc/hosts":{"template":"hosts.tpl","when":["create"],"uid":0,"gid":0,"mode":420}}
            }),
            metadata_etag: Some("etag-1".into()),
            templates: vec!["hosts.tpl".into()],
            selected: 0,
            metadata_editor: Some(("release".into(), "13".into(), 1)),
            template_editor: None,
            template_permissions: None,
            delete_input: None,
            dump_path: None,
            screenshot_path: None,
            repair_input: None,
            protocol_form: None,
            proxy_address: None,
            bitmap_form: None,
        });
        let command = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let AppCommand::UpdateInstanceMetadata { metadata, etag, .. } = command else {
            panic!("metadata update")
        };
        assert_eq!(etag, "etag-1");
        assert_eq!(metadata["properties"]["os"], "Linux");
        assert_eq!(metadata["properties"]["release"], "13");

        let manager = app.maintenance_manager.as_mut().unwrap();
        manager.metadata_editor = None;
        manager.tab = MaintenanceTab::Templates;
        manager.template_editor = Some(("hosts.tpl".into(), "{{ container.name }}".into(), 1));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            AppCommand::WriteInstanceTemplate {
                name: "demo".into(),
                project: "ops".into(),
                template: "hosts.tpl".into(),
                content: "{{ container.name }}".into(),
            }
        );
        let manager = app.maintenance_manager.as_mut().expect("maintenance");
        manager.template_editor = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT));
        let permissions = app
            .maintenance_manager
            .as_mut()
            .and_then(|manager| manager.template_permissions.as_mut())
            .expect("template permissions");
        assert_eq!(permissions.0, "/etc/hosts");
        assert_eq!(permissions.3, "0644");
        permissions.1 = "1000".into();
        permissions.2 = "1000".into();
        permissions.3 = "0640".into();
        permissions.4 = 3;
        let AppCommand::UpdateInstanceMetadata { metadata, etag, .. } =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("template permission metadata update");
        };
        assert_eq!(etag, "etag-1");
        assert_eq!(metadata["templates"]["/etc/hosts"]["uid"], 1000);
        assert_eq!(metadata["templates"]["/etc/hosts"]["gid"], 1000);
        assert_eq!(metadata["templates"]["/etc/hosts"]["mode"], 416);
    }

    #[test]
    fn instance_and_volume_protocol_forms_build_loopback_proxy_commands() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec![
                "instance_nbd".into(),
                "custom_volume_sftp".into(),
                "file_storage_volume".into(),
                "storage_volume_state".into(),
                "instance_console_screenshot".into(),
            ],
            ..Default::default()
        });
        app.model.instances = vec![InstanceSummary {
            name: "vm1".into(),
            project: "ops".into(),
            instance_type: "virtual-machine".into(),
            ..Default::default()
        }];
        app.maintenance_manager = Some(InstanceMaintenanceManager {
            name: "vm1".into(),
            project: "ops".into(),
            tab: MaintenanceTab::Diagnostics,
            metadata: serde_json::json!({}),
            metadata_etag: None,
            templates: vec![],
            selected: 0,
            metadata_editor: None,
            template_editor: None,
            template_permissions: None,
            delete_input: None,
            dump_path: None,
            screenshot_path: None,
            repair_input: None,
            protocol_form: None,
            proxy_address: None,
            bitmap_form: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::StartInstanceProtocolProxy {
                name: "vm1".into(),
                project: "ops".into(),
                protocol: "nbd".into(),
                port: 0,
            }
        );
        app.maintenance_manager.as_mut().unwrap().protocol_form = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ExportInstanceScreenshot {
                name: "vm1".into(),
                project: "ops".into(),
                path: "./vm1-console.png".into(),
            }
        );

        app.maintenance_manager = None;
        app.volume_manager = Some(VolumeManager::new("fast".into()));
        app.volume_manager.as_mut().unwrap().volumes = vec![StorageVolumeSummary {
            name: "data".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::StartStorageVolumeProtocolProxy {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "data".into(),
                protocol: "sftp".into(),
                port: 0,
            }
        );
        app.volume_manager.as_mut().unwrap().protocol_form = None;
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)),
            AppCommand::ReadStorageVolumeState {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "data".into(),
            }
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT));
        app.volume_manager
            .as_mut()
            .unwrap()
            .file_form
            .as_mut()
            .unwrap()
            .path = "/etc".into();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            AppCommand::ListStorageVolumeDirectory {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "data".into(),
                path: "/etc".into()
            }
        );
    }

    #[test]
    fn extension_gating_explains_unsupported_controls() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.server = Some(ServerInfo::default());
        app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
        assert!(app.allocation_explorer().is_none());
        assert!(
            app.model
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("network_allocations")
        );
    }

    #[test]
    fn configuration_form_hides_fields_for_missing_api_extensions() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["instance_auto_restart".into()],
            ..Default::default()
        });
        app.load_configuration_form(ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "vm1".into(),
            project: "default".into(),
            value: serde_json::json!({"config":{}}),
            etag: None,
        });
        let form = app.config_form.as_ref().unwrap();
        assert!(
            form.fields
                .iter()
                .any(|field| matches!(field.key, ConfigKey::Config("boot.autorestart")))
        );
        assert!(
            !form
                .fields
                .iter()
                .any(|field| matches!(field.key, ConfigKey::Config("raw.qemu.scriptlet")))
        );
        assert!(form.omitted_capabilities.contains(&"qemu_scriptlet"));
        assert!(
            form.omitted_capabilities
                .contains(&"container_migration_stateful")
        );
    }

    #[test]
    fn network_zone_manager_builds_a_structured_zone() {
        let mut app = App::new("local".into());
        app.zone_manager = Some(ZoneManager::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "example.test".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateNetworkZone {
                project: "default".into(),
                name: "example.test".into(),
                description: String::new()
            }
        );
    }

    #[test]
    fn bgp_form_builds_an_etag_guarded_peer_update() {
        let mut app = App::new("local".into());
        app.bgp_form = Some(BgpForm::new(ResourceDefinition {
            kind: ResourceKind::Network,
            name: "uplink".into(),
            project: "default".into(),
            value: serde_json::json!({"config": {}}),
            etag: Some("etag-bgp".into()),
        }));
        let form = app.bgp_form.as_mut().expect("BGP form");
        form.fields[0] = "router1".into();
        form.fields[1] = "192.0.2.1".into();
        form.fields[3] = "64513".into();
        form.field = 9;
        let AppCommand::UpdateResource { definition, value } =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected BGP update")
        };
        assert_eq!(definition.etag.as_deref(), Some("etag-bgp"));
        assert_eq!(value["config"]["bgp.peers.router1.address"], "192.0.2.1");
        assert_eq!(value["config"]["bgp.peers.router1.asn"], "64513");
    }

    #[test]
    fn acl_rule_editor_builds_a_structured_ingress_update() {
        let mut app = App::new("local".into());
        let mut manager = NetworkServicesManager::new("ovn0".into(), "ops".into());
        manager.tab = NetworkServicesTab::Acls;
        manager.acl_rules = Some(AclRulesForm {
            acl: NetworkAclSummary {
                name: "web".into(),
                project: "ops".into(),
                ..Default::default()
            },
            ingress: true,
            selected: 0,
            editor: Some(AclRuleEditor {
                original: None,
                fields: [
                    "allow".into(),
                    "@external".into(),
                    "".into(),
                    "tcp".into(),
                    "".into(),
                    "443".into(),
                    "".into(),
                    "".into(),
                    "HTTPS".into(),
                    "enabled".into(),
                ],
                field: 9,
            }),
            delete_input: None,
        });
        app.network_services = Some(manager);
        let AppCommand::UpdateNetworkAcl(acl) =
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected ACL update")
        };
        assert_eq!(
            acl.ingress,
            vec![NetworkAclRule {
                action: "allow".into(),
                source: "@external".into(),
                protocol: "tcp".into(),
                destination_port: "443".into(),
                description: "HTTPS".into(),
                state: "enabled".into(),
                ..Default::default()
            }]
        );
    }

    #[test]
    fn address_set_manager_builds_a_structured_create_request() {
        let mut app = App::new("local".into());
        app.address_set_manager = Some(AddressSetManager::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "trusted".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        for c in "192.0.2.0/24,2001:db8::/32".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateNetworkAddressSet {
                project: "default".into(),
                name: "trusted".into(),
                description: String::new(),
                addresses: vec!["192.0.2.0/24".into(), "2001:db8::/32".into()]
            }
        );
    }

    #[test]
    fn integration_manager_builds_structured_ovn_configuration() {
        let mut app = App::new("local".into());
        app.integration_manager = Some(IntegrationManager::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        if let Some(editor) = app
            .integration_manager
            .as_mut()
            .and_then(|m| m.editor.as_mut())
        {
            editor.fields[0] = "region".into();
            editor.fields[2] = "remote region".into();
            editor.fields[3] = "tcp:192.0.2.10:6645".into();
            editor.fields[4] = "tcp:192.0.2.10:6646".into();
            editor.fields[8] = "ts-{{ integrationName }}".into();
            editor.field = 8;
        }
        let AppCommand::CreateNetworkIntegration {
            name,
            integration_type,
            config,
            ..
        } = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("expected integration create")
        };
        assert_eq!(name, "region");
        assert_eq!(integration_type, "ovn");
        assert_eq!(config["ovn.northbound_connection"], "tcp:192.0.2.10:6645");
        assert_eq!(config["ovn.transit.pattern"], "ts-{{ integrationName }}");
    }

    #[test]
    fn unmanaged_network_rejects_service_controls() {
        let mut app = App::new("local".into());
        app.page = Page::Networks;
        app.model.networks = vec![NetworkSummary {
            name: "eth0".into(),
            managed: false,
            ..Default::default()
        }];

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT)),
            AppCommand::None
        );
        assert!(app.network_services().is_none());
        assert!(
            app.model
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("unmanaged")
        );
    }

    #[test]
    fn bulk_manager_defaults_to_dry_run_and_filters_invalid_targets() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![
            InstanceSummary {
                name: "stopped".into(),
                project: "default".into(),
                status: "Stopped".into(),
                ..Default::default()
            },
            InstanceSummary {
                name: "running".into(),
                project: "ops".into(),
                status: "Running".into(),
                ..Default::default()
            },
        ];
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT));
        assert!(app.bulk_manager().expect("bulk manager").dry_run());
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let manager = app.bulk_manager().expect("bulk manager");
        assert_eq!(manager.selected_count(), 2);
        assert_eq!(manager.eligible_count(), 1);
        assert!(
            manager
                .results()
                .iter()
                .any(|line| line.starts_with("PASS"))
        );
        assert!(
            manager
                .results()
                .iter()
                .any(|line| line.starts_with("SKIP"))
        );
    }

    #[test]
    fn bulk_manager_requires_exact_typed_confirmation_before_apply() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "default".into(),
            status: "Stopped".into(),
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for character in "apply bulk start to 1 instances".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::ApplyBulkInstances {
                targets: vec![BulkTarget {
                    name: "demo".into(),
                    project: "default".into(),
                    status: "Stopped".into(),
                    selected: true,
                }],
                action: InstanceAction::Start,
                concurrency: 4,
            }
        );
    }

    #[test]
    fn running_bulk_manager_accepts_cooperative_cancellation() {
        let mut app = App::new("local".into());
        app.bulk_manager = Some(BulkManager::new(&[]));
        app.bulk_manager.as_mut().unwrap().running = true;
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.bulk_cancel = Some(cancel.clone());
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
        assert!(app.bulk_manager.as_ref().unwrap().cancelled);
    }

    #[test]
    fn volume_bitmap_manager_builds_structured_create_and_guarded_delete() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["storage_volume_nbd".into()],
            ..Default::default()
        });
        app.volume_manager = Some(VolumeManager::new("fast".into()));
        app.volume_manager.as_mut().unwrap().volumes = vec![StorageVolumeSummary {
            name: "vm-disk".into(),
            project: "ops".into(),
            content_type: "block".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT)),
            AppCommand::ReadStorageVolumeBitmaps {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "vm-disk".into()
            }
        );
        app.volume_manager
            .as_mut()
            .unwrap()
            .bitmaps
            .as_mut()
            .unwrap()
            .bitmaps = vec![StorageVolumeBitmap {
            name: "nightly".into(),
            persistent: true,
            recording: true,
            granularity: 32768,
            ..Default::default()
        }];
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        for c in "incremental".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateStorageVolumeBitmap {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "vm-disk".into(),
                spec: StorageVolumeBitmapSpec {
                    name: "incremental".into(),
                    persistent: true,
                    disabled: false,
                    granularity: 32768
                }
            }
        );
        app.volume_manager
            .as_mut()
            .unwrap()
            .bitmaps
            .as_mut()
            .unwrap()
            .create = None;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        for c in "delete bitmap nightly".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::DeleteStorageVolumeBitmap {
                pool: "fast".into(),
                project: "ops".into(),
                volume: "vm-disk".into(),
                bitmap: "nightly".into()
            }
        );
    }

    #[test]
    fn vm_maintenance_builds_coordinated_dependent_volume_bitmaps() {
        let mut app = App::new("local".into());
        app.model.server = Some(ServerInfo {
            api_extensions: vec!["storage_volume_nbd".into()],
            ..Default::default()
        });
        app.model.instances = vec![InstanceSummary {
            name: "vm01".into(),
            project: "ops".into(),
            instance_type: "virtual-machine".into(),
            ..Default::default()
        }];
        app.maintenance_manager = Some(InstanceMaintenanceManager {
            name: "vm01".into(),
            project: "ops".into(),
            tab: MaintenanceTab::Diagnostics,
            metadata: serde_json::json!({}),
            metadata_etag: None,
            templates: Vec::new(),
            selected: 0,
            metadata_editor: None,
            template_editor: None,
            template_permissions: None,
            delete_input: None,
            dump_path: None,
            screenshot_path: None,
            repair_input: None,
            protocol_form: None,
            proxy_address: None,
            bitmap_form: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        for c in "backup-set".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppCommand::CreateInstanceBitmaps {
                name: "vm01".into(),
                project: "ops".into(),
                spec: StorageVolumeBitmapSpec {
                    name: "backup-set".into(),
                    persistent: true,
                    disabled: false,
                    granularity: 32768
                }
            }
        );
    }

    #[test]
    fn instance_snapshot_policy_uses_choices_and_live_retention_preview() {
        let mut app = App::new("local".into());
        app.page = Page::Instances;
        app.model.instances = vec![InstanceSummary {
            name: "demo".into(),
            project: "ops".into(),
            ..Default::default()
        }];
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT)),
            AppCommand::ReadResourceDefinition {
                kind: ResourceKind::Instance,
                name: "demo".into(),
                project: "ops".into()
            }
        );
        app.load_configuration_form(ResourceDefinition {
            kind: ResourceKind::Instance,
            name: "demo".into(),
            project: "ops".into(),
            value: serde_json::json!({"config":{"snapshots.pattern":"snap-%d"}}),
            etag: Some("etag-policy".into()),
        });
        let form = app.config_form.as_ref().expect("snapshot policy");
        assert!(matches!(
            form.fields[form.field].key,
            ConfigKey::Config("snapshots.schedule")
        ));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        let preview = app
            .config_form
            .as_ref()
            .unwrap()
            .snapshot_policy_preview()
            .unwrap();
        assert!(preview.contains("@hourly"));
        assert!(preview.contains("snap-%d"));
    }
}
