//! Terminal application lifecycle and rendering.

mod app;
mod theme;
mod ui;

pub use app::{App, AppCommand, Page};
use app::{ClusterMemberUpdate, FleetRow};

use std::io;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::incus::{IncusClient, ResourceKind};

/// Runs the interactive virtty application.
pub async fn run(mut client: IncusClient) -> io::Result<()> {
    let mut terminal = TerminalSession::enter()?;
    let mut app = App::new(client.transport().label());
    match crate::remotes::load() {
        Ok(remotes) => app.set_saved_remotes(remotes),
        Err(error) => app.model.last_error = Some(format!("cannot load saved remotes: {error}")),
    }
    match crate::views::load() {
        Ok(views) => app.set_saved_views(views),
        Err(error) => app.model.last_error = Some(format!("cannot load saved views: {error}")),
    }
    match crate::journal::load() {
        Ok(entries) => app.set_action_journal(entries),
        Err(error) => app.model.last_error = Some(format!("cannot load action journal: {error}")),
    }
    match crate::schedules::load() {
        Ok(schedules) => app.set_backup_schedules(schedules),
        Err(error) => app.model.last_error = Some(format!("cannot load backup schedules: {error}")),
    }
    let mut events = client.subscribe_events();
    app.refresh(&client).await;

    while !app.should_quit() {
        let mut refresh_from_event = false;
        while let Ok(event) = events.try_recv() {
            refresh_from_event |= app.record_event(event);
        }
        if refresh_from_event {
            app.refresh(&client).await;
        }
        if app.poll_bulk_progress() {
            app.refresh(&client).await;
        }
        let due_backups = app.take_due_backups();
        if !due_backups.is_empty() {
            if let Err(error) = crate::schedules::save(app.backup_schedules()) {
                app.model.last_error = Some(format!("cannot save backup schedules: {error}"));
            }
            for (name, project, backup_name, instance_only, optimized_storage) in due_backups {
                app.create_backup(
                    &client,
                    name,
                    project,
                    backup_name,
                    instance_only,
                    optimized_storage,
                )
                .await;
            }
        }
        terminal.terminal.draw(|frame| ui::render(frame, &app))?;

        if event::poll(std::time::Duration::from_millis(150))?
            && let Event::Key(key) = event::read()?
        {
            let command = app.handle_key(key);
            if let Some(entries) = app.record_submitted_action(&command)
                && let Err(error) = crate::journal::save(&entries)
            {
                app.model.last_error = Some(format!("cannot save action journal: {error}"));
            }
            match command {
                AppCommand::None => {}
                AppCommand::Refresh => app.refresh(&client).await,
                AppCommand::Connect(transport) => match IncusClient::from_transport(transport) {
                    Ok(next_client) => {
                        client = next_client;
                        events = client.subscribe_events();
                        app.set_target(client.transport().label());
                        app.refresh(&client).await;
                    }
                    Err(error) => app.model.last_error = Some(error.to_string()),
                },
                AppCommand::PersistRemotes(remotes) => match crate::remotes::save(&remotes) {
                    Ok(()) => app.model.last_error = None,
                    Err(error) => {
                        app.model.last_error =
                            Some(format!("cannot save remote inventory: {error}"))
                    }
                },
                AppCommand::PersistViews(views) => match crate::views::save(&views) {
                    Ok(()) => app.model.last_error = None,
                    Err(error) => {
                        app.model.last_error = Some(format!("cannot save views: {error}"))
                    }
                },
                AppCommand::PersistSchedules(schedules) => match crate::schedules::save(&schedules)
                {
                    Ok(()) => app.model.last_error = None,
                    Err(error) => {
                        app.model.last_error =
                            Some(format!("cannot save backup schedules: {error}"))
                    }
                },
                AppCommand::ReadFleetHealth(remotes) => {
                    app.load_fleet_health(read_fleet_health(remotes).await);
                }
                AppCommand::ReadOpenFga => {
                    match client
                        .resource_definition(ResourceKind::Server, "", "")
                        .await
                    {
                        Ok(definition) => {
                            let get = |key| {
                                definition
                                    .value
                                    .get(key)
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned()
                            };
                            match crate::openfga::OpenFgaClient::new(
                                get("authorization.openfga.api.url"),
                                get("authorization.openfga.store.id"),
                                get("authorization.openfga.api.token"),
                            ) {
                                Ok(fga) => match fga.read().await {
                                    Ok(tuples) => app.load_openfga(definition, tuples),
                                    Err(error) => app.model.last_error = Some(error),
                                },
                                Err(error) => app.model.last_error = Some(error),
                            }
                        }
                        Err(error) => app.model.last_error = Some(error.to_string()),
                    }
                }
                AppCommand::RefreshOpenFga => {
                    if let Some((url, store, token)) = app.openfga_connection() {
                        match crate::openfga::OpenFgaClient::new(url, store, token) {
                            Ok(fga) => match fga.read().await {
                                Ok(tuples) => app.load_openfga_tuples(tuples),
                                Err(error) => app.model.last_error = Some(error),
                            },
                            Err(error) => app.model.last_error = Some(error),
                        }
                    }
                }
                AppCommand::MutateOpenFga { tuple, delete } => {
                    if let Some((url, store, token)) = app.openfga_connection() {
                        match crate::openfga::OpenFgaClient::new(url, store, token) {
                            Ok(fga) => match fga.write(&tuple, delete).await {
                                Ok(()) => match fga.read().await {
                                    Ok(tuples) => app.load_openfga_tuples(tuples),
                                    Err(error) => app.model.last_error = Some(error),
                                },
                                Err(error) => app.model.last_error = Some(error),
                            },
                            Err(error) => app.model.last_error = Some(error),
                        }
                    }
                }
                AppCommand::CheckOpenFga { tuple } => {
                    if let Some((url, store, token)) = app.openfga_connection() {
                        match crate::openfga::OpenFgaClient::new(url, store, token) {
                            Ok(fga) => match fga.check(&tuple).await {
                                Ok(allowed) => app.set_openfga_result(format!(
                                    "effective entitlement: {}",
                                    if allowed { "ALLOWED" } else { "DENIED" }
                                )),
                                Err(error) => app.model.last_error = Some(error),
                            },
                            Err(error) => app.model.last_error = Some(error),
                        }
                    }
                }
                AppCommand::ReadImageRetention => {
                    match client
                        .resource_definition(ResourceKind::Server, "", "")
                        .await
                    {
                        Ok(definition) => app.load_image_retention(definition),
                        Err(error) => app.model.last_error = Some(error.to_string()),
                    }
                }
                AppCommand::ExportAudit { path, value } => app.export_audit(path, value).await,
                AppCommand::ChangeInstanceState {
                    name,
                    project,
                    action,
                } => {
                    app.execute_instance_action(&client, name, project, action)
                        .await;
                }
                AppCommand::CreateInstance(spec) => app.create_instance(&client, spec).await,
                AppCommand::ExecuteCommand {
                    name,
                    project,
                    command,
                } => {
                    app.execute_command(&client, name, project, command).await;
                }
                AppCommand::InteractiveExec {
                    name,
                    project,
                    shell,
                } => {
                    terminal.suspend()?;
                    println!("virtty interactive shell: Ctrl-] detaches\r");
                    let result = client.interactive_exec(&name, &project, &shell).await;
                    terminal.resume()?;
                    app.record_interactive_result(result);
                    app.refresh(&client).await;
                }
                AppCommand::InteractiveConsole {
                    name,
                    project,
                    force,
                } => {
                    terminal.suspend()?;
                    println!("virtty serial console: Ctrl-] detaches\r");
                    let result = client.interactive_console(&name, &project, force).await;
                    terminal.resume()?;
                    app.record_interactive_result(result);
                    app.refresh(&client).await;
                }
                AppCommand::VgaConsole {
                    name,
                    project,
                    force,
                } => {
                    let result = client.vga_console(&name, &project, force).await;
                    app.record_interactive_result(result);
                    app.refresh(&client).await;
                }
                AppCommand::CreateSnapshot {
                    name,
                    project,
                    snapshot_name,
                    stateful,
                } => {
                    app.create_snapshot(&client, name, project, snapshot_name, stateful)
                        .await;
                }
                AppCommand::CreateBackup {
                    name,
                    project,
                    backup_name,
                    instance_only,
                    optimized_storage,
                } => {
                    app.create_backup(
                        &client,
                        name,
                        project,
                        backup_name,
                        instance_only,
                        optimized_storage,
                    )
                    .await;
                }
                AppCommand::ReadConsole { name, project } => {
                    app.read_console(&client, name, project).await;
                }
                AppCommand::ReadInstanceLogs { name, project } => {
                    app.load_instance_logs(&client, name, project).await;
                }
                AppCommand::ReadInstanceLog {
                    name,
                    project,
                    filename,
                } => {
                    app.read_instance_log(&client, name, project, filename)
                        .await;
                }
                AppCommand::DeleteInstanceLog {
                    name,
                    project,
                    filename,
                } => {
                    app.delete_instance_log(&client, name, project, filename)
                        .await;
                }
                AppCommand::CleanupInstanceExecOutputs {
                    name,
                    project,
                    filenames,
                } => {
                    app.cleanup_instance_exec_outputs(&client, name, project, filenames)
                        .await;
                }
                AppCommand::ClearConsoleLog { name, project } => {
                    app.clear_console_log(&client, name, project).await;
                }
                AppCommand::ReadInstanceMaintenance { name, project } => {
                    app.load_instance_maintenance(&client, name, project).await;
                }
                AppCommand::ReadInstanceTemplate {
                    name,
                    project,
                    template,
                } => {
                    app.read_instance_template(&client, name, project, template)
                        .await;
                }
                AppCommand::UpdateInstanceMetadata {
                    name,
                    project,
                    metadata,
                    etag,
                } => {
                    app.update_instance_metadata(&client, name, project, metadata, etag)
                        .await;
                }
                AppCommand::WriteInstanceTemplate {
                    name,
                    project,
                    template,
                    content,
                } => {
                    app.write_instance_template(&client, name, project, template, content)
                        .await;
                }
                AppCommand::DeleteInstanceTemplate {
                    name,
                    project,
                    template,
                } => {
                    app.delete_instance_template(&client, name, project, template)
                        .await;
                }
                AppCommand::ExportInstanceMemory {
                    name,
                    project,
                    path,
                } => {
                    app.export_instance_memory(&client, name, project, path)
                        .await;
                }
                AppCommand::ExportInstanceScreenshot {
                    name,
                    project,
                    path,
                } => {
                    app.export_instance_screenshot(&client, name, project, path)
                        .await;
                }
                AppCommand::RepairInstance { name, project } => {
                    app.repair_instance(&client, name, project).await;
                }
                AppCommand::CreateInstanceBitmaps {
                    name,
                    project,
                    spec,
                } => {
                    app.create_instance_bitmaps(&client, name, project, spec)
                        .await;
                }
                AppCommand::StartInstanceProtocolProxy {
                    name,
                    project,
                    protocol,
                    port,
                } => {
                    app.start_instance_protocol_proxy(&client, name, project, protocol, port)
                        .await;
                }
                AppCommand::ReadInstanceDetail { name, project } => {
                    app.read_instance_detail(&client, name, project).await;
                }
                AppCommand::ReadInstanceAccess { name, project } => {
                    app.load_instance_access(&client, name, project).await;
                }
                AppCommand::ReadResourceDefinition {
                    kind,
                    name,
                    project,
                } => match client.resource_definition(kind, &name, &project).await {
                    Ok(definition) => app.load_configuration_form(definition),
                    Err(error) => app.model.last_error = Some(error.to_string()),
                },
                AppCommand::ReadLoggingTargets => {
                    match client
                        .resource_definition(ResourceKind::Server, "", "")
                        .await
                    {
                        Ok(definition) => app.load_logging_targets(definition),
                        Err(error) => app.model.last_error = Some(error.to_string()),
                    }
                }
                AppCommand::ReadScriptlets => {
                    match client
                        .resource_definition(ResourceKind::Server, "", "")
                        .await
                    {
                        Ok(definition) => app.load_scriptlets(definition),
                        Err(error) => app.model.last_error = Some(error.to_string()),
                    }
                }
                AppCommand::ReadQemuScriptlet {
                    kind,
                    name,
                    project,
                } => match client.resource_definition(kind, &name, &project).await {
                    Ok(definition) => app.load_qemu_scriptlet(definition),
                    Err(error) => app.model.last_error = Some(error.to_string()),
                },
                AppCommand::UpdateScriptlet {
                    definition,
                    value,
                    activated_source,
                } => {
                    app.update_scriptlet(&client, definition, value, activated_source)
                        .await;
                }
                AppCommand::ApplyPreseed(spec) => {
                    app.apply_preseed(&client, spec).await;
                }
                AppCommand::ImportPreseed { path } => {
                    app.import_preseed(path).await;
                }
                AppCommand::ExportPreseed { path, spec } => {
                    app.export_preseed(path, spec).await;
                }
                AppCommand::ReadPoolSafety { pool } => {
                    let definition = client
                        .resource_definition(ResourceKind::StoragePool, &pool, "")
                        .await;
                    let resources = client.storage_pool_resources(&pool).await;
                    match (definition, resources) {
                        (Ok(definition), Ok(resources)) => {
                            app.load_pool_safety(definition, resources)
                        }
                        (Err(error), _) | (_, Err(error)) => {
                            app.model.last_error = Some(error.to_string())
                        }
                    }
                }
                AppCommand::ReadQmpManager { name, project } => {
                    match client
                        .resource_definition(ResourceKind::Instance, &name, &project)
                        .await
                    {
                        Ok(definition) => app.load_qmp_manager(definition),
                        Err(error) => app.model.last_error = Some(error.to_string()),
                    }
                }
                AppCommand::ApplyBulkInstances {
                    targets,
                    action,
                    concurrency,
                } => {
                    app.start_bulk_instances(&client, targets, action, concurrency);
                }
                AppCommand::ReadDeviceManager {
                    kind,
                    name,
                    project,
                } => match client.resource_definition(kind, &name, &project).await {
                    Ok(definition) => app.load_device_manager(definition),
                    Err(error) => app.model.last_error = Some(error.to_string()),
                },
                AppCommand::UpdateResource { definition, value } => {
                    app.update_resource(&client, definition, value).await;
                }
                AppCommand::CreateResource {
                    kind,
                    project,
                    value,
                } => app.create_resource(&client, kind, project, value).await,
                AppCommand::DeleteResource {
                    kind,
                    name,
                    project,
                } => app.delete_resource(&client, kind, name, project).await,
                AppCommand::CreateImageAlias {
                    alias,
                    fingerprint,
                    project,
                    description,
                } => {
                    app.create_image_alias(&client, alias, fingerprint, project, description)
                        .await;
                }
                AppCommand::UpdateImageAlias {
                    alias,
                    new_name,
                    fingerprint,
                    project,
                    description,
                } => {
                    app.update_image_alias(
                        &client,
                        alias,
                        new_name,
                        fingerprint,
                        project,
                        description,
                    )
                    .await
                }
                AppCommand::DeleteImageAlias { alias, project } => {
                    app.delete_image_alias(&client, alias, project).await
                }
                AppCommand::UpdateImage(spec) => app.update_image(&client, spec).await,
                AppCommand::ImportImage {
                    project,
                    path,
                    aliases,
                    public,
                } => {
                    app.import_image(&client, project, path, public, aliases)
                        .await
                }
                AppCommand::ExportImage {
                    fingerprint,
                    project,
                    path,
                } => app.export_image(&client, fingerprint, project, path).await,
                AppCommand::PushImage {
                    fingerprint,
                    project,
                    target,
                    target_project,
                    secret,
                    certificate_path,
                    aliases,
                    profiles,
                } => {
                    app.push_image(
                        &client,
                        fingerprint,
                        project,
                        target,
                        target_project,
                        secret,
                        certificate_path,
                        aliases,
                        profiles,
                    )
                    .await
                }
                AppCommand::RefreshImage {
                    fingerprint,
                    project,
                } => app.refresh_image(&client, fingerprint, project).await,
                AppCommand::CopyImageProject {
                    fingerprint,
                    source_project,
                    target_project,
                    public,
                } => {
                    app.copy_image_project(
                        &client,
                        fingerprint,
                        source_project,
                        target_project,
                        public,
                    )
                    .await
                }
                AppCommand::PublishInstanceImage(spec) => {
                    app.publish_instance_image(&client, spec).await
                }
                AppCommand::ReadArtifacts { name, project } => {
                    app.read_artifacts(&client, name, project).await;
                }
                AppCommand::DeleteSnapshot {
                    name,
                    project,
                    snapshot,
                } => app.delete_snapshot(&client, name, project, snapshot).await,
                AppCommand::RestoreSnapshot {
                    name,
                    project,
                    snapshot,
                    stateful,
                } => {
                    app.restore_snapshot(&client, name, project, snapshot, stateful)
                        .await;
                }
                AppCommand::RenameSnapshot {
                    name,
                    project,
                    snapshot,
                    new_name,
                } => {
                    app.rename_snapshot(&client, name, project, snapshot, new_name)
                        .await;
                }
                AppCommand::DeleteBackup {
                    name,
                    project,
                    backup,
                } => app.delete_backup(&client, name, project, backup).await,
                AppCommand::ExportBackup {
                    name,
                    project,
                    backup,
                    path,
                } => {
                    app.export_backup(&client, name, project, backup, path)
                        .await;
                }
                AppCommand::ImportBackup { project, path } => {
                    app.import_backup(&client, project, path).await;
                }
                AppCommand::RenameInstance {
                    name,
                    project,
                    new_name,
                } => app.rename_instance(&client, name, project, new_name).await,
                AppCommand::RebuildInstance {
                    name,
                    project,
                    image,
                } => app.rebuild_instance(&client, name, project, image).await,
                AppCommand::MoveInstance(spec) => app.move_instance(&client, spec).await,
                AppCommand::AcknowledgeWarning { uuid } => {
                    app.acknowledge_warning(&client, uuid).await;
                }
                AppCommand::DeleteWarning { uuid } => app.delete_warning(&client, uuid).await,
                AppCommand::DeleteCertificate { fingerprint } => {
                    app.delete_certificate(&client, fingerprint).await;
                }
                AppCommand::GenerateCertificateToken {
                    name,
                    restricted,
                    projects,
                } => {
                    app.generate_certificate_token(&client, name, restricted, projects)
                        .await;
                }
                AppCommand::AddCertificateFromFile {
                    name,
                    restricted,
                    projects,
                    path,
                } => {
                    app.add_certificate_from_file(&client, name, restricted, projects, path)
                        .await;
                }
                AppCommand::UpdateCertificate {
                    fingerprint,
                    name,
                    restricted,
                    projects,
                } => {
                    app.update_certificate(&client, fingerprint, name, restricted, projects)
                        .await;
                }
                AppCommand::UpdateClusterMember {
                    name,
                    description,
                    roles,
                    groups,
                    failure_domain,
                    config,
                } => {
                    app.update_cluster_member(
                        &client,
                        ClusterMemberUpdate {
                            name,
                            description,
                            roles,
                            groups,
                            failure_domain,
                            config,
                        },
                    )
                    .await;
                }
                AppCommand::RenameClusterMember { name, new_name } => {
                    app.rename_cluster_member(&client, name, new_name).await
                }
                AppCommand::ReadClusterMemberState { name } => {
                    app.read_cluster_member_state(&client, name).await
                }
                AppCommand::ReadClusterConfiguration => {
                    app.read_cluster_configuration(&client).await
                }
                AppCommand::ReadMetadataConfiguration => {
                    app.read_metadata_configuration(&client).await
                }
                AppCommand::ConfigureCluster { value } => {
                    app.configure_cluster(&client, value).await
                }
                AppCommand::UpdateClusterCertificate {
                    certificate_path,
                    key_path,
                } => {
                    app.update_cluster_certificate(&client, certificate_path, key_path)
                        .await
                }
                AppCommand::ChangeClusterMemberState { name, action } => {
                    app.change_cluster_member_state(&client, name, action).await;
                }
                AppCommand::DeleteClusterMember { name } => {
                    app.delete_cluster_member(&client, name).await;
                }
                AppCommand::GenerateClusterJoinToken { name } => {
                    app.generate_cluster_join_token(&client, name).await;
                }
                AppCommand::ReadClusterGroups => app.load_cluster_groups(&client).await,
                AppCommand::ReadProjectUsage { project } => {
                    app.load_project_usage(&client, project).await
                }
                AppCommand::ReadProjectAccess { project } => {
                    app.load_project_access(&client, project).await
                }
                command @ (AppCommand::CreateClusterGroup { .. }
                | AppCommand::UpdateClusterGroup { .. }
                | AppCommand::RenameClusterGroup { .. }
                | AppCommand::DeleteClusterGroup { .. }) => {
                    app.mutate_cluster_group(&client, command).await
                }
                AppCommand::ReadStorageVolumes { pool } => {
                    app.load_storage_volumes(&client, pool).await;
                }
                AppCommand::ReadStoragePoolResources { pool } => {
                    app.load_storage_pool_resources(&client, pool).await
                }
                AppCommand::ReadStorageVolumeState {
                    pool,
                    project,
                    volume,
                } => {
                    app.load_storage_volume_state(&client, pool, project, volume)
                        .await
                }
                AppCommand::ReadStorageVolumeBitmaps {
                    pool,
                    project,
                    volume,
                } => {
                    app.load_storage_volume_bitmaps(&client, pool, project, volume)
                        .await;
                }
                AppCommand::CreateStorageVolumeBitmap {
                    pool,
                    project,
                    volume,
                    spec,
                } => {
                    app.create_storage_volume_bitmap(&client, pool, project, volume, spec)
                        .await;
                }
                AppCommand::DeleteStorageVolumeBitmap {
                    pool,
                    project,
                    volume,
                    bitmap,
                } => {
                    app.delete_storage_volume_bitmap(&client, pool, project, volume, bitmap)
                        .await;
                }
                AppCommand::CreateStorageVolume(spec) => {
                    app.create_storage_volume(&client, spec).await;
                }
                AppCommand::ImportStorageVolume {
                    pool,
                    project,
                    name,
                    import_type,
                    path,
                } => {
                    app.import_storage_volume(&client, pool, project, name, import_type, path)
                        .await;
                }
                AppCommand::UpdateStorageVolume(spec) => {
                    app.update_storage_volume(&client, spec).await;
                }
                AppCommand::AttachStorageVolume(spec) => {
                    app.attach_storage_volume(&client, spec).await;
                }
                AppCommand::DetachStorageVolume(spec) => {
                    app.detach_storage_volume(&client, spec).await;
                }
                AppCommand::ReadStorageVolumeSnapshots {
                    pool,
                    project,
                    volume,
                } => {
                    app.load_storage_volume_snapshots(&client, pool, project, volume)
                        .await;
                }
                command @ (AppCommand::CreateStorageVolumeSnapshot { .. }
                | AppCommand::RenameStorageVolumeSnapshot { .. }
                | AppCommand::RestoreStorageVolumeSnapshot { .. }
                | AppCommand::DeleteStorageVolumeSnapshot { .. }) => {
                    app.mutate_storage_volume_snapshot(&client, command).await;
                }
                AppCommand::ReadStorageVolumeBackups {
                    pool,
                    project,
                    volume,
                } => {
                    app.load_storage_volume_backups(&client, pool, project, volume)
                        .await;
                }
                command @ (AppCommand::CreateStorageVolumeBackup { .. }
                | AppCommand::RenameStorageVolumeBackup { .. }
                | AppCommand::DeleteStorageVolumeBackup { .. }) => {
                    app.mutate_storage_volume_backup(&client, command).await;
                }
                AppCommand::ExportStorageVolumeBackup {
                    pool,
                    project,
                    volume,
                    backup,
                    path,
                } => {
                    app.export_storage_volume_backup(&client, pool, project, volume, backup, path)
                        .await;
                }
                AppCommand::ReadStorageBuckets { pool } => {
                    app.load_storage_buckets(&client, pool).await
                }
                command @ (AppCommand::CreateStorageBucket { .. }
                | AppCommand::UpdateStorageBucket { .. }
                | AppCommand::DeleteStorageBucket { .. }) => {
                    app.mutate_storage_bucket(&client, command).await;
                }
                AppCommand::ReadStorageBucketKeys {
                    pool,
                    project,
                    bucket,
                } => {
                    app.load_storage_bucket_keys(&client, pool, project, bucket)
                        .await;
                }
                command @ (AppCommand::CreateStorageBucketKey { .. }
                | AppCommand::UpdateStorageBucketKey { .. }
                | AppCommand::DeleteStorageBucketKey { .. }) => {
                    app.mutate_storage_bucket_key(&client, command).await;
                }
                AppCommand::ReadStorageBucketBackups {
                    pool,
                    project,
                    bucket,
                } => {
                    app.load_storage_bucket_backups(&client, pool, project, bucket)
                        .await;
                }
                command @ (AppCommand::CreateStorageBucketBackup { .. }
                | AppCommand::RenameStorageBucketBackup { .. }
                | AppCommand::DeleteStorageBucketBackup { .. }) => {
                    app.mutate_storage_bucket_backup(&client, command).await;
                }
                AppCommand::ExportStorageBucketBackup {
                    pool,
                    project,
                    bucket,
                    backup,
                    path,
                } => {
                    app.export_storage_bucket_backup(&client, pool, project, bucket, backup, path)
                        .await;
                }
                AppCommand::ImportStorageBucket {
                    pool,
                    project,
                    name,
                    path,
                } => {
                    app.import_storage_bucket(&client, pool, project, name, path)
                        .await;
                }
                AppCommand::DeleteStorageVolume {
                    pool,
                    project,
                    name,
                } => {
                    app.delete_storage_volume(&client, pool, project, name)
                        .await;
                }
                AppCommand::RebuildStorageVolume {
                    pool,
                    project,
                    name,
                } => {
                    app.rebuild_storage_volume(&client, pool, project, name)
                        .await;
                }
                AppCommand::CopyStorageVolume {
                    source_pool,
                    source_project,
                    source_name,
                    target_pool,
                    target_project,
                    target_name,
                    volume_only,
                    refresh,
                    live,
                    refresh_exclude_older,
                } => {
                    app.copy_storage_volume(
                        &client,
                        source_pool,
                        source_project,
                        source_name,
                        target_pool,
                        target_project,
                        target_name,
                        volume_only,
                        refresh,
                        live,
                        refresh_exclude_older,
                    )
                    .await;
                }
                AppCommand::StartStorageVolumeProtocolProxy {
                    pool,
                    project,
                    volume,
                    protocol,
                    port,
                } => {
                    app.start_storage_volume_protocol_proxy(
                        &client, pool, project, volume, protocol, port,
                    )
                    .await;
                }
                AppCommand::ReadStorageVolumeFile {
                    pool,
                    project,
                    volume,
                    path,
                } => {
                    app.read_storage_volume_file(&client, pool, project, volume, path)
                        .await
                }
                AppCommand::ListStorageVolumeDirectory {
                    pool,
                    project,
                    volume,
                    path,
                } => {
                    app.list_storage_volume_directory(&client, pool, project, volume, path)
                        .await
                }
                AppCommand::WriteStorageVolumeFile {
                    pool,
                    project,
                    volume,
                    path,
                    content,
                } => {
                    app.write_storage_volume_file(&client, pool, project, volume, path, content)
                        .await
                }
                AppCommand::DeleteStorageVolumeFile {
                    pool,
                    project,
                    volume,
                    path,
                } => {
                    app.delete_storage_volume_file(&client, pool, project, volume, path)
                        .await
                }
                AppCommand::PullStorageVolumeFile {
                    pool,
                    project,
                    volume,
                    path,
                    local_path,
                } => {
                    app.transfer_storage_volume_file(
                        &client, false, pool, project, volume, path, local_path,
                    )
                    .await
                }
                AppCommand::PushStorageVolumeFile {
                    pool,
                    project,
                    volume,
                    path,
                    local_path,
                } => {
                    app.transfer_storage_volume_file(
                        &client, true, pool, project, volume, path, local_path,
                    )
                    .await
                }
                AppCommand::ReadNetworkServices { network, project } => {
                    app.load_network_services(&client, network, project).await;
                }
                AppCommand::CreateNetworkForward(spec) => {
                    app.create_network_forward(&client, spec).await;
                }
                AppCommand::UpdateNetworkForward(spec) => {
                    app.update_network_forward(&client, spec).await;
                }
                AppCommand::DeleteNetworkForward {
                    network,
                    project,
                    listen_address,
                } => {
                    app.delete_network_forward(&client, network, project, listen_address)
                        .await;
                }
                AppCommand::CreateNetworkAcl(spec) => {
                    app.create_network_acl(&client, spec).await;
                }
                AppCommand::UpdateNetworkAcl(acl) => app.update_network_acl(&client, acl).await,
                AppCommand::DeleteNetworkAcl {
                    network,
                    project,
                    name,
                } => {
                    app.delete_network_acl(&client, network, project, name)
                        .await;
                }
                AppCommand::ReadNetworkAclLog {
                    network,
                    project,
                    name,
                } => {
                    app.read_network_acl_log(&client, network, project, name)
                        .await
                }
                AppCommand::RenameNetworkAcl {
                    network,
                    project,
                    name,
                    new_name,
                } => {
                    app.rename_network_acl(&client, network, project, name, new_name)
                        .await
                }
                AppCommand::CreateNetworkLoadBalancer(spec) => {
                    app.create_network_load_balancer(&client, spec).await
                }
                AppCommand::UpdateNetworkLoadBalancer(spec) => {
                    app.update_network_load_balancer(&client, spec).await
                }
                AppCommand::DeleteNetworkLoadBalancer {
                    network,
                    project,
                    listen_address,
                } => {
                    app.delete_network_load_balancer(&client, network, project, listen_address)
                        .await
                }
                AppCommand::ReadNetworkLoadBalancerState {
                    network,
                    project,
                    address,
                } => {
                    app.read_network_load_balancer_state(&client, network, project, address)
                        .await
                }
                AppCommand::ReadNetworkState { network, project } => {
                    app.load_network_state(&client, network, project).await
                }
                AppCommand::CreateNetworkPeer(spec) => app.create_network_peer(&client, spec).await,
                AppCommand::UpdateNetworkPeer(spec) => app.update_network_peer(&client, spec).await,
                AppCommand::DeleteNetworkPeer {
                    network,
                    project,
                    name,
                } => {
                    app.delete_network_peer(&client, network, project, name)
                        .await
                }
                AppCommand::ReadNetworkZones => app.load_network_zones(&client).await,
                AppCommand::ReadBgpConfiguration { network, project } => {
                    app.load_bgp_configuration(&client, network, project).await
                }
                AppCommand::ReadNetworkAddressSets => app.load_network_address_sets(&client).await,
                AppCommand::ReadNetworkIntegrations => app.load_network_integrations(&client).await,
                command @ (AppCommand::CreateNetworkIntegration { .. }
                | AppCommand::UpdateNetworkIntegration { .. }
                | AppCommand::RenameNetworkIntegration { .. }
                | AppCommand::DeleteNetworkIntegration { .. }) => {
                    app.mutate_network_integration(&client, command).await
                }
                command @ (AppCommand::CreateNetworkAddressSet { .. }
                | AppCommand::UpdateNetworkAddressSet { .. }
                | AppCommand::RenameNetworkAddressSet { .. }
                | AppCommand::DeleteNetworkAddressSet { .. }) => {
                    app.mutate_network_address_set(&client, command).await
                }
                command @ (AppCommand::CreateNetworkZone { .. }
                | AppCommand::UpdateNetworkZone { .. }
                | AppCommand::RenameNetworkZone { .. }
                | AppCommand::DeleteNetworkZone { .. }) => {
                    app.mutate_network_zone(&client, command).await
                }
                AppCommand::ReadNetworkZoneRecords { project, zone } => {
                    app.load_network_zone_records(&client, project, zone).await
                }
                command @ (AppCommand::CreateNetworkZoneRecord { .. }
                | AppCommand::UpdateNetworkZoneRecord { .. }
                | AppCommand::DeleteNetworkZoneRecord { .. }) => {
                    app.mutate_network_zone_record(&client, command).await
                }
                AppCommand::ReadFile {
                    name,
                    project,
                    path,
                } => app.read_file(&client, name, project, path).await,
                AppCommand::ListDirectory {
                    name,
                    project,
                    path,
                } => app.list_directory(&client, name, project, path).await,
                AppCommand::WriteFile {
                    name,
                    project,
                    path,
                    contents,
                } => app.write_file(&client, name, project, path, contents).await,
                AppCommand::DeleteFile {
                    name,
                    project,
                    path,
                } => app.delete_file(&client, name, project, path).await,
                AppCommand::PullFile {
                    name,
                    project,
                    path,
                    local_path,
                } => {
                    app.pull_file(&client, name, project, path, local_path)
                        .await
                }
                AppCommand::PushFile {
                    name,
                    project,
                    path,
                    local_path,
                } => {
                    app.push_file(&client, name, project, path, local_path)
                        .await
                }
                AppCommand::CancelOperation { id } => app.cancel_operation(&client, id).await,
            }
        }
    }

    Ok(())
}

async fn read_fleet_health(remotes: Vec<crate::remotes::SavedRemote>) -> Vec<FleetRow> {
    use futures_util::future::join_all;
    let probes = remotes.into_iter().map(|remote| async move {
        let endpoint = remote.transport.label();
        let client = match IncusClient::from_transport(remote.transport) {
            Ok(client) => client,
            Err(error) => {
                return (
                    FleetRow {
                        name: remote.name,
                        endpoint,
                        version: "-".into(),
                        instances: 0,
                        running: 0,
                        warnings: 0,
                        drift_keys: 0,
                        error: Some(error.to_string()),
                    },
                    None,
                );
            }
        };
        let (server, instances, warnings, config) = tokio::join!(
            client.server_info(),
            client.instances(),
            client.warnings(),
            client.resource_definition(ResourceKind::Server, "", "")
        );
        match (server, instances, warnings, config) {
            (Ok(server), Ok(instances), Ok(warnings), Ok(config)) => {
                let running = instances
                    .iter()
                    .filter(|instance| instance.status == "Running")
                    .count();
                (
                    FleetRow {
                        name: remote.name,
                        endpoint,
                        version: server.environment.server_version,
                        instances: instances.len(),
                        running,
                        warnings: warnings.len(),
                        drift_keys: 0,
                        error: None,
                    },
                    config.value.as_object().cloned(),
                )
            }
            (server, instances, warnings, config) => {
                let error = server
                    .err()
                    .or_else(|| instances.err())
                    .or_else(|| warnings.err())
                    .or_else(|| config.err())
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "fleet probe failed".into());
                (
                    FleetRow {
                        name: remote.name,
                        endpoint,
                        version: "-".into(),
                        instances: 0,
                        running: 0,
                        warnings: 0,
                        drift_keys: 0,
                        error: Some(error),
                    },
                    None,
                )
            }
        }
    });
    let mut results = join_all(probes).await;
    let baseline = results.iter().find_map(|(_, config)| config.clone());
    if let Some(baseline) = baseline {
        for (row, config) in &mut results {
            if let Some(config) = config {
                let keys = baseline
                    .keys()
                    .chain(config.keys())
                    .collect::<std::collections::BTreeSet<_>>();
                row.drift_keys = keys
                    .into_iter()
                    .filter(|key| baseline.get(*key) != config.get(*key))
                    .count();
            }
        }
    }
    results.into_iter().map(|(row, _)| row).collect()
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }

    fn suspend(&mut self) -> io::Result<()> {
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        self.terminal.show_cursor()
    }

    fn resume(&mut self) -> io::Result<()> {
        execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        )?;
        self.terminal.clear()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}
