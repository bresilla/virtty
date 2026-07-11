use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Widget, Wrap},
};
use tui_tabs::TabNav;

use crate::tui::{
    app::{App, Page},
    theme,
};

/// Nastty-style modal clear: draw a hard COLOR0 drop shadow first, then clear
/// the popup card itself. Keeping the name `Clear` upgrades every existing
/// popup consistently without duplicating shadow code across dozens of forms.
#[derive(Clone, Copy)]
struct Clear;

impl Widget for Clear {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let bounds = buffer.area;
        let shadow = Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(1),
            area.width
                .min(bounds.right().saturating_sub(area.x.saturating_add(2))),
            area.height
                .min(bounds.bottom().saturating_sub(area.y.saturating_add(1))),
        );
        ratatui::widgets::Block::default()
            .style(Style::default().bg(theme::COLOR0))
            .render(shadow, buffer);
        ratatui::widgets::Clear.render(area, buffer);
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    // nastty deliberately uses the user's terminal background rather than
    // painting a separate application canvas. Keep only the shared text
    // colour here so every unstyled value follows its palette.
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme::text()),
        frame.area(),
    );
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, layout[0], app);
    let body = layout[1];
    if body.width >= 90 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(body);
        render_sidebar(frame, columns[0], app);
        render_page(frame, columns[2], app);
    } else {
        render_page(frame, body, app);
    }
    render_footer(frame, layout[2], app);
    if app.overlay_visible() {
        apply_modal_backdrop(frame);
    }
    render_overlays(frame, app);
}

fn render_page(frame: &mut Frame, area: Rect, app: &App) {
    match app.page() {
        Page::Dashboard => render_dashboard(frame, area, app),
        Page::Instances => render_instances(frame, area, app),
        Page::Operations => render_operations(frame, area, app),
        Page::Events => render_events(frame, area, app),
        Page::Storage => render_storage(frame, area, app),
        Page::Networks => render_networks(frame, area, app),
        Page::Images => render_images(frame, area, app),
        Page::Projects => render_projects(frame, area, app),
        Page::Settings => render_settings(frame, area, app),
    }
}

fn render_overlays(frame: &mut Frame, app: &App) {
    if let Some(error) = &app.model.last_error {
        render_error(frame, frame.area(), error);
    }
    if let Some(confirmation) = app.confirmation() {
        render_confirmation(frame, frame.area(), confirmation);
    }
    if let Some(form) = app.create_form() {
        render_create_form(frame, frame.area(), form);
    }
    if let Some(form) = app.exec_form() {
        render_exec_form(frame, frame.area(), form);
    }
    if let Some(form) = app.snapshot_form() {
        render_snapshot_form(frame, frame.area(), form);
    }
    if let Some(form) = app.backup_form() {
        render_backup_form(frame, frame.area(), form);
    }
    if let Some(form) = app.console_form() {
        render_console_form(frame, frame.area(), form);
    }
    if let Some(form) = app.detail_form() {
        render_detail_form(frame, frame.area(), form);
    }
    if let Some(form) = app.config_form() {
        render_config_form(frame, frame.area(), form);
    }
    if let Some(viewer) = app.journal_viewer() {
        render_journal_viewer(frame, frame.area(), app, viewer);
    }
    if let Some(manager) = app.fleet_manager() {
        render_fleet_manager(frame, frame.area(), manager);
    }
    if let Some(inspector) = app.placement_inspector() {
        render_placement_inspector(frame, frame.area(), inspector);
    }
    if let Some(form) = app.backup_schedule_form() {
        render_backup_schedule(frame, frame.area(), form);
    }
    if let Some(manager) = app.openfga_manager() {
        render_openfga_manager(frame, frame.area(), manager);
    }
    if let Some(preview) = app.image_retention_preview() {
        render_image_retention(frame, frame.area(), preview);
    }
    if let Some(form) = app.resource_create_form() {
        render_resource_create_form(frame, frame.area(), form);
    }
    if let Some(confirmation) = app.resource_delete_confirmation() {
        render_resource_delete_confirmation(frame, frame.area(), confirmation);
    }
    if let Some(form) = app.image_alias_form() {
        render_image_alias_form(frame, frame.area(), form);
    }
    if let Some(manager) = app.image_alias_manager() {
        render_image_alias_manager(frame, frame.area(), manager);
    }
    if let Some(form) = app.image_form() {
        render_image_form(frame, frame.area(), form);
    }
    if let Some(form) = app.artifacts_form() {
        render_artifacts_form(frame, frame.area(), form);
    }
    if let Some(form) = app.operation_detail() {
        render_operation_detail(frame, frame.area(), form);
    }
    if let Some(form) = app.rename_form() {
        render_rename_form(frame, frame.area(), form);
    }
    if let Some(form) = app.rebuild_form() {
        render_rebuild_form(frame, frame.area(), form);
    }
    if let Some(manager) = app.log_manager() {
        render_log_manager(frame, frame.area(), manager);
    }
    if let Some(explorer) = app.allocation_explorer() {
        render_allocation_explorer(frame, frame.area(), app, explorer);
    }
    if let Some(explorer) = app.metrics_explorer() {
        render_metrics_explorer(frame, frame.area(), app, explorer);
    }
    if let Some(manager) = app.maintenance_manager() {
        render_maintenance_manager(frame, frame.area(), manager);
    }
    if let Some(detail) = app.storage_resource_detail() {
        let popup = centered_rect(82, 72, frame.area());
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .block(theme::panel("storage pool resources · esc close")),
            popup,
        );
    }
    if let Some(manager) = app.settings_manager() {
        render_settings_manager(frame, frame.area(), app, manager);
    }
    if let Some(manager) = app.logging_target_manager() {
        render_logging_target_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.scriptlet_manager() {
        render_scriptlet_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.preseed_manager() {
        render_preseed_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.pool_safety_manager() {
        render_pool_safety_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.qmp_manager() {
        render_qmp_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.bulk_manager() {
        render_bulk_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.device_manager() {
        render_device_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.volume_manager() {
        render_volume_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.bucket_manager() {
        render_bucket_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.network_services() {
        render_network_services(frame, frame.area(), manager);
    }
    if let Some(manager) = app.zone_manager() {
        render_zone_manager(frame, frame.area(), manager);
    }
    if let Some(form) = app.bgp_form() {
        render_bgp_form(frame, frame.area(), form);
    }
    if let Some(manager) = app.address_set_manager() {
        render_address_set_manager(frame, frame.area(), manager);
    }
    if let Some(manager) = app.integration_manager() {
        render_integration_manager(frame, frame.area(), manager);
    }
    if app.help_visible() {
        render_help(frame, frame.area(), app);
    }
    if let Some(form) = app.file_form() {
        render_file_form(frame, frame.area(), form);
    }
    if let Some(confirmation) = app.operation_confirmation() {
        render_operation_confirmation(frame, frame.area(), confirmation);
    }
    if let Some(input) = app.filter_form() {
        render_filter(frame, frame.area(), input);
    }
    if let Some(menu) = app.action_menu() {
        render_action_menu(frame, frame.area(), menu);
    }
    if let Some(manager) = app.view_manager() {
        render_saved_views(frame, frame.area(), manager);
    }
    if let Some(form) = app.audit_export() {
        render_audit_export(frame, frame.area(), app, form);
    }
    if let Some(form) = app.connection_form() {
        render_connection_form(frame, frame.area(), form);
    }
    if let Some(manager) = app.cluster_group_manager() {
        render_cluster_group_manager(frame, frame.area(), manager);
    }
    if let Some(form) = app.move_form() {
        render_move_form(frame, frame.area(), form);
    }
    if let Some(form) = app.project_usage() {
        render_project_usage(frame, frame.area(), form);
    }
    if let Some(form) = app.project_access() {
        render_project_access(frame, frame.area(), form);
    }
    if let Some(form) = app.instance_access() {
        render_instance_access(frame, frame.area(), form);
    }
}

fn render_instance_access(frame: &mut Frame, area: Rect, form: &super::app::InstanceAccessForm) {
    let popup = centered_rect(76, 64, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Instance  ", theme::label()),
            Span::styled(form.name(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(form.project(), theme::text()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "identifier                              ",
                theme::table_header(),
            ),
            Span::styled("provider      ", theme::table_header()),
            Span::styled("role", theme::table_header()),
        ]),
    ];
    if form.entries().is_empty() {
        lines.push(Line::styled("No explicit access entries", theme::muted()));
    } else {
        lines.extend(form.entries().iter().map(|entry| {
            Line::from(vec![
                Span::styled(format!("{:<40}", entry.identifier), theme::text()),
                Span::styled(format!("{:<14}", entry.provider), theme::subtle()),
                Span::styled(&entry.role, theme::label()),
            ])
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "These are Incus access identities, not Linux users inside the guest.",
        theme::subtle(),
    ));
    lines.push(Line::styled("r refresh · Enter/Esc close", theme::muted()));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("instance access")),
        popup,
    );
}

fn render_project_access(frame: &mut Frame, area: Rect, form: &super::app::ProjectAccessForm) {
    let popup = centered_rect(76, 64, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Project  ", theme::label()),
            Span::styled(form.project(), theme::title()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "identifier                              ",
                theme::table_header(),
            ),
            Span::styled("provider      ", theme::table_header()),
            Span::styled("role", theme::table_header()),
        ]),
    ];
    if form.entries().is_empty() {
        lines.push(Line::styled("No explicit access entries", theme::muted()));
    } else {
        lines.extend(form.entries().iter().map(|entry| {
            Line::from(vec![
                Span::styled(format!("{:<40}", entry.identifier), theme::text()),
                Span::styled(format!("{:<14}", entry.provider), theme::subtle()),
                Span::styled(&entry.role, theme::label()),
            ])
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "These are Incus access identities, not Linux users inside guest instances.",
        theme::subtle(),
    ));
    lines.push(Line::styled(
        "TLS restrictions are edited in Settings · OpenFGA relationships are managed by OpenFGA",
        theme::muted(),
    ));
    lines.push(Line::styled("r refresh · Enter/Esc close", theme::subtle()));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("project access")),
        popup,
    );
}

fn render_project_usage(frame: &mut Frame, area: Rect, form: &super::app::ProjectUsageForm) {
    let popup = centered_rect(74, 68, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Project  ", theme::label()),
            Span::styled(form.project(), theme::title()),
        ]),
        Line::raw(""),
    ];
    if let Some(state) = form.state() {
        if let Some(resources) = state
            .get("resources")
            .and_then(serde_json::Value::as_object)
        {
            lines.push(Line::styled("Allocated resources", theme::heading()));
            for (name, value) in resources.iter().take(24) {
                let (usage, limit) = if let Some(object) = value.as_object() {
                    (
                        object
                            .get("usage")
                            .map(format_json_scalar)
                            .unwrap_or_else(|| "-".into()),
                        object
                            .get("limit")
                            .map(format_json_scalar)
                            .unwrap_or_else(|| "unlimited".into()),
                    )
                } else {
                    (format_json_scalar(value), "-".into())
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{name:<28}"), theme::label()),
                    Span::styled(format!("{usage:<16}"), theme::text()),
                    Span::styled(format!("limit {limit}"), theme::muted()),
                ]));
            }
        } else if let Some(object) = state.as_object() {
            for (name, value) in object.iter().take(24) {
                lines.push(Line::from(vec![
                    Span::styled(format!("{name:<28}"), theme::label()),
                    Span::styled(format_json_scalar(value), theme::text()),
                ]));
            }
        }
    } else {
        lines.push(Line::styled(
            "Loading project allocation state…",
            theme::muted(),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("r refresh · Enter/Esc close", theme::subtle()));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("project resource usage")),
        popup,
    );
}

fn format_json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => "-".to_owned(),
        serde_json::Value::Array(value) => format!("{} items", value.len()),
        serde_json::Value::Object(value) => format!("{} fields", value.len()),
    }
}

fn render_move_form(frame: &mut Frame, area: Rect, form: &super::app::MoveForm) {
    let popup = centered_rect(72, 54, area);
    frame.render_widget(Clear, popup);
    let (name, project) = form.source();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Source  ", theme::label()),
            Span::styled(name, theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(project, theme::text()),
        ]),
        Line::raw(""),
    ];
    lines.extend(
        form.fields()
            .into_iter()
            .map(|(label, value, toggle, focused)| {
                let shown = if toggle {
                    if value == "true" { "● on" } else { "○ off" }
                } else {
                    value
                };
                form_line(label, shown, focused)
            }),
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Blank member/pool keeps current placement · Space toggles live · Enter migrates",
        theme::subtle(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("move or migrate instance")),
        popup,
    );
}

fn render_cluster_group_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::ClusterGroupManager,
) {
    let popup = centered_rect(82, 70, area);
    frame.render_widget(Clear, popup);
    let rows = manager.groups().iter().enumerate().map(|(index, group)| {
        Row::new([
            group.name.clone(),
            group.description.clone(),
            group.members.join(", "),
        ])
        .style(if index == manager.selected() {
            theme::selected_row()
        } else {
            theme::text()
        })
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Min(24),
                Constraint::Percentage(40),
            ],
        )
        .header(Row::new(["Name", "Description", "Members"]).style(theme::heading()))
        .block(theme::panel(
            "cluster groups · n create · e edit · R rename · d delete · Esc close",
        )),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let editor = centered_rect(68, 42, area);
        frame.render_widget(Clear, editor);
        let mut lines = vec![
            Line::styled(
                if manager.editing() {
                    "Edit cluster group"
                } else {
                    "Create cluster group"
                },
                theme::title(),
            ),
            Line::raw(""),
        ];
        lines.extend(
            fields
                .into_iter()
                .map(|(label, value, focused)| form_line(label, value, focused)),
        );
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Members are comma-separated · Enter save · Esc cancel",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("cluster group form")),
            editor,
        );
    }
    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(66, 26, area);
        frame.render_widget(Clear, confirm);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete cluster group", theme::title()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type  ", theme::label()),
                    Span::styled(manager.delete_phrase(), theme::text()),
                ]),
                Line::from(vec![
                    Span::styled("> ", theme::label()),
                    Span::styled(input, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
                ]),
            ])
            .block(theme::panel("confirm deletion")),
            confirm,
        );
    }
    if let Some(input) = manager.rename_input() {
        render_rename_resource(frame, area, "Rename cluster group", input);
    }
}

fn render_connection_form(frame: &mut Frame, area: Rect, form: &super::app::ConnectionForm) {
    let popup = centered_rect(76, 58, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Mode  ", theme::label()),
            Span::styled(
                if form.remote() {
                    "remote TLS"
                } else {
                    "local Unix socket"
                },
                theme::title(),
            ),
        ]),
        Line::styled(
            "↑↓ saved target · m/Space mode · Tab fields · Ctrl-S save · Ctrl-D delete · Enter connect",
            theme::subtle(),
        ),
        Line::raw(""),
    ];
    if !form.saved().is_empty() {
        lines.push(Line::styled("Saved targets", theme::heading()));
        lines.extend(form.saved().iter().enumerate().map(|(index, saved)| {
            Line::styled(
                format!(
                    "{} {:<18} {}",
                    if index == form.selected_saved() {
                        "▌"
                    } else {
                        " "
                    },
                    saved.name,
                    saved.transport.label()
                ),
                if index == form.selected_saved() {
                    theme::selected_row()
                } else {
                    theme::subtle()
                },
            )
        }));
        lines.push(Line::raw(""));
    }
    lines.extend(
        form.fields()
            .into_iter()
            .map(|(label, value, focused)| form_line(label, value, focused)),
    );
    if !form.remote() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "TLS identity fields apply only to remote mode.",
            theme::muted(),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("connect to Incus target")),
        popup,
    );
}

fn render_action_menu(frame: &mut Frame, area: Rect, menu: &super::app::ActionMenu) {
    let popup = centered_rect(48, 70, area);
    frame.render_widget(Clear, popup);
    let lines = menu
        .entries()
        .map(|(label, selected)| {
            Line::from(vec![
                Span::styled(if selected { "▌ " } else { "  " }, theme::label()),
                Span::styled(
                    label,
                    if selected {
                        theme::selected_row()
                    } else {
                        theme::text()
                    },
                ),
            ])
        })
        .chain(std::iter::once(Line::raw("")))
        .chain(std::iter::once(Line::styled(
            "j/k select  Enter run  x/Esc close",
            theme::subtle(),
        )))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel(menu.title())),
        popup,
    );
}

fn render_saved_views(frame: &mut Frame, area: Rect, manager: &super::app::SavedViewManager) {
    let popup = centered_rect(70, 62, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Saved views and bookmarks", theme::title()),
        Line::styled("A view restores both its page and filter.", theme::subtle()),
        Line::raw(""),
    ];
    if manager.views().is_empty() {
        lines.push(Line::styled("No saved views", theme::dim()));
    } else {
        lines.extend(manager.views().iter().enumerate().map(|(index, view)| {
            Line::styled(
                format!(
                    "{} {:<20} {:<12} /{}",
                    if index == manager.selected() {
                        "▌"
                    } else {
                        " "
                    },
                    view.name,
                    view.page,
                    view.filter
                ),
                if index == manager.selected() {
                    theme::selected_row()
                } else {
                    theme::text()
                },
            )
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("enter", "apply"),
            theme::chip("n", "save current"),
            theme::chip("d", "delete"),
            theme::chip("'", "close"),
        ]
        .concat(),
    ));
    if let Some(input) = manager.name_input() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Name current page + filter", theme::heading()));
        lines.push(Line::from(vec![
            Span::styled("> ", theme::label()),
            Span::styled(input, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
        ]));
        lines.push(Line::styled(
            "Enter saves or replaces a view with this name.",
            theme::dim(),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("operator bookmarks")),
        popup,
    );
}

fn render_audit_export(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    form: &super::app::AuditExportForm,
) {
    let popup = centered_rect(72, 52, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Audit export", theme::title()),
        Line::styled(
            "Structured export of the currently retained event and operation history.",
            theme::subtle(),
        ),
        Line::raw(""),
    ];
    for (label, value, choice, focused) in form.fields() {
        lines.push(Line::from(vec![
            Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
            Span::styled(format!("{label:<18}"), theme::label()),
            Span::styled(
                value,
                if focused {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
            Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
        ]));
    }
    let (events, operations) = form.preview_counts(app);
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("Preview · {events} events · {operations} operations"),
        Style::default().fg(theme::BLUE),
    ));
    lines.push(Line::styled(
        "Redaction recursively masks password, secret, token, private-key, and certificate fields.",
        theme::muted(),
    ));
    lines.push(Line::styled(
        "Tab fields · Space choices · final Enter writes · Esc cancels",
        theme::dim(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("filtered redacted audit export")),
        popup,
    );
}

fn render_filter(frame: &mut Frame, area: Rect, input: &str) {
    let popup = centered_rect(62, 18, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("/ ", theme::label()),
                Span::styled(
                    if input.is_empty() { " " } else { input },
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT),
                ),
            ]),
            Line::raw(""),
            Line::styled(
                "Enter apply  •  Esc cancel  •  L clears active filter",
                theme::subtle(),
            ),
        ])
        .block(theme::panel("filter current page")),
        popup,
    );
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    const PAGE_ICONS: [&str; 9] = ["⌂", "⛁", "◷", "⇄", "▤", "⌁", "▦", "☰", "⚙"];
    let group = app.page().group();
    let mut lines = Page::group_pages(group)
        .iter()
        .map(|page| {
            let active = *page == app.page();
            let icon = PAGE_ICONS[page.index()];
            Line::from(vec![
                Span::styled(
                    if active { " ▌ " } else { "   " },
                    Style::default().fg(theme::ACCENT),
                ),
                Span::styled(
                    format!("{icon} {}", page.title()),
                    if active {
                        Style::default().fg(theme::ACCENT)
                    } else {
                        theme::subtle()
                    },
                ),
            ])
        })
        .collect::<Vec<_>>();
    lines.extend([
        Line::raw(""),
        Line::styled(" Tab / ⇧Tab  views", theme::dim()),
        Line::styled(" ← / →  sections", theme::dim()),
    ]);
    frame.render_widget(
        Paragraph::new(lines).block(
            theme::panel(Page::GROUPS[group]).border_style(Style::default().fg(theme::SURFACE)),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let connected = app.model.server.is_some() && app.model.last_error.is_none();
    let count = match app.page() {
        Page::Dashboard | Page::Instances => app.model.instances.len(),
        Page::Operations => app.model.operations.len(),
        Page::Events => app.model.events.len(),
        Page::Storage => app.model.storage_pools.len(),
        Page::Networks => app.model.networks.len(),
        Page::Images => app.model.images.len(),
        Page::Projects => app.model.projects.len(),
        Page::Settings => app.model.cluster_members.len() + app.model.certificates.len(),
    };
    let selected = match app.page() {
        Page::Dashboard | Page::Instances => app.selected_instance(),
        Page::Operations => app.selected_operation(),
        Page::Storage => app.selected_storage_pool(),
        Page::Networks => app.selected_network(),
        Page::Images => app.selected_image(),
        Page::Projects => app.selected_project(),
        Page::Events | Page::Settings => 0,
    };
    let summary = if count == 0 {
        "0 items · no selection".to_owned()
    } else {
        format!("{count} items · row {}", (selected + 1).min(count))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "● ",
                Style::default().fg(if connected { theme::GREEN } else { theme::RED }),
            ),
            Span::styled(
                if connected {
                    "connected"
                } else {
                    "disconnected"
                },
                theme::subtle(),
            ),
            Span::styled(format!(" · {}", app.target()), theme::dim()),
        ])),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(
                    "{} / {}  ",
                    Page::GROUPS[app.page().group()],
                    app.page().title()
                ),
                theme::title(),
            ),
            Span::styled(
                if area.width >= 82 {
                    summary
                } else {
                    String::new()
                },
                theme::dim(),
            ),
        ]))
        .alignment(Alignment::Right),
        chunks[1],
    );
}

fn apply_modal_backdrop(frame: &mut Frame) {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            cell.fg = theme::MUTED;
            cell.bg = theme::SURFACE_LO;
        }
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let labels = if area.width >= 82 {
        Some(&Page::GROUPS)
    } else if area.width >= 45 {
        Some(&Page::GROUPS_COMPACT)
    } else {
        None
    };
    if let Some(labels) = labels {
        frame.render_widget(
            TabNav::new(labels, app.page().group())
                .style(theme::subtle())
                .highlight_style(Style::default().fg(theme::ACCENT))
                .highlight_bold(false)
                .border_style(Style::default().fg(theme::SURFACE))
                .indicator(Some("●")),
            area,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " virtty ",
                Style::default()
                    .fg(theme::SELECTED_TAB_TEXT)
                    .bg(theme::ACCENT),
            ),
            Span::styled(
                format!(
                    " {} / {} ",
                    Page::GROUPS[app.page().group()],
                    app.page().title()
                ),
                theme::title(),
            ),
        ]))
        .block(theme::panel_bare()),
        area,
    );
}

fn render_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    // This intentionally follows nastty's overview arrangement: one
    // information card on the left, live state on the right, then four
    // balanced statistic tiles below it.
    let [server_area, right] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
    let [overview_area, tiles_area] =
        Layout::vertical([Constraint::Length(8), Constraint::Min(8)]).areas(right);

    let server = app.model.server.as_ref();
    let server_name = server
        .map(|server| server.environment.server_name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Not connected");
    let version = server
        .map(|server| server.environment.server_version.as_str())
        .filter(|version| !version.is_empty())
        .unwrap_or("-");
    let api = server
        .map(|server| server.api_version.as_str())
        .filter(|version| !version.is_empty())
        .unwrap_or("-");
    let auth = server
        .map(|server| server.auth.as_str())
        .filter(|auth| !auth.is_empty())
        .unwrap_or("-");
    let resources = app.model.host_resources.as_ref();
    let cpu_threads = resources
        .and_then(|value| value.pointer("/cpu/total"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let memory_total = resources
        .and_then(|value| value.pointer("/memory/total"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let memory_used = resources
        .and_then(|value| value.pointer("/memory/used"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let disks = resources
        .and_then(|value| value.pointer("/storage/disks"))
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let network_cards = resources
        .and_then(|value| value.pointer("/network/cards"))
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let metric_series = app
        .model
        .metrics
        .as_deref()
        .map(|metrics| {
            metrics
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .count()
        })
        .unwrap_or(0);
    let connection = Paragraph::new(vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("Target  ", theme::label()),
            Span::styled(app.target(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Server  ", theme::label()),
            Span::styled(server_name, theme::title()),
        ]),
        Line::from(vec![
            Span::styled("Version ", theme::label()),
            Span::styled(version, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("API     ", theme::label()),
            Span::styled(api, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Auth    ", theme::label()),
            Span::styled(auth, theme::text()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("CPU     ", theme::label()),
            Span::styled(
                if cpu_threads > 0 {
                    format!("{cpu_threads} thread(s)")
                } else {
                    "unavailable".to_owned()
                },
                theme::text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Memory  ", theme::label()),
            Span::styled(
                if memory_total > 0 {
                    format!(
                        "{} / {}",
                        format_bytes(memory_used as i64),
                        format_bytes(memory_total as i64)
                    )
                } else {
                    "unavailable".to_owned()
                },
                theme::text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Devices ", theme::label()),
            Span::styled(
                format!("{disks} disk(s) · {network_cards} NIC(s)"),
                theme::text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Metrics ", theme::label()),
            Span::styled(format!("{metric_series} live series"), theme::text()),
        ]),
    ])
    .block(theme::panel("server"));
    frame.render_widget(connection, server_area);

    let counts = app.model.instance_counts();
    let overview = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("running  ", theme::subtle()),
            Span::styled(
                counts.running.to_string(),
                Style::default().fg(theme::SUCCESS),
            ),
            Span::raw("    "),
            Span::styled("stopped  ", theme::subtle()),
            Span::styled(counts.stopped.to_string(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("frozen   ", theme::subtle()),
            Span::styled(
                counts.frozen.to_string(),
                Style::default().fg(theme::WARNING),
            ),
            Span::raw("    "),
            Span::styled("other    ", theme::subtle()),
            Span::styled(counts.other.to_string(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("containers  ", theme::subtle()),
            Span::styled(counts.containers.to_string(), theme::text()),
            Span::raw("    "),
            Span::styled("VMs  ", theme::subtle()),
            Span::styled(
                counts.virtual_machines.to_string(),
                Style::default().fg(theme::BLUE),
            ),
        ]),
        Line::from(vec![
            Span::styled("projects     ", theme::subtle()),
            Span::styled(
                app.model.projects.len().to_string(),
                Style::default().fg(theme::PEACH),
            ),
        ]),
    ])
    .block(theme::panel("instance state"));
    frame.render_widget(overview, overview_area);

    let [top, bottom] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
        .areas(tiles_area);
    let [instances, containers] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);
    let [vms, projects] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(bottom);
    stat_tile(
        frame,
        instances,
        "instances",
        &app.model.instances.len().to_string(),
        theme::MAUVE,
    );
    stat_tile(
        frame,
        containers,
        "containers",
        &counts.containers.to_string(),
        theme::GREEN,
    );
    stat_tile(
        frame,
        vms,
        "virtual machines",
        &counts.virtual_machines.to_string(),
        theme::BLUE,
    );
    stat_tile(
        frame,
        projects,
        "projects",
        &app.model.projects.len().to_string(),
        theme::PEACH,
    );
}

fn stat_tile(
    frame: &mut Frame,
    area: Rect,
    caption: &str,
    value: &str,
    color: ratatui::style::Color,
) {
    let block = theme::panel_bare();
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [_, value_area, caption_area, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        value_area,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(caption.to_string(), theme::dim()))
            .alignment(Alignment::Center),
        caption_area,
    );
}

fn render_instances(frame: &mut Frame, area: Rect, app: &App) {
    let [create_area, table_area] = Layout::vertical([Constraint::Length(4), Constraint::Min(0)])
        .spacing(1)
        .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "  ＋  CREATE VM / CONTAINER  ",
                Style::default().fg(theme::COLOR0).bg(theme::ACCENT),
            ),
            Span::styled(
                "   guided setup: image · compute · storage · network · security · review",
                theme::subtle(),
            ),
        ]))
        .block(
            theme::panel("Create workload · press c or n")
                .border_style(Style::default().fg(theme::ACCENT)),
        ),
        create_area,
    );
    let header = Row::new(["Name", "Type", "Status", "Project", "Location", "Profiles"])
        .style(theme::heading())
        .bottom_margin(1);
    let rows = app
        .model
        .instances
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            app.matches_filter([
                v.name.as_str(),
                v.status.as_str(),
                v.project.as_str(),
                v.location.as_str(),
                v.instance_type.as_str(),
            ])
        })
        .map(|(index, instance)| {
            let style = if index == app.selected_instance() {
                theme::selected_row()
            } else {
                theme::text()
            };
            Row::new(vec![
                Cell::from(instance.name.clone()),
                Cell::from(instance.type_label()),
                Cell::from(instance.status.clone()).style(theme::status(&instance.status)),
                Cell::from(instance.project.clone()),
                Cell::from(instance.location.clone()),
                Cell::from(instance.profiles.join(", ")),
            ])
            .style(style)
        });
    let table = Table::new(
        rows,
        [
            Constraint::Min(18),
            Constraint::Length(6),
            Constraint::Length(11),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Instances ({}) ",
        app.model.instances.len()
    )));
    frame.render_widget(table, table_area);
}

fn render_operations(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["Status", "Description", "Location", "Created", "ID"])
        .style(theme::heading())
        .bottom_margin(1);
    let rows = app
        .model
        .operations
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            app.matches_filter([
                v.id.as_str(),
                v.description.as_str(),
                v.status.as_str(),
                v.location.as_str(),
            ])
        })
        .take(100)
        .map(|(index, operation)| {
            let description = if operation.description.is_empty() {
                operation.class.clone()
            } else {
                operation.description.clone()
            };
            let style = if index == app.selected_operation() {
                theme::selected_row()
            } else {
                theme::text()
            };
            Row::new(vec![
                Cell::from(operation.status.clone()).style(operation_style(&operation.status)),
                Cell::from(description),
                Cell::from(operation.location.clone()),
                Cell::from(operation.created_at.clone()),
                Cell::from(operation.id.clone()),
            ])
            .style(style)
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(36),
            Constraint::Length(16),
            Constraint::Length(24),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Operations ({}) ",
        app.model.operations.len()
    )));
    frame.render_widget(table, area);
}

fn render_events(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["Time", "Type", "Project", "Location", "Event"])
        .style(theme::heading())
        .bottom_margin(1);
    let rows = app
        .model
        .events
        .iter()
        .filter(|event| {
            app.matches_filter([
                event.event_type.as_str(),
                event.project.as_str(),
                event.location.as_str(),
                event.summary().as_str(),
            ])
        })
        .map(|event| {
            Row::new(vec![
                Cell::from(event.timestamp.clone()),
                Cell::from(event.event_type.clone()).style(event_style(&event.event_type)),
                Cell::from(event.project.clone()),
                Cell::from(event.location.clone()),
                Cell::from(event.summary()),
            ])
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Length(16),
            Constraint::Min(24),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Events ({}) ",
        app.model.events.len()
    )));
    frame.render_widget(table, area);
}

fn render_storage(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["Name", "Driver", "Status", "Locations", "Description"])
        .style(theme::heading())
        .bottom_margin(1);
    let rows = app
        .model
        .storage_pools
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            app.matches_filter([
                v.name.as_str(),
                v.driver.as_str(),
                v.status.as_str(),
                v.description.as_str(),
            ])
        })
        .map(|(index, pool)| {
            Row::new([
                pool.name.clone(),
                pool.driver.clone(),
                pool.status.clone(),
                pool.locations.join(", "),
                pool.description.clone(),
            ])
            .style(if index == app.selected_storage_pool() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(14),
            Constraint::Length(14),
            Constraint::Length(24),
            Constraint::Min(24),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Storage pools ({}) ",
        app.model.storage_pools.len()
    )));
    frame.render_widget(table, area);
}

fn render_networks(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(["Name", "Type", "Status", "Project", "Managed", "Location"])
        .style(theme::heading())
        .bottom_margin(1);
    let rows = app
        .model
        .networks
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            app.matches_filter([
                v.name.as_str(),
                v.network_type.as_str(),
                v.status.as_str(),
                v.project.as_str(),
                v.location.as_str(),
            ])
        })
        .map(|(index, network)| {
            Row::new([
                network.name.clone(),
                network.network_type.clone(),
                network.status.clone(),
                network.project.clone(),
                if network.managed { "yes" } else { "no" }.to_owned(),
                network.location.clone(),
            ])
            .style(if index == app.selected_network() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(10),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Networks ({}) · {} allocations ",
        app.model.networks.len(),
        app.model.network_allocations.len()
    )));
    frame.render_widget(table, area);
}

fn render_images(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new([
        "Alias / fingerprint",
        "Type",
        "Architecture",
        "Size",
        "Public",
    ])
    .style(theme::heading())
    .bottom_margin(1);
    let rows = app
        .model
        .images
        .iter()
        .enumerate()
        .filter(|(_, image)| {
            app.matches_filter([
                image.fingerprint.as_str(),
                image.architecture.as_str(),
                image.image_type.as_str(),
                image.project.as_str(),
            ]) || image
                .aliases
                .iter()
                .any(|alias| app.matches_filter([alias.name.as_str()]))
        })
        .map(|(index, image)| {
            Row::new([
                image.display_name(),
                image.image_type.clone(),
                image.architecture.clone(),
                format_bytes(image.size),
                if image.public { "yes" } else { "no" }.to_owned(),
            ])
            .style(if index == app.selected_image() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    let table = Table::new(
        rows,
        [
            Constraint::Min(28),
            Constraint::Length(14),
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .column_spacing(1)
    .block(theme::panel(format!(
        " Images ({}) ",
        app.model.images.len()
    )));
    frame.render_widget(table, area);
}

fn render_projects(frame: &mut Frame, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let project_rows = app
        .model
        .projects
        .iter()
        .enumerate()
        .filter(|(_, v)| app.matches_filter([v.name.as_str(), v.description.as_str()]))
        .map(|(index, project)| {
            Row::new([project.name.clone(), project.description.clone()]).style(
                if index == app.selected_project() {
                    theme::selected_row()
                } else {
                    theme::text()
                },
            )
        });
    let projects = Table::new(project_rows, [Constraint::Length(18), Constraint::Min(18)])
        .header(Row::new(["Name", "Description"]).style(theme::heading()))
        .column_spacing(1)
        .block(theme::panel(format!(
            " Projects ({}) · active: {} · Enter switches ",
            app.model.projects.len(),
            app.active_project()
        )));
    frame.render_widget(projects, columns[0]);

    let profile_rows = app
        .model
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, v)| {
            app.matches_filter([v.name.as_str(), v.project.as_str(), v.description.as_str()])
        })
        .map(|(index, profile)| {
            Row::new([
                profile.name.clone(),
                profile.project.clone(),
                profile.description.clone(),
            ])
            .style(if index == app.selected_profile() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    let profiles = Table::new(
        profile_rows,
        [
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Min(18),
        ],
    )
    .header(Row::new(["Name", "Project", "Description"]).style(theme::heading()))
    .column_spacing(1)
    .block(theme::panel(format!(
        " Profiles ({}) ",
        app.model.profiles.len()
    )));
    frame.render_widget(profiles, columns[1]);
}

fn render_settings(frame: &mut Frame, area: Rect, app: &App) {
    let [connection, server_config] =
        Layout::vertical([Constraint::Length(8), Constraint::Min(8)]).areas(area);
    let connected = app.model.server.is_some() && app.model.last_error.is_none();
    let server = app.model.server.as_ref();
    let lines = vec![
        Line::from(vec![
            Span::styled("Target  ", theme::label()),
            Span::styled(app.target(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Connection  ", theme::label()),
            Span::styled(
                if connected {
                    "● connected"
                } else {
                    "○ disconnected"
                },
                Style::default().fg(if connected { theme::GREEN } else { theme::RED }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Server  ", theme::label()),
            Span::styled(
                server
                    .map(|info| info.environment.server_name.as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("-"),
                theme::text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Version  ", theme::label()),
            Span::styled(
                server
                    .map(|info| info.environment.server_version.as_str())
                    .filter(|version| !version.is_empty())
                    .unwrap_or("-"),
                theme::text(),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("connection")),
        connection,
    );

    let config = Paragraph::new(vec![
        Line::styled("Server configuration", theme::title()),
        Line::raw(""),
        Line::styled(
            "E  Open structured server configuration fields.",
            theme::text(),
        ),
        Line::styled(
            format!(
                "s  Manage {} warning(s), {} certificate(s), and {} cluster member(s).",
                app.model.warnings.len(),
                app.model.certificates.len(),
                app.model.cluster_members.len()
            ),
            theme::text(),
        ),
        Line::styled(
            "Tab changes fields; the final Enter saves with Incus's ETag.",
            theme::dim(),
        ),
        Line::raw(""),
        Line::styled(
            "Server config is powerful: the current server revision is checked before any write.",
            theme::dim(),
        ),
    ])
    .block(theme::panel("server control"));
    frame.render_widget(config, server_config);
}

fn render_confirmation(frame: &mut Frame, area: Rect, confirmation: &super::app::Confirmation) {
    let popup = centered_rect(72, 28, area);
    frame.render_widget(Clear, popup);
    let expected = confirmation.expected_phrase();
    let message = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(
                "{} instance '{}'",
                confirmation.action().label(),
                confirmation.instance_name()
            ),
            Style::default()
                .fg(if confirmation.action().is_destructive() {
                    theme::DANGER
                } else {
                    theme::WARNING
                })
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from("This submits an asynchronous Incus operation."),
        Line::from(vec![
            Span::styled("Type ", theme::muted()),
            Span::styled(&expected, theme::heading()),
            Span::styled(" and press Enter:", theme::muted()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme::ACCENT)),
            Span::raw(confirmation.input()),
        ]),
        Line::raw(""),
        Line::styled("Esc cancels", theme::muted()),
    ])
    .wrap(Wrap { trim: true })
    .block(theme::panel(" Confirm lifecycle action "));
    frame.render_widget(message, popup);
}

fn render_create_form(frame: &mut Frame, area: Rect, form: &super::app::CreateForm) {
    let popup = centered_rect(88, 88, area);
    frame.render_widget(Clear, popup);
    let block =
        theme::panel(" CREATE VM / CONTAINER ").border_style(Style::default().fg(theme::ACCENT));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [steps, fields] = Layout::horizontal([Constraint::Length(22), Constraint::Min(0)])
        .spacing(2)
        .areas(inner);
    let active_step = match form.field() {
        0..=2 => 0,
        3..=4 => 1,
        5..=6 => 2,
        7..=9 => 3,
        10..=11 => 4,
        12..=15 => 5,
        _ => 6,
    };
    let step_names = [
        "1  Type & source",
        "2  Project & profiles",
        "3  CPU & memory",
        "4  Storage & network",
        "5  Initialization",
        "6  Security & boot",
        "7  Review & create",
    ];
    frame.render_widget(
        Paragraph::new(
            step_names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    Line::styled(
                        format!("{} {name}", if index == active_step { "▌" } else { " " }),
                        if index == active_step {
                            Style::default()
                                .fg(theme::ACCENT)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            theme::dim()
                        },
                    )
                })
                .collect::<Vec<_>>(),
        )
        .block(theme::panel_bare()),
        steps,
    );
    let mut lines = vec![
        Line::styled(step_names[active_step], theme::title()),
        Line::raw(""),
    ];
    for field in 0..17 {
        let selected = field == form.field();
        let style = if selected {
            Style::default()
                .bg(theme::SURFACE)
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::subtle()
        };
        let value_style = if selected {
            style
        } else {
            Style::default().fg(theme::TEXT)
        };
        lines.push(Line::from(vec![
            Span::styled(if selected { "▌ " } else { "  " }, theme::label()),
            Span::styled(format!("{:<20}", form_field_name(field)), style),
            Span::styled(form.value(field), value_style),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!(
            "Editing {} · Tab/Shift-Tab moves · Space toggles · final Enter creates · Esc cancels",
            form.field_name()
        ),
        theme::muted(),
    ));
    lines.push(Line::styled(
        "images: aliases pull from images.linuxcontainers.org; other values use a local image alias.",
        theme::muted(),
    ));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), fields);
}

fn render_exec_form(frame: &mut Frame, area: Rect, form: &super::app::ExecForm) {
    let popup = centered_rect(84, 72, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::from(vec![
        Span::styled("Instance  ", theme::muted()),
        Span::styled(form.instance_name(), theme::heading()),
        Span::styled("   Project  ", theme::muted()),
        Span::raw(if form.project().is_empty() {
            "default"
        } else {
            form.project()
        }),
    ])];
    if form.vm_agent_notice() {
        lines.push(Line::styled(
            "VM command execution requires a running incus-agent.",
            Style::default().fg(theme::WARNING),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("$ ", Style::default().fg(theme::ACCENT)),
        Span::raw(form.command()),
    ]));
    lines.push(Line::styled(
        "Enter runs through /bin/sh -lc. Esc closes this command result.",
        theme::muted(),
    ));
    if let Some(result) = form.result() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!(
                "Exit status: {}",
                result
                    .exit_status
                    .map_or_else(|| "unknown".to_owned(), |status| status.to_string())
            ),
            if result.exit_status == Some(0) {
                Style::default().fg(theme::SUCCESS)
            } else {
                Style::default().fg(theme::WARNING)
            },
        ));
        lines.push(Line::styled("stdout", theme::heading()));
        lines.extend(output_lines(&result.stdout));
        if !result.stderr.is_empty() {
            lines.push(Line::styled("stderr", Style::default().fg(theme::DANGER)));
            lines.extend(output_lines(&result.stderr));
        }
    }
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(theme::panel(" Run command "));
    frame.render_widget(widget, popup);
}

fn render_snapshot_form(frame: &mut Frame, area: Rect, form: &super::app::SnapshotForm) {
    let popup = centered_rect(62, 34, area);
    frame.render_widget(Clear, popup);
    let project = if form.project().is_empty() {
        "default"
    } else {
        form.project()
    };
    let message = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Instance  ", theme::muted()),
            Span::styled(form.instance_name(), theme::heading()),
            Span::styled("   Project  ", theme::muted()),
            Span::raw(project),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Name      ", theme::muted()),
            Span::styled(form.name(), Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("Stateful  ", theme::muted()),
            Span::styled(
                if form.stateful() { "yes" } else { "no" },
                if form.stateful() {
                    Style::default().fg(theme::WARNING)
                } else {
                    Style::default().fg(theme::TEXT)
                },
            ),
        ]),
        Line::raw(""),
        Line::styled(
            "Space toggles stateful; Enter creates; Esc cancels.",
            theme::muted(),
        ),
    ])
    .block(theme::panel(" Create snapshot "));
    frame.render_widget(message, popup);
}

fn render_backup_form(frame: &mut Frame, area: Rect, form: &super::app::BackupForm) {
    let popup = centered_rect(66, 40, area);
    frame.render_widget(Clear, popup);
    let project = if form.project().is_empty() {
        "default"
    } else {
        form.project()
    };
    let message = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Instance  ", theme::muted()),
            Span::styled(form.instance_name(), theme::heading()),
            Span::styled("   Project  ", theme::muted()),
            Span::raw(project),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Name               ", theme::muted()),
            Span::raw(form.name()),
        ]),
        Line::from(vec![
            Span::styled("Instance-only (i)  ", theme::muted()),
            Span::raw(if form.instance_only() { "yes" } else { "no" }),
        ]),
        Line::from(vec![
            Span::styled("Optimized storage (z) ", theme::muted()),
            Span::raw(if form.optimized_storage() {
                "yes"
            } else {
                "no"
            }),
        ]),
        Line::raw(""),
        Line::styled(
            "i/z toggle options; Enter creates; Esc cancels.",
            theme::muted(),
        ),
    ])
    .block(theme::panel(" Create backup "));
    frame.render_widget(message, popup);
}

fn render_console_form(frame: &mut Frame, area: Rect, form: &super::app::ConsoleForm) {
    let popup = centered_rect(88, 78, area);
    frame.render_widget(Clear, popup);
    let project = if form.project().is_empty() {
        "default"
    } else {
        form.project()
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("Instance  ", theme::muted()),
        Span::styled(form.instance_name(), theme::heading()),
        Span::styled("   Project  ", theme::muted()),
        Span::raw(project),
    ])];
    if form.vm_notice() {
        lines.push(Line::styled(
            "This is the retained serial console log. Interactive VM console handoff is pending.",
            Style::default().fg(theme::WARNING),
        ));
    }
    lines.push(Line::styled("Console output", theme::heading()));
    lines.extend(output_lines(form.output()));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "r refreshes the log; Esc closes.",
        theme::muted(),
    ));
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(theme::panel(" Serial console "));
    frame.render_widget(widget, popup);
}

fn render_detail_form(frame: &mut Frame, area: Rect, form: &super::app::DetailForm) {
    let popup = centered_rect(88, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = Vec::new();
    if let Some(detail) = form.detail() {
        let instance = &detail.summary;
        lines.push(Line::from(vec![
            Span::styled("Name  ", theme::muted()),
            Span::styled(&instance.name, theme::heading()),
            Span::styled("  Type  ", theme::muted()),
            Span::raw(instance.type_label()),
            Span::styled("  Status  ", theme::muted()),
            Span::styled(&instance.status, theme::status(&instance.status)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Project  ", theme::muted()),
            Span::raw(&instance.project),
            Span::styled("  Location  ", theme::muted()),
            Span::raw(&instance.location),
        ]));
        lines.push(Line::raw(""));
        let runtime = form.runtime_rows();
        if !runtime.is_empty() {
            lines.push(Line::styled("Runtime and guest state", theme::heading()));
            lines.extend(runtime.into_iter().take(12).map(|(label, value)| {
                Line::from(vec![
                    Span::styled(format!("{label:<16}"), theme::muted()),
                    Span::raw(value),
                ])
            }));
            lines.push(Line::raw(""));
        }
        if form.expanded_diff() {
            lines.push(Line::from(vec![
                Span::styled("Expanded/profile contribution", theme::heading()),
                Span::styled(
                    format!("   profiles: {}", instance.profiles.join(", ")),
                    theme::muted(),
                ),
            ]));
            let inherited = detail
                .expanded_config
                .iter()
                .filter(|(key, value)| detail.config.get(*key) != Some(*value))
                .collect::<Vec<_>>();
            if inherited.is_empty() {
                lines.push(Line::styled(
                    "(no inherited configuration differences)",
                    theme::muted(),
                ));
            } else {
                lines.extend(inherited.into_iter().take(16).map(|(key, value)| {
                    Line::from(vec![
                        Span::styled(format!("+ {key} = "), Style::default().fg(theme::GREEN)),
                        Span::raw(value),
                    ])
                }));
            }
            lines.push(Line::styled("Expanded/profile devices", theme::heading()));
            let inherited_devices = detail
                .expanded_devices
                .iter()
                .filter(|(name, device)| detail.devices.get(*name) != Some(*device))
                .collect::<Vec<_>>();
            if inherited_devices.is_empty() {
                lines.push(Line::styled(
                    "(no inherited device differences)",
                    theme::muted(),
                ));
            } else {
                lines.extend(
                    inherited_devices
                        .into_iter()
                        .take(10)
                        .map(|(name, device)| {
                            let values = device
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join(", ");
                            Line::from(vec![
                                Span::styled(
                                    format!("+ {name}: "),
                                    Style::default().fg(theme::GREEN),
                                ),
                                Span::raw(values),
                            ])
                        }),
                );
            }
        } else {
            lines.push(Line::styled("Local configuration", theme::heading()));
            if detail.config.is_empty() {
                lines.push(Line::styled("(no local configuration)", theme::muted()));
            } else {
                lines.extend(detail.config.iter().take(12).map(|(key, value)| {
                    Line::from(vec![
                        Span::styled(format!("{key} = "), theme::muted()),
                        Span::raw(value),
                    ])
                }));
            }
            lines.push(Line::styled("Local devices", theme::heading()));
            if detail.devices.is_empty() {
                lines.push(Line::styled("(no local devices)", theme::muted()));
            } else {
                lines.extend(detail.devices.iter().take(8).map(|(name, device)| {
                    let parts = device
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Line::from(vec![
                        Span::styled(format!("{name}: "), theme::muted()),
                        Span::raw(parts),
                    ])
                }));
            }
        }
    } else {
        lines.push(Line::styled("Loading instance definition…", theme::muted()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Tab/d toggles local vs expanded/profile diff · r refreshes · Esc closes",
        theme::muted(),
    ));
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(theme::panel(" Instance details "));
    frame.render_widget(widget, popup);
}

fn render_journal_viewer(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    viewer: &super::app::JournalViewer,
) {
    let popup = centered_rect(82, 72, area);
    frame.render_widget(Clear, popup);
    let rows = app.journal_rows();
    let mut lines = vec![Line::from(vec![
        Span::styled("Filter  ", theme::label()),
        Span::styled(
            if viewer.filter().is_empty() {
                "(all actions)"
            } else {
                viewer.filter()
            },
            if viewer.filtering() {
                Style::default().fg(theme::ACCENT)
            } else {
                theme::text()
            },
        ),
        Span::styled(format!("   {} entries", rows.len()), theme::dim()),
    ])];
    lines.push(Line::styled(
        "/ filters · j/k navigates · x/Esc closes · payloads and secrets are never recorded",
        theme::dim(),
    ));
    lines.push(Line::raw(""));
    let visible = popup.height.saturating_sub(6) as usize;
    let start = viewer
        .selected()
        .saturating_sub(visible / 2)
        .min(rows.len().saturating_sub(visible));
    for (index, entry) in rows.iter().enumerate().skip(start).take(visible) {
        let selected = index == viewer.selected();
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(format!("{:<12}", entry.timestamp), theme::muted()),
            Span::styled(format!("{:<18}", entry.target), theme::label()),
            Span::styled(
                format!("{:<32}", entry.action),
                if selected {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
            Span::styled(&entry.status, theme::dim()),
        ]));
    }
    if rows.is_empty() {
        lines.push(Line::styled("No matching local actions.", theme::muted()));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(" Local action journal ")),
        popup,
    );
}

fn render_fleet_manager(frame: &mut Frame, area: Rect, manager: &super::app::FleetManager) {
    let popup = centered_rect(92, 72, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::styled(
        "Enter switches target · r probes every remote concurrently · j/k navigates · x/Esc closes",
        theme::dim(),
    )];
    lines.push(Line::from(vec![
        Span::styled(format!("{:<18}", "Remote"), theme::label()),
        Span::styled(format!("{:<18}", "Version"), theme::label()),
        Span::styled(format!("{:>10}", "Instances"), theme::label()),
        Span::styled(format!("{:>10}", "Running"), theme::label()),
        Span::styled(format!("{:>10}", "Warnings"), theme::label()),
        Span::styled(format!("{:>9}", "Drift"), theme::label()),
    ]));
    lines.push(Line::raw(""));
    if manager.loading() {
        lines.push(Line::styled("Probing saved remotes…", theme::muted()));
    } else if manager.rows().is_empty() {
        lines.push(Line::styled(
            "No saved remotes. Add targets with c first.",
            theme::muted(),
        ));
    }
    let visible = popup.height.saturating_sub(7) as usize;
    let start = manager
        .selected()
        .saturating_sub(visible / 2)
        .min(manager.rows().len().saturating_sub(visible));
    for (index, row) in manager.rows().iter().enumerate().skip(start).take(visible) {
        let selected = index == manager.selected();
        let style = if selected {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else if row.error.is_some() {
            Style::default().fg(theme::RED)
        } else {
            theme::text()
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(format!("{:<16}", row.name), style),
            Span::styled(format!("{:<18}", row.version), style),
            Span::styled(format!("{:>10}", row.instances), style),
            Span::styled(format!("{:>10}", row.running), style),
            Span::styled(format!("{:>10}", row.warnings), style),
            Span::styled(format!("{:>9}", row.drift_keys), style),
        ]));
        if selected {
            lines.push(Line::styled(
                row.error.as_deref().unwrap_or(&row.endpoint),
                if row.error.is_some() {
                    Style::default().fg(theme::RED)
                } else {
                    theme::dim()
                },
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Drift = server configuration keys differing from the first reachable saved remote.",
        theme::muted(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(" Fleet health and configuration drift ")),
        popup,
    );
}

fn render_placement_inspector(
    frame: &mut Frame,
    area: Rect,
    inspector: &super::app::PlacementInspector,
) {
    let popup = centered_rect(88, 72, area);
    frame.render_widget(Clear, popup);
    let instance = inspector.instance();
    let evacuation = instance
        .config
        .get("cluster.evacuate")
        .map(String::as_str)
        .unwrap_or("auto");
    let mut lines = vec![Line::from(vec![
        Span::styled("Instance  ", theme::label()),
        Span::styled(&instance.name, theme::title()),
        Span::styled("   Architecture  ", theme::label()),
        Span::raw(&instance.architecture),
        Span::styled("   Evacuation  ", theme::label()),
        Span::raw(evacuation),
    ])];
    lines.push(Line::styled(
        "Observable preflight only: server placement scriptlets and resource capacity make the authoritative decision.",
        theme::dim(),
    ));
    lines.push(Line::raw(""));
    if inspector.members().is_empty() {
        lines.push(Line::styled(
            "No cluster members reported; this target is not clustered or member data is unavailable.",
            theme::muted(),
        ));
    }
    for (index, member) in inspector.members().iter().enumerate() {
        let selected = index == inspector.selected();
        let (eligible, reason) = inspector.eligibility(member);
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(
                format!("{:<20}", member.server_name),
                if selected {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
            Span::styled(
                format!("{:<12}", member.status),
                theme::status(&member.status),
            ),
            Span::styled(format!("{:<14}", member.architecture), theme::muted()),
            Span::styled(
                if eligible { "ELIGIBLE" } else { "BLOCKED" },
                Style::default().fg(if eligible { theme::GREEN } else { theme::RED }),
            ),
            Span::styled(
                if member.server_name == instance.location {
                    "  current"
                } else {
                    ""
                },
                theme::dim(),
            ),
        ]));
        if selected {
            lines.push(Line::styled(format!("  {reason}"), theme::muted()));
            lines.push(Line::styled(
                format!(
                    "  failure domain: {} · groups: {} · roles: {}",
                    if member.failure_domain.is_empty() {
                        "default"
                    } else {
                        &member.failure_domain
                    },
                    if member.groups.is_empty() {
                        "none".into()
                    } else {
                        member.groups.join(", ")
                    },
                    if member.roles.is_empty() {
                        "none".into()
                    } else {
                        member.roles.join(", ")
                    }
                ),
                theme::dim(),
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "j/k navigates · x/Esc closes · E on Instances edits evacuation mode",
        theme::dim(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(" Placement and evacuation policy ")),
        popup,
    );
}

fn render_backup_schedule(frame: &mut Frame, area: Rect, form: &super::app::BackupScheduleForm) {
    let popup = centered_rect(72, 56, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::from(vec![
        Span::styled("Instance  ", theme::label()),
        Span::styled(form.instance(), theme::title()),
        Span::styled("   Project  ", theme::label()),
        Span::raw(form.project()),
    ])];
    lines.push(Line::styled(
        "Runs while virtty is active · {timestamp} expands at execution · Delete removes schedule",
        theme::dim(),
    ));
    lines.push(Line::raw(""));
    for (label, value, choice, focused) in form.fields() {
        lines.push(Line::from(vec![
            Span::styled(
                if focused { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(format!("{label:<20}"), theme::label()),
            Span::styled(
                value,
                if focused {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else if choice {
                    theme::subtle()
                } else {
                    theme::text()
                },
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Tab changes field · Space/Left/Right changes choices · Enter on Last result saves · Esc cancels",
        theme::muted(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(" Scheduled backup ")),
        popup,
    );
}

fn render_image_retention(
    frame: &mut Frame,
    area: Rect,
    preview: &super::app::ImageRetentionPreview,
) {
    let popup = centered_rect(76, 70, area);
    frame.render_widget(Clear, popup);
    let delete_count = preview.rows().iter().filter(|row| row.2).count();
    let mut lines = vec![Line::styled(
        format!(
            "DRY RUN · cache expiry {} hours · {delete_count} exact deletion candidates",
            preview.expiry_hours()
        ),
        Style::default().fg(theme::YELLOW),
    )];
    lines.push(Line::styled("Only server-reported cached images participate · r recalculates · j/k navigates · x/Esc closes", theme::dim()));
    lines.push(Line::raw(""));
    for (index, (name, expires, delete)) in preview.rows().iter().enumerate() {
        let selected = index == preview.selected();
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(
                if *delete { "DELETE  " } else { "KEEP    " },
                Style::default().fg(if *delete { theme::RED } else { theme::GREEN }),
            ),
            Span::styled(
                format!("{name:<28}"),
                if selected {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
            Span::styled(expires, theme::muted()),
        ]));
    }
    if preview.rows().is_empty() {
        lines.push(Line::styled(
            "No cached images were reported.",
            theme::muted(),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(" Cached image retention simulation ")),
        popup,
    );
}

fn render_openfga_manager(frame: &mut Frame, area: Rect, manager: &super::app::OpenFgaManager) {
    let popup = centered_rect(90, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::from(vec![
        Span::styled("Operation  ", theme::label()),
        Span::styled(manager.operation(), theme::title()),
        Span::styled("   F2 cycles check/grant/revoke", theme::dim()),
    ])];
    lines.push(Line::styled("Tab changes field · Space cycles roles/types · Enter checks or stages confirmation · r refreshes", theme::dim()));
    lines.push(Line::raw(""));
    for (label, value, focused) in manager.fields() {
        lines.push(Line::from(vec![
            Span::styled(
                if focused { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(format!("{label:<18}"), theme::label()),
            Span::styled(
                value,
                if focused {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
        ]));
    }
    if manager.confirming() {
        lines.push(Line::styled(
            format!("Type exactly: {} relationship", manager.operation()),
            Style::default().fg(theme::RED),
        ));
        lines.push(Line::styled(manager.confirmation(), theme::text()));
    }
    if !manager.result().is_empty() {
        lines.push(Line::styled(
            manager.result(),
            Style::default().fg(theme::BLUE),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Current relationship tuples",
        theme::heading(),
    ));
    let visible = popup.height.saturating_sub(17) as usize;
    let start = manager
        .selected()
        .saturating_sub(visible / 2)
        .min(manager.tuples().len().saturating_sub(visible));
    for (index, tuple) in manager
        .tuples()
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
    {
        let selected = index == manager.selected();
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(
                format!("{:<30}", tuple.user),
                if selected {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::text()
                },
            ),
            Span::styled(format!("{:<24}", tuple.relation), theme::label()),
            Span::raw(&tuple.object),
        ]));
    }
    if manager.tuples().is_empty() {
        lines.push(Line::styled(
            "No relationship tuples returned.",
            theme::muted(),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel(
                " OpenFGA relationships and entitlement check ",
            )),
        popup,
    );
}

fn render_config_form(frame: &mut Frame, area: Rect, form: &super::app::ConfigForm) {
    let popup = centered_rect(76, 58, area);
    frame.render_widget(Clear, popup);
    let project = if form.project().is_empty() {
        "default"
    } else {
        form.project()
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("Resource  ", theme::label()),
        Span::styled(form.kind().label(), theme::text()),
        Span::styled("   Name  ", theme::label()),
        Span::styled(form.name(), theme::title()),
        Span::styled("   Project  ", theme::label()),
        Span::styled(project, theme::text()),
    ])];
    lines.push(Line::styled(
        "Tab changes field; Space/Left/Right changes toggles and choices; Enter saves from the last field; Esc cancels.",
        theme::dim(),
    ));
    lines.push(Line::raw(""));
    let fields = form.fields().collect::<Vec<_>>();
    let focused_index = fields
        .iter()
        .position(|(_, _, _, _, focused)| *focused)
        .unwrap_or(0);
    let visible_count = popup.height.saturating_sub(7) as usize;
    let start = focused_index
        .saturating_sub(visible_count / 2)
        .min(fields.len().saturating_sub(visible_count));
    for (label, value, toggle, secret, focused) in
        fields.iter().skip(start).take(visible_count).copied()
    {
        let marker = if focused { "▌ " } else { "  " };
        let style = if focused {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else {
            theme::subtle()
        };
        let value = if secret {
            if value.is_empty() {
                "(not set)".to_owned()
            } else {
                "•••••••• (set; typing replaces)".to_owned()
            }
        } else if toggle {
            if value == "true" { "on" } else { "off" }.to_owned()
        } else {
            value.to_owned()
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(theme::ACCENT)),
            Span::styled(format!("{label:<22}"), theme::label()),
            Span::styled(value, style),
        ]));
    }
    if fields.len() > visible_count {
        lines.push(Line::styled(
            format!(
                "fields {}-{} of {}",
                start + 1,
                (start + visible_count).min(fields.len()),
                fields.len()
            ),
            theme::dim(),
        ));
    }
    if !form.omitted_capabilities().is_empty() {
        lines.push(Line::styled(
            format!(
                "Hidden unsupported fields: {}",
                form.omitted_capabilities().join(", ")
            ),
            Style::default().fg(theme::YELLOW),
        ));
    }
    if let Some(preview) = form.cpu_topology_preview() {
        let (message, style) = match preview {
            Ok(message) => (message, Style::default().fg(theme::GREEN)),
            Err(error) => (
                format!("Invalid CPU topology: {error}"),
                Style::default().fg(theme::RED),
            ),
        };
        lines.push(Line::styled(message, style));
    }
    if let Some(preview) = form.snapshot_policy_preview() {
        lines.push(Line::styled(preview, Style::default().fg(theme::BLUE)));
    }
    if let Some(preview) = form.image_policy_preview() {
        lines.push(Line::styled(preview, Style::default().fg(theme::BLUE)));
    }
    if let Some(preview) = form.authorization_preview() {
        lines.push(Line::styled(preview, Style::default().fg(theme::BLUE)));
    }
    let widget = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        theme::panel(format!("configure {}", form.kind().label()))
            .border_style(Style::default().fg(theme::ACCENT)),
    );
    frame.render_widget(widget, popup);
}

fn render_resource_create_form(
    frame: &mut Frame,
    area: Rect,
    form: &super::app::ResourceCreateForm,
) {
    let popup = centered_rect(76, 66, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![Line::styled(
        format!("Create {}", form.kind().label()),
        theme::title(),
    )];
    lines.push(Line::styled(
        "Tab changes field; Space/Left/Right changes choices; Enter on the last field creates.",
        theme::dim(),
    ));
    lines.push(Line::raw(""));
    for (label, value, kind, focused) in form.fields() {
        let marker = if focused { "▌ " } else { "  " };
        let style = if focused {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else {
            theme::subtle()
        };
        let shown = match (kind, value) {
            ("toggle", "true") => "on",
            ("toggle", _) => "off",
            _ => value,
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(theme::ACCENT)),
            Span::styled(format!("{label:<22}"), theme::label()),
            Span::styled(shown, style),
            Span::styled(if kind == "choice" { "  ◂ ▸" } else { "" }, theme::dim()),
        ]));
    }
    if form.kind() == crate::incus::ResourceKind::StoragePool {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Source wipe is creation-only and destructive. Incus performs authoritative mounted/in-use/signature checks.",
            Style::default().fg(theme::RED),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            theme::panel(format!("new {}", form.kind().label()))
                .border_style(Style::default().fg(theme::ACCENT)),
        ),
        popup,
    );
    if let Some(input) = form.wipe_confirmation() {
        render_typed_confirmation(
            frame,
            area,
            "Wipe source and create storage pool",
            &form.wipe_phrase(),
            input,
        );
    }
}

fn render_resource_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    confirmation: &super::app::ResourceDeleteConfirmation,
) {
    let popup = centered_rect(70, 28, area);
    frame.render_widget(Clear, popup);
    let expected = confirmation.expected_phrase();
    let lines = vec![
        Line::styled(
            format!(
                "Delete {} '{}'",
                confirmation.kind().label(),
                confirmation.name()
            ),
            Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Type ", theme::dim()),
            Span::styled(&expected, theme::label().add_modifier(Modifier::BOLD)),
            Span::styled(" to confirm:", theme::dim()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme::RED)),
            Span::styled(confirmation.input(), theme::text()),
        ]),
        Line::raw(""),
        Line::from([theme::chip("↵", "delete"), theme::chip("esc", "cancel")].concat()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme::panel("confirm deletion").border_style(Style::default().fg(theme::RED))),
        popup,
    );
}

fn render_image_alias_form(frame: &mut Frame, area: Rect, form: &super::app::ImageAliasForm) {
    let popup = centered_rect(70, 32, area);
    frame.render_widget(Clear, popup);
    let focused = |field| {
        if form.field() == field {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else {
            theme::subtle()
        }
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Image  ", theme::label()),
            Span::styled(
                form.fingerprint().chars().take(16).collect::<String>(),
                theme::text(),
            ),
            Span::styled("   Project  ", theme::label()),
            Span::styled(
                if form.project().is_empty() {
                    "default"
                } else {
                    form.project()
                },
                theme::text(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(if form.field() == 0 { "▌ " } else { "  " }, theme::label()),
            Span::styled("Alias        ", theme::label()),
            Span::styled(form.alias(), focused(0)),
        ]),
        Line::from(vec![
            Span::styled(if form.field() == 1 { "▌ " } else { "  " }, theme::label()),
            Span::styled("Description  ", theme::label()),
            Span::styled(form.description(), focused(1)),
        ]),
        Line::raw(""),
        Line::from(
            [
                theme::chip("↹", "field"),
                theme::chip("↵", "create"),
                theme::chip("esc", "cancel"),
            ]
            .concat(),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            theme::panel("add image alias").border_style(Style::default().fg(theme::ACCENT)),
        ),
        popup,
    );
}

fn render_image_alias_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::ImageAliasManager,
) {
    let popup = centered_rect(76, 64, area);
    frame.render_widget(Clear, popup);
    let rows = manager.aliases().iter().enumerate().map(|(index, alias)| {
        Row::new([alias.name.clone(), alias.description.clone()]).style(
            if index == manager.selected() {
                theme::selected_row()
            } else {
                theme::text()
            },
        )
    });
    frame.render_widget(
        Table::new(rows, [Constraint::Length(28), Constraint::Min(30)])
            .header(Row::new(["Alias", "Description"]).style(theme::heading()))
            .block(theme::panel(format!(
                "aliases for {} · e edit · d delete · Esc close",
                manager.fingerprint().chars().take(12).collect::<String>()
            ))),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let editor = centered_rect(62, 34, area);
        frame.render_widget(Clear, editor);
        let mut lines = vec![
            Line::styled("Edit image alias", theme::title()),
            Line::raw(""),
        ];
        lines.extend(
            fields
                .into_iter()
                .map(|(label, value, focused)| form_line(label, value, focused)),
        );
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab fields · Enter save · Esc cancel",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("alias form")),
            editor,
        );
    }
    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(66, 26, area);
        frame.render_widget(Clear, confirm);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete image alias", theme::title()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type  ", theme::label()),
                    Span::styled(manager.delete_phrase(), theme::text()),
                ]),
                Line::from(vec![
                    Span::styled("> ", theme::label()),
                    Span::styled(input, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
                ]),
            ])
            .block(theme::panel("confirm deletion")),
            confirm,
        );
    }
}

fn render_image_form(frame: &mut Frame, area: Rect, form: &super::app::ImageForm) {
    use super::app::ImageForm;
    let popup = centered_rect(72, 48, area);
    frame.render_widget(Clear, popup);
    let (title, lines) = match form {
        ImageForm::Edit {
            spec,
            fields,
            field,
        } => {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Image  ", theme::label()),
                    Span::styled(
                        spec.fingerprint.chars().take(16).collect::<String>(),
                        theme::text(),
                    ),
                    Span::styled("   Project  ", theme::label()),
                    Span::styled(&spec.project, theme::text()),
                ]),
                Line::raw(""),
            ];
            for (index, (label, value)) in [
                "Description",
                "OS",
                "Release",
                "Variant",
                "Requires privileged",
                "Requires nesting",
                "Requires secure boot",
                "Requires agent CD-ROM",
                "Requires cloud-init CD-ROM",
            ]
            .into_iter()
            .zip(fields.iter())
            .enumerate()
            {
                lines.push(form_line(label, value, *field == index));
            }
            lines.push(toggle_line("Public", spec.public, *field == 9));
            lines.push(toggle_line("Auto update", spec.auto_update, *field == 10));
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "Tab fields  Space toggle  Enter save  Esc cancel",
                theme::subtle(),
            ));
            ("edit image", lines)
        }
        ImageForm::Import {
            project,
            path,
            aliases,
            public,
            field,
        } => (
            "import image",
            vec![
                form_line("Project", project, *field == 0),
                form_line("Archive", path, *field == 1),
                form_line("Aliases", aliases, *field == 2),
                toggle_line("Public", *public, *field == 3),
                Line::raw(""),
                Line::styled(
                    "Unified Incus image archive  •  Enter import  •  Esc cancel",
                    theme::subtle(),
                ),
            ],
        ),
        ImageForm::Export {
            fingerprint,
            project,
            path,
        } => (
            "export image",
            vec![
                Line::from(vec![
                    Span::styled("Image  ", theme::label()),
                    Span::styled(
                        fingerprint.chars().take(16).collect::<String>(),
                        theme::text(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Project  ", theme::label()),
                    Span::styled(project, theme::text()),
                ]),
                Line::raw(""),
                form_line("Destination", path, true),
                Line::raw(""),
                Line::styled("Enter export  •  Esc cancel", theme::subtle()),
            ],
        ),
        ImageForm::Publish { spec, field } => (
            "publish instance as image",
            vec![
                Line::from(vec![
                    Span::styled("Instance  ", theme::label()),
                    Span::styled(&spec.instance, theme::text()),
                    Span::styled("   Project  ", theme::label()),
                    Span::styled(&spec.project, theme::text()),
                ]),
                Line::raw(""),
                form_line("Alias", &spec.alias, *field == 0),
                form_line("Description", &spec.description, *field == 1),
                toggle_line("Public", spec.public, *field == 2),
                Line::raw(""),
                Line::styled(
                    "Tab fields  Space toggle  Enter publish  Esc cancel",
                    theme::subtle(),
                ),
            ],
        ),
        ImageForm::Push {
            fingerprint,
            project,
            fields,
            field,
        } => {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Image  ", theme::label()),
                    Span::styled(
                        fingerprint.chars().take(16).collect::<String>(),
                        theme::text(),
                    ),
                    Span::styled("   Project  ", theme::label()),
                    Span::styled(project, theme::text()),
                ]),
                Line::raw(""),
            ];
            for (index, (label, value)) in [
                "Target URL",
                "Target project",
                "Receive secret",
                "Server certificate",
                "Aliases",
                "Profiles",
            ]
            .into_iter()
            .zip(fields.iter())
            .enumerate()
            {
                lines.push(form_line(label, value, *field == index));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled("The target must provide an image receive secret. Certificate is an optional PEM file path.", theme::subtle()));
            lines.push(Line::styled("Enter push  •  Esc cancel", theme::subtle()));
            ("push image to remote", lines)
        }
        ImageForm::CopyProject {
            fingerprint,
            source_project,
            target_project,
            public,
            field,
        } => (
            "copy image to project",
            vec![
                Line::from(vec![
                    Span::styled("Image  ", theme::label()),
                    Span::styled(
                        fingerprint.chars().take(16).collect::<String>(),
                        theme::text(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Source project  ", theme::label()),
                    Span::styled(source_project, theme::text()),
                ]),
                Line::raw(""),
                form_line("Target project", target_project, *field == 0),
                toggle_line("Public", *public, *field == 1),
                Line::raw(""),
                Line::styled(
                    "The image archive is streamed through virtty and imported into the target project.",
                    theme::subtle(),
                ),
                Line::styled("Enter copy  •  Esc cancel", theme::subtle()),
            ],
        ),
    };
    frame.render_widget(Paragraph::new(lines).block(theme::panel(title)), popup);
}

fn form_line<'a>(label: &'a str, value: &'a str, focused: bool) -> Line<'a> {
    Line::from(vec![
        Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
        Span::styled(format!("{label:<13}"), theme::label()),
        Span::styled(
            if value.is_empty() { " " } else { value },
            if focused {
                Style::default().bg(theme::SURFACE).fg(theme::TEXT)
            } else {
                theme::text()
            },
        ),
    ])
}

fn toggle_line(label: &str, enabled: bool, focused: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
        Span::styled(format!("{label:<13}"), theme::label()),
        Span::styled(
            if enabled { "● on" } else { "○ off" },
            if focused {
                theme::selected_row()
            } else {
                theme::text()
            },
        ),
    ])
}

fn render_artifacts_form(frame: &mut Frame, area: Rect, form: &super::app::ArtifactsForm) {
    use super::app::ArtifactTab;

    let popup = centered_rect(84, 76, area);
    frame.render_widget(Clear, popup);
    let selected_tab = |tab| {
        if form.tab() == tab {
            Style::default()
                .fg(theme::SELECTED_TAB_TEXT)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::subtle()
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Instance  ", theme::label()),
            Span::styled(form.instance_name(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(
                if form.project().is_empty() {
                    "default"
                } else {
                    form.project()
                },
                theme::text(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!(" snapshots ({}) ", form.snapshots().len()),
                selected_tab(ArtifactTab::Snapshots),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" backups ({}) ", form.backups().len()),
                selected_tab(ArtifactTab::Backups),
            ),
        ]),
        Line::raw(""),
    ];
    match form.tab() {
        ArtifactTab::Snapshots => {
            if form.snapshots().is_empty() {
                lines.push(Line::styled("No snapshots", theme::dim()));
            } else {
                lines.extend(form.snapshots().iter().enumerate().map(|(index, item)| {
                    Line::from(vec![
                        Span::styled(
                            if index == form.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{:<28}", item.name), theme::text()),
                        Span::styled(
                            if item.stateful {
                                "stateful  "
                            } else {
                                "stateless "
                            },
                            if item.stateful {
                                Style::default().fg(theme::YELLOW)
                            } else {
                                theme::dim()
                            },
                        ),
                        Span::styled(&item.created_at, theme::dim()),
                    ])
                    .style(if index == form.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }));
            }
        }
        ArtifactTab::Backups => {
            if form.backups().is_empty() {
                lines.push(Line::styled("No backups", theme::dim()));
            } else {
                lines.extend(form.backups().iter().enumerate().map(|(index, item)| {
                    Line::from(vec![
                        Span::styled(
                            if index == form.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{:<28}", item.name), theme::text()),
                        Span::styled(
                            if item.instance_only {
                                "instance-only  "
                            } else {
                                "with snapshots "
                            },
                            theme::dim(),
                        ),
                        Span::styled(&item.created_at, theme::dim()),
                    ])
                    .style(if index == form.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("↹", "snapshots/backups"),
            theme::chip("n", "new"),
            theme::chip("r", "restore snapshot"),
            theme::chip("R", "rename snapshot"),
            theme::chip("e", "export backup"),
            theme::chip("i", "import backup"),
            theme::chip("d", "delete"),
            theme::chip("V", "retention preview"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("snapshots and backups")),
        popup,
    );

    if form.retention_preview() {
        let preview = centered_rect(78, 62, area);
        frame.render_widget(Clear, preview);
        let rows = form.retention_rows();
        let expired = rows.iter().filter(|(_, _, expired)| *expired).count();
        let mut lines = vec![
            Line::styled("Retention simulation", theme::title()),
            Line::styled(
                format!("DRY RUN · {expired} expired artifacts would be eligible for deletion now"),
                Style::default().fg(theme::YELLOW),
            ),
            Line::raw(""),
        ];
        if rows.is_empty() {
            lines.push(Line::styled(
                "No snapshots or backups have an expiry timestamp.",
                theme::dim(),
            ));
        } else {
            lines.extend(rows.into_iter().map(|(name, expiry, expired)| {
                Line::from(vec![
                    Span::styled(
                        if expired { "DELETE  " } else { "KEEP    " },
                        Style::default().fg(if expired { theme::RED } else { theme::GREEN }),
                    ),
                    Span::styled(format!("{name:<30}"), theme::text()),
                    Span::styled(expiry, theme::dim()),
                ])
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "This is simulation only; no artifacts are changed. Enter/V/Esc closes.",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(theme::panel("exact retention candidates")),
            preview,
        );
    }

    if let Some(input) = form.delete_input() {
        let confirm = centered_rect(66, 24, area);
        frame.render_widget(Clear, confirm);
        let expected = form.delete_phrase().unwrap_or_default();
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    if form.restoring() {
                        "Restore snapshot"
                    } else {
                        "Delete artifact"
                    },
                    Style::default().fg(if form.restoring() {
                        theme::YELLOW
                    } else {
                        theme::RED
                    }),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type ", theme::dim()),
                    Span::styled(&expected, theme::label()),
                    Span::styled(" to confirm:", theme::dim()),
                ]),
                Line::from(vec![
                    Span::styled("> ", theme::label()),
                    Span::styled(input, theme::text()),
                ]),
            ])
            .block(theme::panel("confirm artifact action").border_style(
                Style::default().fg(if form.restoring() {
                    theme::YELLOW
                } else {
                    theme::RED
                }),
            )),
            confirm,
        );
    }

    if let Some(input) = form.rename_input() {
        let rename = centered_rect(58, 20, area);
        frame.render_widget(Clear, rename);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Rename snapshot", theme::title()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Current  ", theme::label()),
                    Span::styled(form.selected_name().unwrap_or_default(), theme::text()),
                ]),
                Line::from(vec![
                    Span::styled("New name ", theme::label()),
                    Span::styled(
                        input,
                        Style::default()
                            .bg(theme::SURFACE)
                            .fg(theme::TEXT)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::raw(""),
                Line::from([theme::chip("enter", "rename"), theme::chip("esc", "cancel")].concat()),
            ])
            .block(theme::panel("rename snapshot")),
            rename,
        );
    }
    if let Some((importing, path)) = form.transfer() {
        let transfer = centered_rect(72, 24, area);
        frame.render_widget(Clear, transfer);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    if importing {
                        "Import backup archive"
                    } else {
                        "Export backup archive"
                    },
                    theme::title(),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Path  ", theme::label()),
                    Span::styled(path, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
                ]),
                Line::raw(""),
                Line::styled(
                    if importing {
                        "The archive will be imported into the current project."
                    } else {
                        "The backup archive will be written to this local path."
                    },
                    theme::dim(),
                ),
            ])
            .block(
                theme::panel("backup transfer").border_style(Style::default().fg(theme::ACCENT)),
            ),
            transfer,
        );
    }
}

fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(72, 58, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(theme::chip("?", "close help")),
        Line::raw(""),
        Line::styled(app.page().title(), theme::heading()),
        Line::raw(""),
    ];
    lines.extend(match app.page() {
        Page::Dashboard => vec![
            Line::raw("r  Refresh control-plane state"),
            Line::raw("x  Open dashboard actions"),
        ],
        Page::Instances => vec![
            Line::raw("Enter details · n create · e exec · E configure"),
            Line::raw("I shell · V serial · C VGA · v logs · o files"),
            Line::raw("p snapshot · b backup · m artifacts · M rename"),
            Line::raw("T migrate · g devices · P publish · A access"),
            Line::raw("s start · X stop · R restart · f freeze · u unfreeze · d delete"),
        ],
        Page::Operations => vec![
            Line::raw("j/k select operation"),
            Line::raw("Enter open structured details"),
            Line::raw("c cancel selected cancellable operation"),
        ],
        Page::Events => vec![
            Line::raw("j/k select event"),
            Line::raw("r refresh event stream"),
            Line::raw("/ filter lifecycle, logging, and operation events"),
        ],
        Page::Storage => vec![
            Line::raw("j/k select · n create · E configure · d delete"),
            Line::raw("v volumes · b buckets · R resource inventory"),
            Line::raw("G pool growth and driver health"),
        ],
        Page::Networks => vec![
            Line::raw("j/k select · n create · E configure · d delete"),
            Line::raw("X services · z DNS zones · B BGP peers"),
            Line::raw("A address sets · N OVN integrations"),
        ],
        Page::Images => vec![
            Line::raw("j/k select · Enter launch · e edit · d delete"),
            Line::raw("i import · o export · a add alias · A manage aliases"),
        ],
        Page::Projects => vec![
            Line::raw("j/k project · Enter activate · n create · E configure"),
            Line::raw("U usage · A access · d delete"),
            Line::raw("[/] profile · N create · P configure · G devices · D delete"),
        ],
        Page::Settings => vec![
            Line::raw("c connect target · E server configuration · s administration"),
            Line::raw("K cluster groups · L logging targets · T policy scriptlets"),
            Line::raw("G OpenFGA relationships · J action journal · F fleet health"),
        ],
    });
    lines.extend([
        Line::raw(""),
        Line::styled("Navigation", theme::heading()),
        Line::raw("←/→ section · Tab/Shift-Tab view · j/k row"),
        Line::raw("/ filter · x actions · r refresh · q quit · ?/Esc close"),
        Line::raw(""),
        Line::styled(
            "Destructive actions require exact typed confirmation.",
            theme::dim(),
        ),
    ]);
    let message = Paragraph::new(lines).block(theme::panel(format!(
        " {} help ",
        app.page().title().to_lowercase()
    )));
    frame.render_widget(message, popup);
}

fn render_file_form(frame: &mut Frame, area: Rect, form: &super::app::FileForm) {
    let popup = centered_rect(88, 78, area);
    frame.render_widget(Clear, popup);
    let project = if form.project().is_empty() {
        "default"
    } else {
        form.project()
    };
    let path_style = if form.editing_contents() {
        theme::muted()
    } else {
        Style::default()
            .bg(theme::SURFACE)
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    };
    let content_style = if form.editing_contents() {
        Style::default()
            .bg(theme::SURFACE)
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::muted()
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("Instance  ", theme::muted()),
        Span::styled(form.instance_name(), theme::heading()),
        Span::styled("   Project  ", theme::muted()),
        Span::raw(project),
    ])];
    if form.vm_agent_notice() {
        lines.push(Line::styled(
            "VM file access requires a running incus-agent.",
            Style::default().fg(theme::WARNING),
        ));
    }
    lines.push(Line::from(vec![
        Span::styled("Path  ", path_style),
        Span::styled(form.path(), Style::default().fg(theme::TEXT)),
    ]));
    if form.browsing_directory() {
        lines.push(Line::styled("Directory", theme::heading()));
        if form.directory_entries().is_empty() {
            lines.push(Line::styled("  (empty)", theme::muted()));
        } else {
            lines.extend(
                form.directory_entries()
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let marker = if index == form.directory_selected() {
                            "› "
                        } else {
                            "  "
                        };
                        Line::styled(
                            format!("{marker}{entry}"),
                            if index == form.directory_selected() {
                                theme::selected_row()
                            } else {
                                Style::default().fg(theme::TEXT)
                            },
                        )
                    }),
            );
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "j/k select · Enter choose path · b/Esc close browser",
            theme::muted(),
        ));
    }
    lines.push(Line::styled(
        if form.loaded() {
            "Contents"
        } else {
            "Contents (load the path with Enter)"
        },
        content_style,
    ));
    lines.extend(output_lines(form.contents()));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "b browse directory · Tab path/content · Enter load/newline · Ctrl-S save · Ctrl-P pull · Ctrl-U push · Ctrl-D delete",
        theme::muted(),
    ));
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(theme::panel(" Instance files "));
    frame.render_widget(widget, popup);

    if let Some(confirmation) = form.delete_confirmation() {
        let confirmation_popup = centered_rect(68, 26, area);
        frame.render_widget(Clear, confirmation_popup);
        let message = Paragraph::new(vec![
            Line::styled("Delete file", Style::default().fg(theme::DANGER)),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Type ", theme::muted()),
                Span::styled(form.delete_phrase(), theme::heading()),
                Span::styled(" and press Enter:", theme::muted()),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("> ", Style::default().fg(theme::DANGER)),
                Span::raw(confirmation),
            ]),
        ])
        .block(theme::panel(" Confirm file deletion "));
        frame.render_widget(message, confirmation_popup);
    }
    if let Some((upload, local_path)) = form.transfer() {
        let transfer_popup = centered_rect(68, 28, area);
        frame.render_widget(Clear, transfer_popup);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    if upload {
                        "Push local file into instance"
                    } else {
                        "Pull instance file locally"
                    },
                    theme::title(),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Instance path  ", theme::label()),
                    Span::styled(form.path(), theme::text()),
                ]),
                Line::from(vec![
                    Span::styled("Local path     ", theme::label()),
                    Span::styled(
                        local_path,
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT),
                    ),
                ]),
                Line::raw(""),
                Line::styled("Enter transfer · Esc cancel", theme::subtle()),
            ])
            .block(theme::panel("file transfer")),
            transfer_popup,
        );
    }
}

fn render_operation_confirmation(
    frame: &mut Frame,
    area: Rect,
    confirmation: &super::app::OperationConfirmation,
) {
    let popup = centered_rect(72, 28, area);
    frame.render_widget(Clear, popup);
    let message = Paragraph::new(vec![
        Line::styled("Cancel operation", Style::default().fg(theme::DANGER)),
        Line::raw(""),
        Line::from(confirmation.description()),
        Line::from(vec![
            Span::styled("Type ", theme::muted()),
            Span::styled(confirmation.expected_phrase(), theme::heading()),
            Span::styled(" and press Enter:", theme::muted()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme::DANGER)),
            Span::raw(confirmation.input()),
        ]),
        Line::styled("Esc cancels", theme::muted()),
    ])
    .block(theme::panel(" Confirm operation cancellation "));
    frame.render_widget(message, popup);
}

fn render_operation_detail(frame: &mut Frame, area: Rect, form: &super::app::OperationDetailForm) {
    let popup = centered_rect(86, 78, area);
    frame.render_widget(Clear, popup);
    let operation = form.operation();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Status       ", theme::label()),
            Span::styled(&operation.status, operation_style(&operation.status)),
        ]),
        Line::from(vec![
            Span::styled("Description  ", theme::label()),
            Span::styled(
                if operation.description.is_empty() {
                    operation.class.as_str()
                } else {
                    operation.description.as_str()
                },
                theme::text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("ID           ", theme::label()),
            Span::styled(&operation.id, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Class        ", theme::label()),
            Span::styled(&operation.class, theme::text()),
            Span::styled("   Location  ", theme::label()),
            Span::styled(&operation.location, theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Created      ", theme::label()),
            Span::styled(&operation.created_at, theme::subtle()),
        ]),
        Line::from(vec![
            Span::styled("Updated      ", theme::label()),
            Span::styled(&operation.updated_at, theme::subtle()),
        ]),
    ];
    if !operation.err.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Error        ", theme::label()),
            Span::styled(&operation.err, Style::default().fg(theme::RED)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Affected resources", theme::title()));
    if operation.resources.is_empty() {
        lines.push(Line::styled("  none reported", theme::dim()));
    } else {
        for (kind, resources) in &operation.resources {
            lines.push(Line::from(vec![
                Span::styled(format!("{kind:<14}"), theme::label()),
                Span::styled(resources.join(", "), theme::text()),
            ]));
        }
    }
    if let Some(metadata) = operation.metadata.as_object()
        && !metadata.is_empty()
    {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Progress and metadata", theme::title()));
        for (key, value) in metadata.iter().take(12) {
            let value = match value {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Number(value) => value.to_string(),
                serde_json::Value::Bool(value) => value.to_string(),
                _ => "structured data".to_owned(),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<20}"), theme::label()),
                Span::styled(value, theme::text()),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("c", "cancel if supported"),
            theme::chip("g", "jump to resource"),
            theme::chip("↵/esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("operation details")),
        popup,
    );
}

fn allocation_value(value: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key))
        .map(|value| match value {
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Bool(value) => value.to_string(),
            serde_json::Value::Number(value) => value.to_string(),
            _ => value.to_string(),
        })
        .unwrap_or_default()
}

fn render_metrics_explorer(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    explorer: &super::app::MetricsExplorer,
) {
    let popup = centered_rect(94, 82, area);
    frame.render_widget(Clear, popup);
    let rows_data = app
        .model
        .metrics
        .as_deref()
        .map(|metrics| super::app::metric_rows(metrics, explorer))
        .unwrap_or_default();
    let rows = rows_data
        .iter()
        .enumerate()
        .map(|(index, (series, value))| {
            let (name, labels) = series.split_once('{').unwrap_or((series, ""));
            Row::new([
                name.to_owned(),
                labels.trim_end_matches('}').to_owned(),
                (*value).to_owned(),
            ])
            .style(if index == explorer.selected() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    let title = if explorer.filtering() {
        format!(
            " Metrics · {} · filter: {}▌ ",
            explorer.instance(),
            explorer.filter()
        )
    } else {
        format!(
            " Metrics · {} / {} · {} series · / filter · L clear · esc close ",
            explorer.project(),
            explorer.instance(),
            rows_data.len()
        )
    };
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Percentage(52),
            Constraint::Percentage(14),
        ],
    )
    .header(Row::new(["Metric", "Labels", "Value"]).style(theme::heading()))
    .column_spacing(1)
    .block(theme::panel(title));
    frame.render_widget(table, popup);
}

fn render_maintenance_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::InstanceMaintenanceManager,
) {
    use super::app::MaintenanceTab;
    let popup = centered_rect(90, 82, area);
    frame.render_widget(Clear, popup);
    let tabs = [
        MaintenanceTab::Metadata,
        MaintenanceTab::Templates,
        MaintenanceTab::Diagnostics,
    ];
    let tab_names = ["Metadata", "Templates", "Diagnostics"];
    let mut lines = vec![
        Line::from(vec![
            Span::styled(manager.name(), theme::title()),
            Span::styled(format!(" · {}", manager.project()), theme::subtle()),
        ]),
        Line::from(
            tabs.iter()
                .zip(tab_names)
                .flat_map(|(tab, name)| {
                    let style = if *tab == manager.tab() {
                        Style::default()
                            .bg(theme::ACCENT)
                            .fg(theme::SELECTED_TAB_TEXT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        theme::subtle()
                    };
                    [Span::styled(format!(" {name} "), style), Span::raw(" ")]
                })
                .collect::<Vec<_>>(),
        ),
        Line::raw(""),
    ];
    match manager.tab() {
        MaintenanceTab::Metadata => {
            let metadata = manager.metadata();
            for key in ["architecture", "creation_date", "expiry_date"] {
                if let Some(value) = metadata.get(key) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{key:<18}"), theme::label()),
                        Span::styled(value.to_string(), theme::text()),
                    ]));
                }
            }
            if let Some(properties) = metadata
                .get("properties")
                .and_then(serde_json::Value::as_object)
            {
                lines.push(Line::styled("Properties", theme::heading()));
                for (key, value) in properties {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{key:<24}"), theme::label()),
                        Span::styled(value.as_str().unwrap_or_default(), theme::text()),
                    ]));
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(theme::chip("n", "set property")));
        }
        MaintenanceTab::Templates => {
            if manager.templates().is_empty() {
                lines.push(Line::styled("No metadata templates.", theme::muted()));
            }
            for (index, name) in manager.templates().iter().enumerate() {
                lines.push(Line::styled(
                    format!(
                        "{} {name}",
                        if index == manager.selected() {
                            "▌"
                        } else {
                            " "
                        }
                    ),
                    if index == manager.selected() {
                        theme::selected_row()
                    } else {
                        theme::text()
                    },
                ));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(
                [
                    theme::chip("n", "new"),
                    theme::chip("↵/e", "edit"),
                    theme::chip("P", "ownership/mode"),
                    theme::chip("d", "delete"),
                ]
                .concat(),
            ));
        }
        MaintenanceTab::Diagnostics => {
            lines.push(Line::styled("Advanced recovery controls", theme::heading()));
            lines.push(Line::raw(
                "Memory dumps can contain secrets. Repair is intended for broken VM state.",
            ));
            lines.push(Line::raw(""));
            lines.push(Line::from(
                [
                    theme::chip("m", "export memory"),
                    theme::chip("s", "VGA screenshot"),
                    theme::chip("r", "repair"),
                    theme::chip("p", "SFTP/NBD proxy"),
                    theme::chip("b", "coordinated bitmaps"),
                ]
                .concat(),
            ));
        }
    }
    if let Some((key, value, field)) = manager.metadata_editor() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Set metadata property", theme::heading()));
        lines.push(Line::styled(
            format!("{} Key    {key}", if field == 0 { "▌" } else { " " }),
            theme::text(),
        ));
        lines.push(Line::styled(
            format!("{} Value  {value}", if field == 1 { "▌" } else { " " }),
            theme::text(),
        ));
    }
    if let Some((name, content, field)) = manager.template_editor() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Template editor · Ctrl-S save",
            theme::heading(),
        ));
        lines.push(Line::styled(
            format!("{} Name  {name}", if field == 0 { "▌" } else { " " }),
            theme::text(),
        ));
        lines.push(Line::styled(
            format!("{} Content\n{content}", if field == 1 { "▌" } else { " " }),
            theme::text(),
        ));
    }
    if let Some(fields) = manager.template_permission_fields() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Template ownership and mode",
            theme::heading(),
        ));
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(format!("{label:<14}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::text()
                    },
                ),
            ]));
        }
        lines.push(Line::styled(
            "Tab fields · final Enter saves metadata with ETag",
            theme::dim(),
        ));
    }
    if let Some(input) = manager.delete_input() {
        lines.push(Line::styled(
            format!(
                "Type exactly: delete template {}",
                manager.selected_template().unwrap_or_default()
            ),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(input, theme::text()));
    }
    if let Some(path) = manager.dump_path() {
        lines.push(Line::styled(
            format!("Memory dump path: {path}"),
            theme::text(),
        ));
    }
    if let Some(path) = manager.screenshot_path() {
        lines.push(Line::styled(
            format!("VGA screenshot destination: {path}"),
            theme::text(),
        ));
        lines.push(Line::styled("Enter export · Esc cancel", theme::muted()));
    }
    if let Some(input) = manager.repair_input() {
        lines.push(Line::styled(
            format!("Type exactly: repair {}", manager.name()),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(input, theme::text()));
    }
    if let Some(fields) = manager.protocol_fields() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Binary protocol proxy", theme::heading()));
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<14}"), theme::label()),
                Span::styled(value, theme::text()),
                Span::styled(if choice { " ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::styled(
            "Port 0 chooses a free loopback port; one client is accepted.",
            theme::muted(),
        ));
    }
    if let Some(fields) = manager.bitmap_fields() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Coordinated VM dirty bitmap",
            theme::heading(),
        ));
        lines.push(Line::styled("Creates the same bitmap across the VM and dependent block volumes for a consistent incremental-backup boundary.", theme::subtle()));
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<20}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::text()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::styled(
            "Granularity must be a positive power of two · final Enter creates",
            theme::dim(),
        ));
    }
    if let Some(address) = manager.proxy_address() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Proxy ready: {address}"),
            Style::default().fg(theme::SUCCESS),
        ));
        lines.push(Line::styled(
            "Connect a protocol client now; Enter/Esc closes this notice.",
            theme::muted(),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [theme::chip("tab", "section"), theme::chip("esc", "close")].concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("instance maintenance")),
        popup,
    );
}

fn render_allocation_explorer(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    explorer: &super::app::NetworkAllocationExplorer,
) {
    let popup = centered_rect(94, 78, area);
    frame.render_widget(Clear, popup);
    let filter = explorer.filter().to_lowercase();
    let allocations: Vec<_> = app
        .model
        .network_allocations
        .iter()
        .filter(|value| filter.is_empty() || value.to_string().to_lowercase().contains(&filter))
        .collect();
    let rows = allocations.iter().enumerate().map(|(index, value)| {
        Row::new([
            allocation_value(value, &["address"]),
            allocation_value(value, &["hwaddr", "hw_address", "mac_address"]),
            allocation_value(value, &["type"]),
            allocation_value(value, &["nat"]),
            allocation_value(value, &["project"]),
            allocation_value(value, &["used_by", "used-by"]),
        ])
        .style(if index == explorer.selected() {
            theme::selected_row()
        } else {
            theme::text()
        })
    });
    let title = if explorer.filtering() {
        format!(" Network allocations · filter: {}▌ ", explorer.filter())
    } else {
        format!(
            " Network allocations ({}) · / filter · L clear · esc close ",
            allocations.len()
        )
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(6),
            Constraint::Length(14),
            Constraint::Min(24),
        ],
    )
    .header(
        Row::new(["Address", "MAC", "Type", "NAT", "Project", "Used by"]).style(theme::heading()),
    )
    .column_spacing(1)
    .block(theme::panel(title));
    frame.render_widget(table, popup);
}

fn render_log_manager(frame: &mut Frame, area: Rect, manager: &super::app::InstanceLogManager) {
    let popup = centered_rect(82, 72, area);
    frame.render_widget(Clear, popup);
    if let Some(content) = manager.content() {
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .block(theme::panel(format!(
                    "{} · {} · esc close",
                    manager.name(),
                    manager.selected_log().unwrap_or("log")
                ))),
            popup,
        );
        return;
    }
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Instance  ", theme::label()),
            Span::styled(manager.name(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(manager.project(), theme::text()),
        ]),
        Line::raw(""),
    ];
    if manager.logs().is_empty() {
        lines.push(Line::styled("No retained logs.", theme::muted()));
    } else {
        lines.extend(manager.logs().iter().enumerate().map(|(index, log)| {
            Line::styled(
                format!(
                    "{} {}",
                    if index == manager.selected() {
                        "▌"
                    } else {
                        " "
                    },
                    log
                ),
                if index == manager.selected() {
                    theme::selected_row()
                } else {
                    theme::text()
                },
            )
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("↵", "view"),
            theme::chip("d", "delete"),
            theme::chip("c", "clear console"),
            theme::chip("X", "clear exec outputs"),
            theme::chip("r", "refresh"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    if let Some(input) = manager.delete_input() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Type exactly: {}", manager.delete_phrase()),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(
            input,
            Style::default().bg(theme::SURFACE).fg(theme::TEXT),
        ));
    }
    if let Some(input) = manager.clear_console_input() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Type exactly: {}", manager.clear_phrase()),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(
            input,
            Style::default().bg(theme::SURFACE).fg(theme::TEXT),
        ));
    }
    if let Some(input) = manager.cleanup_exec_input() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Type exactly: {}", manager.cleanup_exec_phrase()),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(
            input,
            Style::default().bg(theme::SURFACE).fg(theme::TEXT),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("instance logs")),
        popup,
    );
}

fn render_rebuild_form(frame: &mut Frame, area: Rect, form: &super::app::RebuildForm) {
    let popup = centered_rect(72, 40, area);
    frame.render_widget(Clear, popup);
    let focus = |field| {
        if form.field() == field {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else {
            theme::text()
        }
    };
    let lines = vec![
        Line::styled(
            "This erases and recreates the root disk.",
            Style::default().fg(theme::DANGER),
        ),
        Line::raw("Snapshots must be removed first; the instance must be stopped."),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Instance      ", theme::label()),
            Span::styled(form.name(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Project       ", theme::label()),
            Span::styled(form.project(), theme::text()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(if form.field() == 0 { "▌ " } else { "  " }, theme::label()),
            Span::styled("Image alias   ", theme::label()),
            Span::styled(form.image(), focus(0)),
        ]),
        Line::styled(
            "Leave image empty to rebuild with an empty root disk.",
            theme::dim(),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled(if form.field() == 1 { "▌ " } else { "  " }, theme::label()),
            Span::styled("Confirmation  ", theme::label()),
            Span::styled(form.confirmation(), focus(1)),
        ]),
        Line::styled(
            format!("Type exactly: {}", form.phrase()),
            Style::default().fg(theme::DANGER),
        ),
        Line::raw(""),
        Line::from([theme::chip("↵", "rebuild"), theme::chip("esc", "cancel")].concat()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("rebuild instance")),
        popup,
    );
}

fn render_rename_form(frame: &mut Frame, area: Rect, form: &super::app::RenameForm) {
    let popup = centered_rect(66, 28, area);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from(vec![
            Span::styled("Current name  ", theme::label()),
            Span::styled(form.current_name(), theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Project       ", theme::label()),
            Span::styled(
                if form.project().is_empty() {
                    "default"
                } else {
                    form.project()
                },
                theme::text(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("▌ New name    ", theme::label()),
            Span::styled(
                form.new_name(),
                Style::default().bg(theme::SURFACE).fg(theme::TEXT),
            ),
        ]),
        Line::raw(""),
        Line::from([theme::chip("↵", "rename"), theme::chip("esc", "cancel")].concat()),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            theme::panel("rename instance").border_style(Style::default().fg(theme::ACCENT)),
        ),
        popup,
    );
}

fn render_settings_manager(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    manager: &super::app::SettingsManager,
) {
    use super::app::SettingsTab;

    let popup = centered_rect(88, 80, area);
    frame.render_widget(Clear, popup);
    let selected_tab = |tab| {
        if manager.tab() == tab {
            Style::default()
                .fg(theme::SELECTED_TAB_TEXT)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::subtle()
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" warnings ({}) ", app.model.warnings.len()),
                selected_tab(SettingsTab::Warnings),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" certificates ({}) ", app.model.certificates.len()),
                selected_tab(SettingsTab::Certificates),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" cluster ({}) ", app.model.cluster_members.len()),
                selected_tab(SettingsTab::Cluster),
            ),
        ]),
        Line::raw(""),
    ];
    match manager.tab() {
        SettingsTab::Warnings => {
            if app.model.warnings.is_empty() {
                lines.push(Line::styled("No server warnings", theme::dim()));
            } else {
                lines.extend(
                    app.model
                        .warnings
                        .iter()
                        .enumerate()
                        .map(|(index, warning)| {
                            Line::from(vec![
                                Span::styled(
                                    if index == manager.selected() {
                                        "▌ "
                                    } else {
                                        "  "
                                    },
                                    theme::label(),
                                ),
                                Span::styled(
                                    format!("{:<13}", warning.status),
                                    if warning.status == "new" {
                                        Style::default().fg(theme::YELLOW)
                                    } else {
                                        theme::dim()
                                    },
                                ),
                                Span::styled(format!("{:<16}", warning.location), theme::subtle()),
                                Span::styled(&warning.last_message, theme::text()),
                            ])
                            .style(if index == manager.selected() {
                                theme::selected_row()
                            } else {
                                Style::default()
                            })
                        }),
                );
            }
        }
        SettingsTab::Certificates => {
            if app.model.certificates.is_empty() {
                lines.push(Line::styled("No trusted client certificates", theme::dim()));
            } else {
                lines.extend(
                    app.model
                        .certificates
                        .iter()
                        .enumerate()
                        .map(|(index, cert)| {
                            Line::from(vec![
                                Span::styled(
                                    if index == manager.selected() {
                                        "▌ "
                                    } else {
                                        "  "
                                    },
                                    theme::label(),
                                ),
                                Span::styled(format!("{:<22}", cert.name), theme::text()),
                                Span::styled(
                                    format!("{:<10}", cert.certificate_type),
                                    theme::subtle(),
                                ),
                                Span::styled(
                                    if cert.restricted {
                                        "restricted  "
                                    } else {
                                        "unrestricted"
                                    },
                                    if cert.restricted {
                                        Style::default().fg(theme::YELLOW)
                                    } else {
                                        Style::default().fg(theme::GREEN)
                                    },
                                ),
                                Span::styled(cert.projects.join(","), theme::dim()),
                            ])
                            .style(if index == manager.selected() {
                                theme::selected_row()
                            } else {
                                Style::default()
                            })
                        }),
                );
            }
        }
        SettingsTab::Cluster => {
            if app.model.cluster_members.is_empty() {
                lines.push(Line::styled("Server is not clustered", theme::dim()));
            } else {
                lines.extend(app.model.cluster_members.iter().enumerate().map(
                    |(index, member)| {
                        Line::from(vec![
                            Span::styled(
                                if index == manager.selected() {
                                    "▌ "
                                } else {
                                    "  "
                                },
                                theme::label(),
                            ),
                            Span::styled(format!("{:<20}", member.server_name), theme::text()),
                            Span::styled(
                                format!("{:<12}", member.status),
                                if member.status == "Online" {
                                    Style::default().fg(theme::GREEN)
                                } else {
                                    Style::default().fg(theme::YELLOW)
                                },
                            ),
                            Span::styled(format!("{:<12}", member.architecture), theme::subtle()),
                            Span::styled(member.roles.join(","), theme::dim()),
                        ])
                        .style(if index == manager.selected() {
                            theme::selected_row()
                        } else {
                            Style::default()
                        })
                    },
                ));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("↹/◂▸", "section"),
            theme::chip("j/k", "select"),
            theme::chip("a", "ack warning"),
            theme::chip("n", "new token/member"),
            theme::chip("e", "edit member"),
            theme::chip("x/r", "evacuate/restore"),
            theme::chip("Q", "capabilities"),
            theme::chip("M", "metadata schema"),
            theme::chip("L", "logging targets"),
            theme::chip("T", "scriptlets"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("access, warnings, and cluster")),
        popup,
    );

    if let Some(filter) = manager.capability_filter() {
        let capabilities = centered_rect(82, 76, area);
        frame.render_widget(Clear, capabilities);
        let needle = filter.to_ascii_lowercase();
        let visible = app
            .model
            .server
            .as_ref()
            .map(|server| {
                server
                    .api_extensions
                    .iter()
                    .filter(|extension| extension.to_ascii_lowercase().contains(&needle))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut capability_lines = vec![
            Line::styled("Server API capabilities", theme::title()),
            Line::from(vec![
                Span::styled("Filter  ", theme::label()),
                Span::styled(filter, theme::text()),
                Span::styled(format!("  {} matches", visible.len()), theme::dim()),
            ]),
            Line::raw(""),
        ];
        if visible.is_empty() {
            capability_lines.push(Line::styled("No matching API extensions", theme::dim()));
        } else {
            capability_lines.extend(visible.iter().enumerate().map(|(index, extension)| {
                Line::from(vec![
                    Span::styled(
                        if index == manager.capability_selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(extension.as_str(), theme::text()),
                ])
                .style(if index == manager.capability_selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        capability_lines.push(Line::raw(""));
        capability_lines.push(Line::styled(
            "Type to filter · Up/Down select · Enter/Esc close",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(capability_lines)
                .block(theme::panel("capability matrix"))
                .wrap(Wrap { trim: false }),
            capabilities,
        );
    }

    if let Some(input) = manager.delete_input() {
        let (kind, id) = match manager.tab() {
            SettingsTab::Warnings => (
                "warning",
                app.model
                    .warnings
                    .get(manager.selected())
                    .map(|item| item.uuid.as_str())
                    .unwrap_or_default(),
            ),
            SettingsTab::Certificates => (
                "certificate",
                app.model
                    .certificates
                    .get(manager.selected())
                    .map(|item| item.fingerprint.as_str())
                    .unwrap_or_default(),
            ),
            SettingsTab::Cluster => ("member", ""),
        };
        let expected = format!("delete {kind} {id}");
        let confirm = centered_rect(74, 24, area);
        frame.render_widget(Clear, confirm);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "Delete trusted/server state",
                    Style::default().fg(theme::RED),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type ", theme::dim()),
                    Span::styled(expected, theme::label()),
                ]),
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(theme::RED)),
                    Span::styled(input, theme::text()),
                ]),
            ])
            .block(theme::panel("confirm deletion").border_style(Style::default().fg(theme::RED))),
            confirm,
        );
    }
    if let Some(fields) = manager.certificate_fields() {
        let form = centered_rect(72, 52, area);
        frame.render_widget(Clear, form);
        let mut lines = vec![
            Line::styled("Add trusted client", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Projects is a comma-separated restriction list. Empty means no project restriction.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("certificate / trust token")
                    .border_style(Style::default().fg(theme::ACCENT)),
            ),
            form,
        );
    }
    if let Some(fields) = manager.certificate_edit_fields() {
        let form = centered_rect(70, 40, area);
        frame.render_widget(Clear, form);
        let mut lines = vec![
            Line::styled("Edit trusted client", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Restricted clients can access only the comma-separated projects.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("certificate access").border_style(Style::default().fg(theme::ACCENT)),
            ),
            form,
        );
    }
    if let Some(token) = manager.trust_token() {
        let reveal = centered_rect(78, 30, area);
        frame.render_widget(Clear, reveal);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Single-use trust token", theme::title()),
                Line::raw(""),
                Line::styled(token, Style::default().fg(theme::TEXT).bg(theme::SURFACE)),
                Line::raw(""),
                Line::styled(
                    "Copy this token now. Enter/Esc closes this reveal.",
                    theme::dim(),
                ),
            ])
            .wrap(Wrap { trim: false })
            .block(theme::panel("trust token").border_style(Style::default().fg(theme::YELLOW))),
            reveal,
        );
    }
    if let Some(fields) = manager.cluster_fields() {
        let edit = centered_rect(72, 40, area);
        frame.render_widget(Clear, edit);
        let mut lines = vec![
            Line::styled("Edit cluster member", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<15}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Roles and groups are comma-separated. Existing member config is preserved.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("cluster member").border_style(Style::default().fg(theme::ACCENT)),
            ),
            edit,
        );
    }
    if let Some((action, input)) = manager.cluster_action() {
        let member = app
            .model
            .cluster_members
            .get(manager.selected())
            .map(|m| m.server_name.as_str())
            .unwrap_or_default();
        let expected = format!("{action} cluster member {member}");
        render_typed_confirmation(
            frame,
            area,
            &format!(
                "{} cluster member",
                match action {
                    "evacuate" => "Evacuate",
                    "delete" => "Delete",
                    _ => "Restore",
                }
            ),
            &expected,
            input,
        );
    }
    if let Some(name) = manager.cluster_join_name() {
        let popup = centered_rect(66, 22, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Generate cluster join token", theme::title()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("New member name  ", theme::label()),
                    Span::styled(name, theme::text()),
                ]),
                Line::raw(""),
                Line::styled(
                    "Enter generates a single-use join token · Esc cancels",
                    theme::dim(),
                ),
            ])
            .block(theme::panel("cluster join")),
            popup,
        );
    }
    if let Some(input) = manager.cluster_rename_input() {
        render_rename_resource(frame, area, "Rename cluster member", input);
    }
    if let Some((title, content)) = manager.cluster_detail() {
        let popup = centered_rect(82, 68, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .block(theme::panel(format!("{title} · esc close"))),
            popup,
        );
    }
    if let Some(fields) = manager.cluster_config_fields() {
        let popup = centered_rect(78, 70, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::styled("Initialize or join cluster", theme::title()),
            Line::raw(""),
        ];
        for (label, value, _, focused) in fields {
            lines.push(form_line(label, value, focused));
        }
        lines.push(toggle_line(
            "Enabled",
            manager.cluster_config_enabled().unwrap_or(false),
            false,
        ));
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab fields · Space toggles enabled · final Enter applies",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("cluster configuration")),
            popup,
        );
    }
    if let Some(fields) = manager.cluster_certificate_fields() {
        let popup = centered_rect(72, 32, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::styled("Rotate cluster certificate", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(form_line(label, value, focused));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Enter applies after the private-key path",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("cluster TLS")),
            popup,
        );
    }
}

fn render_logging_target_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::LoggingTargetManager,
) {
    let popup = centered_rect(86, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Remote logging targets", theme::title()),
        Line::styled(
            "Named Loki, syslog, and webhook destinations. Passwords are never displayed; leave blank to preserve.",
            theme::dim(),
        ),
        Line::raw(""),
    ];
    if manager.targets().is_empty() {
        lines.push(Line::styled(
            "No remote logging targets configured",
            theme::dim(),
        ));
    } else {
        lines.extend(manager.targets().iter().enumerate().map(|(index, name)| {
            Line::from(vec![
                Span::styled(
                    if index == manager.selected() {
                        "▌ "
                    } else {
                        "  "
                    },
                    theme::label(),
                ),
                Span::styled(name, theme::text()),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                Style::default()
            })
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("j/k", "select"),
            theme::chip("n", "new"),
            theme::chip("e/enter", "edit"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("server logging")),
        popup,
    );

    if let Some(fields) = manager.editor_fields() {
        let form_area = centered_rect(78, 76, area);
        frame.render_widget(Clear, form_area);
        let mut form_lines = vec![Line::styled("Logging target", theme::title())];
        form_lines.extend(
            fields
                .into_iter()
                .map(|(label, value, choice, selected, secret)| {
                    let shown = if secret && !value.is_empty() {
                        "••••••"
                    } else {
                        value
                    };
                    Line::from(vec![
                        Span::styled(format!("{label:<25}"), theme::label()),
                        Span::styled(
                            if choice {
                                format!("‹ {shown} ›")
                            } else {
                                shown.to_owned()
                            },
                            if selected {
                                Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                            } else {
                                theme::text()
                            },
                        ),
                    ])
                }),
        );
        form_lines.push(Line::raw(""));
        form_lines.push(Line::styled(
            "Lifecycle projects/types uses PROJECTS;TYPES. Tab moves, arrows change choices, Enter saves on final field.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(form_lines).block(theme::panel("structured logging target")),
            form_area,
        );
    }

    if let Some(input) = manager.delete_input() {
        let name = manager
            .targets()
            .get(manager.selected())
            .map(String::as_str)
            .unwrap_or_default();
        render_typed_confirmation(
            frame,
            area,
            "delete logging target",
            &format!("delete logging target {name}"),
            input,
        );
    }
}

fn render_scriptlet_manager(frame: &mut Frame, area: Rect, manager: &super::app::ScriptletManager) {
    let popup = centered_rect(92, 88, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Scriptlet policy studio  ", theme::title()),
            Span::styled(
                format!(" {} ", manager.kind()),
                Style::default()
                    .fg(theme::SELECTED_TAB_TEXT)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if manager.changed() {
                    "  staged changes"
                } else {
                    "  active baseline"
                },
                if manager.changed() {
                    Style::default().fg(theme::YELLOW)
                } else {
                    theme::dim()
                },
            ),
        ]),
        Line::styled(
            "Danger: these server-wide Starlark policies can deny access or prevent instance placement.",
            Style::default().fg(theme::RED),
        ),
        Line::raw(""),
    ];
    let source_lines = manager.draft().lines().collect::<Vec<_>>();
    if source_lines.is_empty() {
        lines.push(Line::styled("  1 │ ", theme::dim()));
    } else {
        lines.extend(
            source_lines
                .iter()
                .take(24)
                .enumerate()
                .map(|(index, source)| {
                    Line::from(vec![
                        Span::styled(format!("{:>3} │ ", index + 1), theme::dim()),
                        Span::styled(*source, theme::text()),
                    ])
                }),
        );
        if source_lines.len() > 24 {
            lines.push(Line::styled(
                format!("… {} more lines", source_lines.len() - 24),
                theme::dim(),
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.extend(manager.validation().iter().map(|message| {
        Line::styled(
            message,
            if message.starts_with("ERROR:") {
                Style::default().fg(theme::RED)
            } else {
                Style::default().fg(theme::GREEN)
            },
        )
    }));
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("tab", "placement/auth"),
            theme::chip("F2", "validate"),
            theme::chip("F3", "safe template"),
            theme::chip("ctrl-a", "activate"),
            theme::chip("ctrl-r", "rollback"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("staged scriptlet editor")),
        popup,
    );
    if let Some(input) = manager.activation_input() {
        let action = manager.activation_action();
        render_typed_confirmation(
            frame,
            area,
            &format!("{action} {} scriptlet", manager.kind()),
            &format!("{action} {} scriptlet", manager.kind()),
            input,
        );
    }
}

fn render_preseed_manager(frame: &mut Frame, area: Rect, manager: &super::app::PreseedManager) {
    let popup = centered_rect(92, 90, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Typed preseed workflow", theme::title()),
        Line::styled(
            "Dependency order: server → cluster → storage → network → profile",
            theme::dim(),
        ),
        Line::raw(""),
    ];
    let fields = manager.fields();
    let selected_index = fields
        .iter()
        .position(|(_, _, _, selected)| *selected)
        .unwrap_or(0);
    let window = 16;
    let start = selected_index
        .saturating_sub(window / 2)
        .min(fields.len().saturating_sub(window));
    if start > 0 {
        lines.push(Line::styled("↑ earlier fields", theme::dim()));
    }
    lines.extend(fields.iter().skip(start).take(window).cloned().map(
        |(label, value, choice, selected)| {
            let value =
                if (label.contains("token") || label.contains("password")) && !value.is_empty() {
                    "••••••".to_owned()
                } else {
                    value
                };
            Line::from(vec![
                Span::styled(format!("{label:<28}"), theme::label()),
                Span::styled(
                    if choice {
                        format!("‹ {value} ›")
                    } else {
                        value
                    },
                    if selected {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::text()
                    },
                ),
            ])
        },
    ));
    if start + window < fields.len() {
        lines.push(Line::styled("↓ later fields", theme::dim()));
    }
    let staged = manager.staged_entries();
    if !staged.is_empty() {
        lines.push(Line::styled(
            format!("{} staged additional entries", staged.len()),
            theme::heading(),
        ));
        lines.extend(
            staged
                .iter()
                .take(3)
                .map(|entry| Line::styled(entry, theme::dim())),
        );
    }
    if manager.preview() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Preview / per-section results",
            theme::heading(),
        ));
        if manager.results().is_empty() {
            lines.push(Line::styled(
                "Validated; only enabled sections will be applied.",
                Style::default().fg(theme::GREEN),
            ));
        } else {
            lines.extend(manager.results().iter().map(|result| {
                Line::styled(
                    result,
                    if result.starts_with("FAIL") {
                        Style::default().fg(theme::RED)
                    } else if result.starts_with("SKIP") {
                        theme::dim()
                    } else {
                        Style::default().fg(theme::GREEN)
                    },
                )
            }));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("tab", "next"),
            theme::chip("◂▸/space", "choice"),
            theme::chip("v", "preview"),
            theme::chip("ctrl-s/d", "stage/remove entry"),
            theme::chip("i/o", "import/export"),
            theme::chip("a", "apply"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("bootstrap and preseed")),
        popup,
    );
    if let Some((import, path)) = manager.transfer() {
        let transfer = centered_rect(70, 24, area);
        frame.render_widget(Clear, transfer);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    if import {
                        "Import typed preseed"
                    } else {
                        "Export typed preseed"
                    },
                    theme::title(),
                ),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Path  ", theme::label()),
                    Span::styled(path, theme::text()),
                ]),
                Line::raw(""),
                Line::styled(
                    "Files are parsed into structured fields, never opened as a raw config editor.",
                    theme::dim(),
                ),
            ])
            .block(theme::panel("typed preseed file")),
            transfer,
        );
    }
    if let Some(input) = manager.apply_input() {
        render_typed_confirmation(
            frame,
            area,
            "Apply typed preseed",
            "apply typed preseed",
            input,
        );
    }
}

fn render_pool_safety_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::PoolSafetyManager,
) {
    use super::app::PoolSafetyTab;
    let popup = centered_rect(78, 70, area);
    frame.render_widget(Clear, popup);
    let selected = |tab| {
        if manager.tab() == tab {
            Style::default()
                .fg(theme::SELECTED_TAB_TEXT)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::subtle()
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(" growth ", selected(PoolSafetyTab::Growth)),
            Span::styled(" · ", theme::dim()),
            Span::styled(" driver health ", selected(PoolSafetyTab::Health)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Pool  ", theme::label()),
            Span::styled(manager.name(), theme::title()),
            Span::styled("   Driver  ", theme::label()),
            Span::styled(manager.driver(), theme::text()),
        ]),
        Line::raw(""),
    ];
    match manager.tab() {
        PoolSafetyTab::Growth => {
            lines.extend([
                Line::from(vec![Span::styled("Source       ", theme::label()), Span::styled(manager.source(), theme::text())]),
                Line::from(vec![Span::styled("Current size ", theme::label()), Span::styled(manager.old_size(), theme::text())]),
                Line::from(vec![Span::styled("New size     ", theme::label()), Span::styled(manager.new_size(), Style::default().bg(theme::SURFACE).fg(theme::TEXT))]),
                Line::raw(""),
                Line::styled("Growth is one-way. The new size must parse and exceed the configured loop size.", Style::default().fg(theme::YELLOW)),
            ]);
        }
        PoolSafetyTab::Health => {
            lines.extend(manager.health_rows().into_iter().map(|(label, value)| {
                Line::from(vec![
                    Span::styled(format!("{label:<24}"), theme::label()),
                    Span::styled(value, theme::text()),
                ])
            }));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("↹/◂▸", "section"),
            theme::chip("g", "validate + grow"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("storage pool safety and health")),
        popup,
    );
    if let Some(input) = manager.confirmation() {
        let expected = format!(
            "grow storage pool {} to {}",
            manager.name(),
            manager.new_size()
        );
        render_typed_confirmation(
            frame,
            area,
            "Grow loop-backed storage pool",
            &expected,
            input,
        );
    }
}

fn render_qmp_manager(frame: &mut Frame, area: Rect, manager: &super::app::QmpManager) {
    let popup = centered_rect(84, 78, area);
    frame.render_widget(Clear, popup);
    let focus = |index| {
        if manager.field() == index {
            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
        } else {
            theme::text()
        }
    };
    let mut lines = vec![
        Line::styled(
            format!("Structured QMP hooks · {}", manager.name()),
            theme::title(),
        ),
        Line::styled(
            "Danger: commands execute inside QEMU during VM startup. Only virtty's explicit allow-list is available.",
            Style::default().fg(theme::RED),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Stage           ", theme::label()),
            Span::styled(format!("‹ {} ›", manager.stage()), focus(0)),
        ]),
        Line::from(vec![
            Span::styled("Command         ", theme::label()),
            Span::styled(format!("‹ {} ›", manager.command()), focus(1)),
        ]),
        Line::from(vec![
            Span::styled("Argument name   ", theme::label()),
            Span::styled(manager.argument_name(), focus(2)),
        ]),
        Line::from(vec![
            Span::styled("Argument value  ", theme::label()),
            Span::styled(manager.argument_value(), focus(3)),
        ]),
        Line::raw(""),
        Line::styled(format!("Commands in {}", manager.stage()), theme::heading()),
    ];
    if manager.commands().is_empty() {
        lines.push(Line::styled(
            "No commands configured for this stage",
            theme::dim(),
        ));
    } else {
        lines.extend(
            manager
                .commands()
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    Line::styled(
                        format!(
                            "{} {}",
                            if index == manager.selected() {
                                "▌"
                            } else {
                                " "
                            },
                            command
                        ),
                        if index == manager.selected() {
                            theme::selected_row()
                        } else {
                            theme::subtle()
                        },
                    )
                }),
        );
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("tab", "field"),
            theme::chip("◂▸", "choice"),
            theme::chip("ctrl-a", "add"),
            theme::chip("ctrl-d", "remove"),
            theme::chip("ctrl-s", "confirm apply"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("QMP command policy")),
        popup,
    );
    if let Some(input) = manager.confirmation() {
        let expected = format!(
            "apply QMP stage {} to VM {}",
            manager.stage(),
            manager.name()
        );
        render_typed_confirmation(
            frame,
            area,
            "Apply dangerous QMP startup hooks",
            &expected,
            input,
        );
    }
}

fn render_bulk_manager(frame: &mut Frame, area: Rect, manager: &super::app::BulkManager) {
    let popup = centered_rect(88, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Bulk instance control", theme::title()),
        Line::from(vec![
            Span::styled("Action  ", theme::label()),
            Span::styled(format!("‹ {} ›", manager.action().as_str()), theme::text()),
            Span::raw("    "),
            Span::styled("Concurrency  ", theme::label()),
            Span::raw(manager.concurrency().to_string()),
            Span::raw("    "),
            Span::styled("Mode  ", theme::label()),
            Span::styled(
                if manager.dry_run() {
                    "DRY RUN"
                } else {
                    "APPLY"
                },
                if manager.dry_run() {
                    Style::default().fg(theme::YELLOW)
                } else {
                    Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)
                },
            ),
        ]),
        Line::styled(
            format!(
                "Selected {} · eligible {}",
                manager.selected_count(),
                manager.eligible_count()
            ),
            theme::subtle(),
        ),
        Line::styled(
            if manager.running() {
                format!(
                    "RUNNING · {}/{} completed · press c to cancel undispatched targets",
                    manager.completed(),
                    manager.total()
                )
            } else if manager.cancelled() {
                format!(
                    "CANCELLED · {}/{} completed",
                    manager.completed(),
                    manager.total()
                )
            } else {
                format!("Progress · {}/{}", manager.completed(), manager.total())
            },
            if manager.running() {
                Style::default().fg(theme::YELLOW)
            } else {
                theme::subtle()
            },
        ),
        Line::raw(""),
    ];
    lines.extend(manager.targets().iter().enumerate().map(|(index, target)| {
        let marker = if target.selected() { "[x]" } else { "[ ]" };
        Line::styled(
            format!(
                "{} {:<20} {:<16} {}",
                marker,
                target.name(),
                target.project(),
                target.status()
            ),
            if index == manager.selected() {
                theme::selected_row()
            } else {
                theme::text()
            },
        )
    }));
    if !manager.results().is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Results", theme::heading()));
        lines.extend(manager.results().iter().take(8).map(|result| {
            let style = if result.starts_with("FAIL") {
                Style::default().fg(theme::RED)
            } else if result.starts_with("SKIP") {
                theme::dim()
            } else {
                Style::default().fg(theme::GREEN)
            };
            Line::styled(result, style)
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("space", "select"),
            theme::chip("A", "all"),
            theme::chip("◂▸", "action"),
            theme::chip("+/-", "parallel"),
            theme::chip("d", "dry-run"),
            theme::chip("r", "retry failures"),
            theme::chip("c", "cancel running"),
            theme::chip("enter", "run"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("bulk control")),
        popup,
    );
    if let Some(input) = manager.confirmation() {
        let expected = format!(
            "apply bulk {} to {} instances",
            manager.action().as_str(),
            manager.eligible_count()
        );
        render_typed_confirmation(frame, area, "Apply bulk instance action", &expected, input);
    }
}

fn render_device_manager(frame: &mut Frame, area: Rect, manager: &super::app::DeviceManager) {
    let popup = centered_rect(88, 82, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{}  ", manager.kind().label()), theme::label()),
            Span::styled(manager.name(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(
                if manager.project().is_empty() {
                    "default"
                } else {
                    manager.project()
                },
                theme::text(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  name                  ", theme::table_header()),
            Span::styled("type          ", theme::table_header()),
            Span::styled("attachment", theme::table_header()),
        ]),
    ];
    if manager.devices().is_empty() {
        lines.push(Line::styled("No local devices", theme::dim()));
    } else {
        lines.extend(
            manager
                .devices()
                .iter()
                .enumerate()
                .map(|(index, (name, config))| {
                    let attachment = config
                        .get("network")
                        .or_else(|| config.get("pool"))
                        .or_else(|| config.get("source"))
                        .or_else(|| config.get("parent"))
                        .cloned()
                        .unwrap_or_default();
                    Line::from(vec![
                        Span::styled(
                            if index == manager.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{name:<22}"), theme::text()),
                        Span::styled(
                            format!(
                                "{:<14}",
                                config.get("type").map(String::as_str).unwrap_or("-")
                            ),
                            theme::subtle(),
                        ),
                        Span::styled(attachment, theme::dim()),
                    ])
                    .style(if index == manager.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }),
        );
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("n", "new"),
            theme::chip("e/↵", "edit"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel(format!("{} devices", manager.kind().label()))),
        popup,
    );

    if manager.editing() {
        let editor_popup = centered_rect(72, 72, area);
        frame.render_widget(Clear, editor_popup);
        let mut fields = vec![Line::styled("Configure device", theme::title())];
        fields.push(Line::styled(
            "Tab changes field; Space/Left/Right cycles choices; Enter on the last field saves.",
            theme::dim(),
        ));
        fields.push(Line::raw(""));
        if let Some(editor_fields) = manager.editor_fields() {
            for (label, value, choice, focused) in editor_fields {
                fields.push(Line::from(vec![
                    Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                    Span::styled(format!("{label:<16}"), theme::label()),
                    Span::styled(
                        value,
                        if focused {
                            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                        } else {
                            theme::subtle()
                        },
                    ),
                    Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
                ]));
            }
        }
        frame.render_widget(
            Paragraph::new(fields).block(
                theme::panel("device form").border_style(Style::default().fg(theme::ACCENT)),
            ),
            editor_popup,
        );
    }

    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(68, 24, area);
        frame.render_widget(Clear, confirm);
        let expected = manager.delete_phrase().unwrap_or_default();
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete device", Style::default().fg(theme::RED)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type ", theme::dim()),
                    Span::styled(&expected, theme::label()),
                ]),
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(theme::RED)),
                    Span::styled(input, theme::text()),
                ]),
            ])
            .block(
                theme::panel("confirm device deletion")
                    .border_style(Style::default().fg(theme::RED)),
            ),
            confirm,
        );
    }
}

fn render_volume_manager(frame: &mut Frame, area: Rect, manager: &super::app::VolumeManager) {
    let popup = centered_rect(86, 78, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Pool  ", theme::label()),
            Span::styled(manager.pool(), theme::title()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  name                  ", theme::table_header()),
            Span::styled("project        ", theme::table_header()),
            Span::styled("content       ", theme::table_header()),
            Span::styled("size        ", theme::table_header()),
            Span::styled("used by", theme::table_header()),
        ]),
    ];
    if manager.volumes().is_empty() {
        lines.push(Line::styled("No custom volumes", theme::dim()));
    } else {
        lines.extend(manager.volumes().iter().enumerate().map(|(index, volume)| {
            Line::from(vec![
                Span::styled(
                    if index == manager.selected() {
                        "▌ "
                    } else {
                        "  "
                    },
                    theme::label(),
                ),
                Span::styled(format!("{:<22}", volume.name), theme::text()),
                Span::styled(format!("{:<15}", volume.project), theme::subtle()),
                Span::styled(format!("{:<14}", volume.content_type), theme::subtle()),
                Span::styled(
                    format!(
                        "{:<12}",
                        volume.config.get("size").map(String::as_str).unwrap_or("-")
                    ),
                    theme::text(),
                ),
                Span::styled(volume.used_by.len().to_string(), theme::dim()),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                Style::default()
            })
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("n", "new volume"),
            theme::chip("i", "import backup/ISO"),
            theme::chip("e/enter", "edit / resize"),
            theme::chip("a", "attach"),
            theme::chip("t", "detach"),
            theme::chip("A/T", "profile attach/detach"),
            theme::chip("s", "snapshots"),
            theme::chip("B", "backups"),
            theme::chip("D", "dirty bitmaps"),
            theme::chip("S", "live state"),
            theme::chip("R", "rebuild empty"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("custom storage volumes")),
        popup,
    );

    if let Some(form) = manager.bitmaps() {
        render_volume_bitmaps(frame, area, form);
    }

    if let Some((title, content)) = manager.detail() {
        let detail = centered_rect(88, 72, area);
        frame.render_widget(Clear, detail);
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .block(theme::panel(format!("{title} · esc close"))),
            detail,
        );
    }

    if let Some(fields) = manager.create_fields() {
        let create = centered_rect(70, 52, area);
        frame.render_widget(Clear, create);
        let mut lines = vec![
            Line::styled("Create custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(
                    if matches!(label, "Content type" | "Shared block volume") {
                        "  ◂ ▸"
                    } else {
                        ""
                    },
                    theme::dim(),
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab changes field; Space/Left/Right changes content type; final Enter creates.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines)
                .block(theme::panel("new volume").border_style(Style::default().fg(theme::ACCENT))),
            create,
        );
    }

    if let Some(fields) = manager.import_fields() {
        let popup = centered_rect(72, 42, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::styled("Import custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("Backup archives recover volumes and snapshots; ISO import creates a read-only custom ISO volume.", theme::muted()));
        lines.push(Line::styled(
            "Tab changes field · Space changes type · final Enter imports",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("volume recovery/import")),
            popup,
        );
    }

    if let Some(fields) = manager.copy_fields() {
        let popup = centered_rect(72, 44, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::styled("Copy custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<18}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("Target pool is a live dropdown. Refresh updates an existing target; volume-only omits snapshots.", theme::muted()));
        lines.push(Line::styled(
            "Tab fields · Space/Left/Right choices · final Enter copies",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("volume copy/move")),
            popup,
        );
    }

    if let Some(fields) = manager.protocol_fields() {
        let popup = centered_rect(68, 34, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::styled("Custom-volume protocol proxy", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<14}"), theme::label()),
                Span::styled(value, theme::text()),
                Span::styled(if choice { " ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Port 0 chooses a free loopback port. Enter starts a one-client bridge.",
            theme::muted(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("SFTP / NBD")),
            popup,
        );
    }
    if let Some(address) = manager.proxy_address() {
        let popup = centered_rect(68, 24, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Proxy ready", theme::title()),
                Line::raw(""),
                Line::styled(address, Style::default().fg(theme::SUCCESS)),
                Line::raw(""),
                Line::styled("Connect one client now · Enter/Esc close", theme::muted()),
            ])
            .block(theme::panel("volume protocol access")),
            popup,
        );
    }
    if let Some(form) = manager.file_form() {
        render_volume_file_form(frame, area, form);
    }

    if let Some(fields) = manager.edit_fields() {
        let edit = centered_rect(70, 34, area);
        frame.render_widget(Clear, edit);
        let mut lines = vec![
            Line::styled("Edit custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab changes field; final Enter saves. Size accepts values such as 20GiB.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("edit volume").border_style(Style::default().fg(theme::ACCENT)),
            ),
            edit,
        );
    }

    if let Some(fields) = manager.attach_fields() {
        let attach = centered_rect(72, 40, area);
        frame.render_widget(Clear, attach);
        let mut lines = vec![
            Line::styled("Attach custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(
                    if matches!(label, "Instance" | "Profile") {
                        "  ◂ ▸"
                    } else {
                        ""
                    },
                    theme::dim(),
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Choose an instance in the same project, then enter a device name and guest mount path.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("attach volume").border_style(Style::default().fg(theme::ACCENT)),
            ),
            attach,
        );
    }

    if let Some(fields) = manager.detach_fields() {
        let detach = centered_rect(70, 34, area);
        frame.render_widget(Clear, detach);
        let mut lines = vec![
            Line::styled("Detach custom volume", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(
                    if matches!(label, "Instance" | "Profile") {
                        "  ◂ ▸"
                    } else {
                        ""
                    },
                    theme::dim(),
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "The device is removed only if its pool and source match this volume.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("detach volume").border_style(Style::default().fg(theme::YELLOW)),
            ),
            detach,
        );
    }

    if let Some(form) = manager.snapshots_form() {
        let snapshots = centered_rect(78, 66, area);
        frame.render_widget(Clear, snapshots);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Volume  ", theme::label()),
                Span::styled(&form.volume().name, theme::title()),
            ]),
            Line::raw(""),
        ];
        if form.snapshots().is_empty() {
            lines.push(Line::styled("No volume snapshots", theme::dim()));
        } else {
            lines.extend(form.snapshots().iter().enumerate().map(|(index, item)| {
                Line::from(vec![
                    Span::styled(
                        if index == form.selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(format!("{:<28}", item.name), theme::text()),
                    Span::styled(&item.created_at, theme::dim()),
                ])
                .style(if index == form.selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("n", "new"),
                theme::chip("R", "rename"),
                theme::chip("r", "restore"),
                theme::chip("d", "delete"),
                theme::chip("esc", "close"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("volume snapshots")),
            snapshots,
        );

        if let Some((rename, input)) = form.name_input() {
            let popup = centered_rect(58, 20, area);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        if rename {
                            "Rename volume snapshot"
                        } else {
                            "Create volume snapshot"
                        },
                        theme::title(),
                    ),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Name  ", theme::label()),
                        Span::styled(input, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
                    ]),
                ])
                .block(theme::panel("snapshot name")),
                popup,
            );
        }
        if let Some((restore, input)) = form.confirmation() {
            let popup = centered_rect(68, 24, area);
            frame.render_widget(Clear, popup);
            let expected = form.phrase(restore);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        if restore {
                            "Restore volume snapshot"
                        } else {
                            "Delete volume snapshot"
                        },
                        Style::default().fg(if restore { theme::YELLOW } else { theme::RED }),
                    ),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Type ", theme::dim()),
                        Span::styled(expected, theme::label()),
                    ]),
                    Line::from(vec![
                        Span::styled("> ", theme::label()),
                        Span::styled(input, theme::text()),
                    ]),
                ])
                .block(theme::panel("confirm snapshot action")),
                popup,
            );
        }
    }

    if let Some(form) = manager.backups_form() {
        let popup = centered_rect(78, 66, area);
        frame.render_widget(Clear, popup);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Volume  ", theme::label()),
                Span::styled(&form.volume().name, theme::title()),
            ]),
            Line::raw(""),
        ];
        if form.backups().is_empty() {
            lines.push(Line::styled("No volume backups", theme::dim()));
        } else {
            lines.extend(form.backups().iter().enumerate().map(|(index, backup)| {
                Line::from(vec![
                    Span::styled(
                        if index == form.selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(format!("{:<30}", backup.name), theme::text()),
                    Span::styled(&backup.created_at, theme::dim()),
                ])
                .style(if index == form.selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("n", "new"),
                theme::chip("r", "rename"),
                theme::chip("o", "export"),
                theme::chip("d", "delete"),
                theme::chip("esc", "close"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("volume backups")),
            popup,
        );

        if let Some((rename, input)) = form.name_input() {
            let create = centered_rect(66, 28, area);
            frame.render_widget(Clear, create);
            let (volume_only, optimized) = form.create_options();
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        if rename {
                            "Rename volume backup"
                        } else {
                            "Create volume backup"
                        },
                        theme::title(),
                    ),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Name  ", theme::label()),
                        Span::styled(input, theme::text()),
                    ]),
                    Line::raw(""),
                    Line::styled(
                        format!(
                            "Ctrl-V volume only: {}",
                            if volume_only { "yes" } else { "no" }
                        ),
                        theme::subtle(),
                    ),
                    Line::styled(
                        format!(
                            "Ctrl-O optimized storage: {}",
                            if optimized { "yes" } else { "no" }
                        ),
                        theme::subtle(),
                    ),
                    Line::styled("Enter submits · Esc cancels", theme::dim()),
                ])
                .block(theme::panel("backup form")),
                create,
            );
        }
        if let Some(path) = form.export_path() {
            let export = centered_rect(66, 22, area);
            frame.render_widget(Clear, export);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Export volume backup", theme::title()),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Local path  ", theme::label()),
                        Span::styled(path, theme::text()),
                    ]),
                    Line::styled("Enter downloads · Esc cancels", theme::dim()),
                ])
                .block(theme::panel("backup export")),
                export,
            );
        }
        if let Some(input) = form.delete_input() {
            let confirm = centered_rect(70, 24, area);
            frame.render_widget(Clear, confirm);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Delete volume backup", Style::default().fg(theme::RED)),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Type ", theme::dim()),
                        Span::styled(form.delete_phrase(), theme::label()),
                    ]),
                    Line::from(vec![
                        Span::styled("> ", theme::label()),
                        Span::styled(input, theme::text()),
                    ]),
                ])
                .block(theme::panel("confirm backup deletion")),
                confirm,
            );
        }
    }

    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(70, 24, area);
        frame.render_widget(Clear, confirm);
        let expected = manager.delete_phrase().unwrap_or_default();
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete custom volume", Style::default().fg(theme::RED)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type ", theme::dim()),
                    Span::styled(&expected, theme::label()),
                ]),
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(theme::RED)),
                    Span::styled(input, theme::text()),
                ]),
            ])
            .block(
                theme::panel("confirm volume deletion")
                    .border_style(Style::default().fg(theme::RED)),
            ),
            confirm,
        );
    }
    if let Some(input) = manager.rebuild_input() {
        let confirm = centered_rect(74, 30, area);
        frame.render_widget(Clear, confirm);
        let expected = manager.rebuild_phrase().unwrap_or_default();
        frame.render_widget(Paragraph::new(vec![
            Line::styled("Rebuild custom volume", Style::default().fg(theme::RED)),
            Line::styled("This destroys the volume contents and recreates it empty with the same configuration.", theme::muted()),
            Line::raw(""),
            Line::from(vec![Span::styled("Type ", theme::dim()), Span::styled(&expected, theme::label())]),
            Line::from(vec![Span::styled("> ", Style::default().fg(theme::RED)), Span::styled(input, theme::text())]),
        ]).block(theme::panel("confirm destructive rebuild").border_style(Style::default().fg(theme::RED))), confirm);
    }
}

fn render_volume_bitmaps(frame: &mut Frame, area: Rect, form: &super::app::VolumeBitmapsForm) {
    let popup = centered_rect(78, 68, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled(
            format!("Dirty bitmaps · {}", form.volume().name),
            theme::title(),
        ),
        Line::styled(
            "Dirty-byte tracking for incremental VM/NBD backup workflows.",
            theme::subtle(),
        ),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "  name                dirty bytes   granularity  flags",
            theme::table_header(),
        )]),
    ];
    if form.bitmaps().is_empty() {
        lines.push(Line::styled("No dirty bitmaps", theme::dim()));
    } else {
        lines.extend(form.bitmaps().iter().enumerate().map(|(index, bitmap)| {
            let flags = [
                bitmap.persistent.then_some("persistent"),
                bitmap.recording.then_some("recording"),
                bitmap.busy.then_some("busy"),
                bitmap.inconsistent.then_some("INCONSISTENT"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(",");
            Line::styled(
                format!(
                    "{} {:<20} {:<13} {:<12} {}",
                    if index == form.selected() { "▌" } else { " " },
                    bitmap.name,
                    bitmap.count,
                    bitmap.granularity,
                    flags
                ),
                if index == form.selected() {
                    theme::selected_row()
                } else {
                    theme::text()
                },
            )
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("n", "new"),
            theme::chip("d", "delete"),
            theme::chip("r", "refresh"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("volume dirty bitmaps")),
        popup,
    );
    if let Some(fields) = form.create_fields() {
        let create = centered_rect(62, 38, area);
        frame.render_widget(Clear, create);
        let mut lines = vec![
            Line::styled("Create dirty bitmap", theme::title()),
            Line::raw(""),
        ];
        lines.extend(fields.into_iter().map(|(label, value, choice, focused)| {
            Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<20}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::text()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ])
        }));
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Granularity must be a positive power of two · final Enter creates",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("new bitmap")),
            create,
        );
    }
    if let Some(input) = form.delete_input() {
        render_typed_confirmation(
            frame,
            area,
            "Delete dirty bitmap",
            &form.delete_phrase(),
            input,
        );
    }
}

fn render_volume_file_form(frame: &mut Frame, area: Rect, form: &super::app::VolumeFileForm) {
    let popup = centered_rect(86, 78, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Volume  ", theme::label()),
            Span::styled(&form.volume().name, theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(&form.volume().project, theme::text()),
        ]),
        Line::raw(""),
        form_line("Path", form.path(), !form.browsing()),
        Line::raw(""),
    ];
    if form.browsing() {
        lines.push(Line::styled("Directory entries", theme::heading()));
        for (index, entry) in form.entries().iter().enumerate() {
            lines.push(Line::styled(
                format!(
                    "{} {entry}",
                    if index == form.selected() { "▌" } else { " " }
                ),
                if index == form.selected() {
                    theme::selected_row()
                } else {
                    theme::text()
                },
            ));
        }
    } else {
        lines.push(Line::styled("File content", theme::heading()));
        lines.push(Line::raw(form.contents()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Tab path/content · Ctrl-B browse · Ctrl-R read · Ctrl-S save · Ctrl-P pull · Ctrl-U push · Ctrl-D delete", theme::muted()));
    if let Some((upload, local)) = form.transfer() {
        lines.push(Line::raw(""));
        lines.push(form_line(
            if upload { "Upload from" } else { "Download to" },
            local,
            true,
        ));
    }
    if let Some(input) = form.delete_input() {
        lines.push(Line::styled(
            format!("Type exactly: delete volume file {}", form.path()),
            Style::default().fg(theme::DANGER),
        ));
        lines.push(Line::styled(input, theme::text()));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(theme::panel("custom-volume files")),
        popup,
    );
}

fn render_bucket_manager(frame: &mut Frame, area: Rect, manager: &super::app::BucketManager) {
    let popup = centered_rect(86, 76, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Pool  ", theme::label()),
            Span::styled(manager.pool(), theme::title()),
        ]),
        Line::raw(""),
    ];
    if manager.buckets().is_empty() {
        lines.push(Line::styled("No storage buckets", theme::dim()));
    } else {
        lines.extend(manager.buckets().iter().enumerate().map(|(index, b)| {
            Line::from(vec![
                Span::styled(
                    if index == manager.selected() {
                        "▌ "
                    } else {
                        "  "
                    },
                    theme::label(),
                ),
                Span::styled(format!("{:<24}", b.name), theme::text()),
                Span::styled(format!("{:<16}", b.project), theme::subtle()),
                Span::styled(
                    b.config.get("size").map(String::as_str).unwrap_or("-"),
                    theme::dim(),
                ),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                Style::default()
            })
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("n", "new"),
            theme::chip("i", "import backup"),
            theme::chip("e/enter", "edit"),
            theme::chip("K", "credentials"),
            theme::chip("B", "backups"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("storage buckets")),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let edit = centered_rect(68, 44, area);
        frame.render_widget(Clear, edit);
        let mut lines = vec![
            Line::styled(
                if manager.editor_is_editing() {
                    "Edit bucket"
                } else {
                    "Create bucket"
                },
                theme::title(),
            ),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("bucket form").border_style(Style::default().fg(theme::ACCENT)),
            ),
            edit,
        );
    }
    if let Some(fields) = manager.import_fields() {
        let import = centered_rect(68, 34, area);
        frame.render_widget(Clear, import);
        let mut lines = vec![
            Line::styled("Import bucket backup", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab changes field · final Enter restores the bucket",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("bucket recovery/import")),
            import,
        );
    }
    if let Some(keys) = manager.keys() {
        let key_popup = centered_rect(76, 64, area);
        frame.render_widget(Clear, key_popup);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Bucket  ", theme::label()),
                Span::styled(&keys.bucket().name, theme::title()),
            ]),
            Line::raw(""),
        ];
        if keys.keys().is_empty() {
            lines.push(Line::styled("No access keys", theme::dim()));
        } else {
            lines.extend(keys.keys().iter().enumerate().map(|(index, key)| {
                Line::from(vec![
                    Span::styled(
                        if index == keys.selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(format!("{:<24}", key.name), theme::text()),
                    Span::styled(&key.role, theme::subtle()),
                ])
                .style(if index == keys.selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("n", "new key"),
                theme::chip("e", "edit"),
                theme::chip("d", "delete"),
                theme::chip("esc", "back"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("bucket credentials")),
            key_popup,
        );
        if let Some(fields) = keys.editor_fields() {
            let edit = centered_rect(64, 36, area);
            frame.render_widget(Clear, edit);
            let mut lines = vec![
                Line::styled(
                    if keys.editor_is_editing() {
                        "Edit bucket key"
                    } else {
                        "Create bucket key"
                    },
                    theme::title(),
                ),
                Line::raw(""),
            ];
            for (label, value, focused) in fields {
                lines.push(Line::from(vec![
                    Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                    Span::styled(format!("{label:<14}"), theme::label()),
                    Span::styled(
                        value,
                        if focused {
                            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                        } else {
                            theme::subtle()
                        },
                    ),
                    Span::styled(if label == "Role" { " ◂ ▸" } else { "" }, theme::dim()),
                ]));
            }
            frame.render_widget(
                Paragraph::new(lines).block(theme::panel(if keys.editor_is_editing() {
                    "edit access key"
                } else {
                    "new access key"
                })),
                edit,
            );
        }
        if let Some(credentials) = keys.credentials() {
            let reveal = centered_rect(74, 30, area);
            frame.render_widget(Clear, reveal);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Save these credentials now", theme::title()),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Access key  ", theme::label()),
                        Span::styled(&credentials.access_key, theme::text()),
                    ]),
                    Line::from(vec![
                        Span::styled("Secret key  ", theme::label()),
                        Span::styled(&credentials.secret_key, theme::text()),
                    ]),
                    Line::raw(""),
                    Line::styled(
                        "The secret may not be shown again. Enter/Esc closes.",
                        theme::dim(),
                    ),
                ])
                .block(
                    theme::panel("new credentials")
                        .border_style(Style::default().fg(theme::YELLOW)),
                ),
                reveal,
            );
        }
        if let Some(input) = keys.delete_input() {
            render_typed_confirmation(
                frame,
                area,
                "Delete bucket key",
                &keys.delete_phrase(),
                input,
            );
        }
    }
    if let Some(form) = manager.backups() {
        let backup_popup = centered_rect(78, 64, area);
        frame.render_widget(Clear, backup_popup);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Bucket  ", theme::label()),
                Span::styled(&form.bucket().name, theme::title()),
            ]),
            Line::raw(""),
        ];
        if form.backups().is_empty() {
            lines.push(Line::styled("No bucket backups", theme::dim()));
        } else {
            lines.extend(form.backups().iter().enumerate().map(|(index, backup)| {
                Line::from(vec![
                    Span::styled(
                        if index == form.selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(format!("{:<30}", backup.name), theme::text()),
                    Span::styled(&backup.created_at, theme::dim()),
                ])
                .style(if index == form.selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("n", "new"),
                theme::chip("r", "rename"),
                theme::chip("o", "export"),
                theme::chip("d", "delete"),
                theme::chip("esc", "back"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("bucket backups")),
            backup_popup,
        );
        if let Some((rename, input)) = form.name_input() {
            let popup = centered_rect(64, 26, area);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        if rename {
                            "Rename bucket backup"
                        } else {
                            "Create bucket backup"
                        },
                        theme::title(),
                    ),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Name  ", theme::label()),
                        Span::styled(input, theme::text()),
                    ]),
                    Line::styled(
                        format!("Compression  {}  ◂ ▸", form.compression()),
                        theme::subtle(),
                    ),
                    Line::styled("Space changes compression · Enter submits", theme::dim()),
                ])
                .block(theme::panel("bucket backup form")),
                popup,
            );
        }
        if let Some(path) = form.export_path() {
            let popup = centered_rect(66, 20, area);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Export bucket backup", theme::title()),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Local path  ", theme::label()),
                        Span::styled(path, theme::text()),
                    ]),
                ])
                .block(theme::panel("backup export")),
                popup,
            );
        }
        if let Some(input) = form.delete_input() {
            render_typed_confirmation(
                frame,
                area,
                "Delete bucket backup",
                &form.delete_phrase(),
                input,
            );
        }
    }
    if let Some(input) = manager.delete_input() {
        render_typed_confirmation(
            frame,
            area,
            "Delete storage bucket",
            &manager.delete_phrase(),
            input,
        );
    }
}

fn render_typed_confirmation(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    expected: &str,
    input: &str,
) {
    let popup = centered_rect(68, 24, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(title, Style::default().fg(theme::RED)),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Type ", theme::dim()),
                Span::styled(expected, theme::label()),
            ]),
            Line::from(vec![
                Span::styled("> ", theme::label()),
                Span::styled(input, theme::text()),
            ]),
        ])
        .block(
            theme::panel("confirm destructive action")
                .border_style(Style::default().fg(theme::RED)),
        ),
        popup,
    );
}

fn render_address_set_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::AddressSetManager,
) {
    let popup = centered_rect(82, 72, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Network address sets", theme::title()),
        Line::raw(""),
    ];
    if manager.sets().is_empty() {
        lines.push(Line::styled("No address sets", theme::dim()));
    } else {
        lines.extend(manager.sets().iter().enumerate().map(|(index, set)| {
            Line::from(vec![
                Span::styled(
                    if index == manager.selected() {
                        "▌ "
                    } else {
                        "  "
                    },
                    theme::label(),
                ),
                Span::styled(format!("{:<24}", set.name), theme::text()),
                Span::styled(format!("{:<14}", set.project), theme::subtle()),
                Span::styled(set.addresses.join(", "), theme::dim()),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                Style::default()
            })
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("n", "new"),
            theme::chip("e/enter", "edit"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("address sets")),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let form = centered_rect(72, 44, area);
        frame.render_widget(Clear, form);
        let mut lines = vec![
            Line::styled(
                if manager.editing() {
                    "Edit address set"
                } else {
                    "Create address set"
                },
                theme::title(),
            ),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<16}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Addresses accepts comma-separated IP addresses and CIDRs.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("address-set form").border_style(Style::default().fg(theme::ACCENT)),
            ),
            form,
        );
    }
    if let Some(input) = manager.delete_input() {
        render_typed_confirmation(
            frame,
            area,
            "Delete address set",
            &manager.delete_phrase(),
            input,
        );
    }
    if let Some(input) = manager.rename_input() {
        render_rename_resource(frame, area, "Rename address set", input);
    }
}

fn render_integration_manager(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::IntegrationManager,
) {
    let popup = centered_rect(84, 72, area);
    frame.render_widget(Clear, popup);
    let rows = manager
        .integrations()
        .iter()
        .enumerate()
        .map(|(index, value)| {
            Row::new([
                value.name.clone(),
                value.integration_type.clone(),
                value.status.clone(),
                value.description.clone(),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                theme::text()
            })
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Min(20),
            ],
        )
        .header(Row::new(["Name", "Type", "Status", "Description"]).style(theme::heading()))
        .block(theme::panel(
            "network integrations · n create · e edit · d delete · Esc close",
        )),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let editor = centered_rect(72, 56, area);
        frame.render_widget(Clear, editor);
        let mut lines = vec![
            Line::styled(
                if manager.editing() {
                    "Edit integration"
                } else {
                    "Create integration"
                },
                theme::title(),
            ),
            Line::raw(""),
        ];
        lines.extend(fields.into_iter().map(|(label, value, choice, focused)| {
            Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<20}"), theme::label()),
                Span::styled(
                    if value.is_empty() { " " } else { value },
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::text()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ])
        }));
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab fields · Enter save · Esc cancel",
            theme::subtle(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("integration form")),
            editor,
        );
    }
    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(68, 28, area);
        frame.render_widget(Clear, confirm);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete network integration", theme::title()),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type  ", theme::label()),
                    Span::styled(manager.delete_phrase(), theme::text()),
                ]),
                Line::from(vec![
                    Span::styled("> ", theme::label()),
                    Span::styled(input, Style::default().bg(theme::SURFACE).fg(theme::TEXT)),
                ]),
            ])
            .block(theme::panel("confirm deletion")),
            confirm,
        );
    }
    if let Some(input) = manager.rename_input() {
        render_rename_resource(frame, area, "Rename integration", input);
    }
}

fn render_bgp_form(frame: &mut Frame, area: Rect, form: &super::app::BgpForm) {
    let popup = centered_rect(78, 76, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Network  ", theme::label()),
            Span::styled(form.network(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(form.project(), theme::text()),
        ]),
        Line::raw(""),
    ];
    for (label, value, toggle, focused) in form.fields() {
        lines.push(Line::from(vec![
            Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
            Span::styled(format!("{label:<27}"), theme::label()),
            Span::styled(
                value,
                if focused {
                    Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                } else {
                    theme::subtle()
                },
            ),
            Span::styled(if toggle { "  ◂ ▸" } else { "" }, theme::dim()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Peer address and interface are alternatives. Final Enter saves with ETag protection.",
        theme::dim(),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme::panel("BGP routing").border_style(Style::default().fg(theme::ACCENT))),
        popup,
    );
}

fn render_zone_manager(frame: &mut Frame, area: Rect, manager: &super::app::ZoneManager) {
    let popup = centered_rect(84, 76, area);
    frame.render_widget(Clear, popup);
    let mut lines = vec![
        Line::styled("Project DNS zones", theme::title()),
        Line::raw(""),
    ];
    if manager.zones().is_empty() {
        lines.push(Line::styled("No network zones", theme::dim()));
    } else {
        lines.extend(manager.zones().iter().enumerate().map(|(index, zone)| {
            Line::from(vec![
                Span::styled(
                    if index == manager.selected() {
                        "▌ "
                    } else {
                        "  "
                    },
                    theme::label(),
                ),
                Span::styled(format!("{:<28}", zone.name), theme::text()),
                Span::styled(format!("{:<16}", zone.project), theme::subtle()),
                Span::styled(&zone.description, theme::dim()),
            ])
            .style(if index == manager.selected() {
                theme::selected_row()
            } else {
                Style::default()
            })
        }));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("enter", "records"),
            theme::chip("n", "new zone"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("network zones")),
        popup,
    );
    if let Some(fields) = manager.editor_fields() {
        let form = centered_rect(68, 36, area);
        frame.render_widget(Clear, form);
        let mut lines = vec![
            Line::styled("Create DNS zone", theme::title()),
            Line::raw(""),
        ];
        for (label, value, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<15}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines).block(theme::panel("zone form")), form);
    }
    if let Some(records) = manager.records() {
        let records_popup = centered_rect(78, 66, area);
        frame.render_widget(Clear, records_popup);
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Zone  ", theme::label()),
                Span::styled(&records.zone().name, theme::title()),
            ]),
            Line::raw(""),
        ];
        if records.records().is_empty() {
            lines.push(Line::styled("No DNS records", theme::dim()));
        } else {
            lines.extend(records.records().iter().enumerate().map(|(index, record)| {
                Line::from(vec![
                    Span::styled(
                        if index == records.selected() {
                            "▌ "
                        } else {
                            "  "
                        },
                        theme::label(),
                    ),
                    Span::styled(format!("{:<24}", record.name), theme::text()),
                    Span::styled(
                        format!("{} entry(s)  ", record.entries.len()),
                        theme::subtle(),
                    ),
                    Span::styled(&record.description, theme::dim()),
                ])
                .style(if index == records.selected() {
                    theme::selected_row()
                } else {
                    Style::default()
                })
            }));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("n", "new record"),
                theme::chip("d", "delete"),
                theme::chip("esc", "back"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("DNS records")),
            records_popup,
        );
        if let Some(fields) = records.editor_fields() {
            let form = centered_rect(70, 48, area);
            frame.render_widget(Clear, form);
            let mut lines = vec![
                Line::styled("Create DNS record", theme::title()),
                Line::raw(""),
            ];
            for (label, value, choice, focused) in fields {
                lines.push(Line::from(vec![
                    Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                    Span::styled(format!("{label:<14}"), theme::label()),
                    Span::styled(
                        value,
                        if focused {
                            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                        } else {
                            theme::subtle()
                        },
                    ),
                    Span::styled(if choice { " ◂ ▸" } else { "" }, theme::dim()),
                ]));
            }
            frame.render_widget(
                Paragraph::new(lines).block(theme::panel("record form")),
                form,
            );
        }
        if let Some(input) = records.delete_input() {
            render_typed_confirmation(
                frame,
                area,
                "Delete DNS record",
                &records.delete_phrase(),
                input,
            );
        }
    }
    if let Some(input) = manager.delete_input() {
        render_typed_confirmation(
            frame,
            area,
            "Delete DNS zone",
            &manager.delete_phrase(),
            input,
        );
    }
    if let Some(input) = manager.rename_input() {
        render_rename_resource(frame, area, "Rename DNS zone", input);
    }
}

fn render_rename_resource(frame: &mut Frame, area: Rect, title: &str, input: &str) {
    let popup = centered_rect(64, 26, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(title, theme::title()),
            Line::raw(""),
            form_line("New name", input, true),
            Line::raw(""),
            Line::styled("Enter rename · Esc cancel", theme::subtle()),
        ])
        .block(theme::panel("rename resource")),
        popup,
    );
}

fn render_network_services(
    frame: &mut Frame,
    area: Rect,
    manager: &super::app::NetworkServicesManager,
) {
    use super::app::NetworkServicesTab;

    let popup = centered_rect(90, 82, area);
    frame.render_widget(Clear, popup);
    let tab_style = |tab| {
        if manager.tab() == tab {
            Style::default()
                .fg(theme::SELECTED_TAB_TEXT)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::subtle()
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Network  ", theme::label()),
            Span::styled(manager.network(), theme::title()),
            Span::styled("   Project  ", theme::label()),
            Span::styled(
                if manager.project().is_empty() {
                    "default"
                } else {
                    manager.project()
                },
                theme::text(),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                format!(" leases ({}) ", manager.leases().len()),
                tab_style(NetworkServicesTab::Leases),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" forwards ({}) ", manager.forwards().len()),
                tab_style(NetworkServicesTab::Forwards),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" ACLs ({}) ", manager.acls().len()),
                tab_style(NetworkServicesTab::Acls),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" load balancers ({}) ", manager.load_balancers().len()),
                tab_style(NetworkServicesTab::LoadBalancers),
            ),
            Span::styled(" · ", theme::dim()),
            Span::styled(
                format!(" peers ({}) ", manager.peers().len()),
                tab_style(NetworkServicesTab::Peers),
            ),
        ]),
        Line::raw(""),
    ];
    match manager.tab() {
        NetworkServicesTab::Leases => {
            if manager.leases().is_empty() {
                lines.push(Line::styled("No leases", theme::dim()));
            } else {
                lines.extend(manager.leases().iter().enumerate().map(|(index, lease)| {
                    Line::from(vec![
                        Span::styled(
                            if index == manager.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{:<28}", lease.address), theme::text()),
                        Span::styled(format!("{:<24}", lease.hostname), theme::subtle()),
                        Span::styled(format!("{:<12}", lease.lease_type), theme::dim()),
                        Span::styled(&lease.hwaddr, theme::dim()),
                    ])
                    .style(if index == manager.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }));
            }
        }
        NetworkServicesTab::Forwards => {
            if manager.forwards().is_empty() {
                lines.push(Line::styled("No forwards", theme::dim()));
            } else {
                lines.extend(
                    manager
                        .forwards()
                        .iter()
                        .enumerate()
                        .map(|(index, forward)| {
                            Line::from(vec![
                                Span::styled(
                                    if index == manager.selected() {
                                        "▌ "
                                    } else {
                                        "  "
                                    },
                                    theme::label(),
                                ),
                                Span::styled(
                                    format!("{:<28}", forward.listen_address),
                                    theme::text(),
                                ),
                                Span::styled(
                                    format!("{} port rule(s)  ", forward.ports.len()),
                                    theme::subtle(),
                                ),
                                Span::styled(&forward.description, theme::dim()),
                            ])
                            .style(if index == manager.selected() {
                                theme::selected_row()
                            } else {
                                Style::default()
                            })
                        }),
                );
            }
        }
        NetworkServicesTab::Acls => {
            if manager.acls().is_empty() {
                lines.push(Line::styled("No network ACLs", theme::dim()));
            } else {
                lines.extend(manager.acls().iter().enumerate().map(|(index, acl)| {
                    Line::from(vec![
                        Span::styled(
                            if index == manager.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{:<24}", acl.name), theme::text()),
                        Span::styled(
                            format!("{} in · {} out  ", acl.ingress.len(), acl.egress.len()),
                            theme::subtle(),
                        ),
                        Span::styled(&acl.description, theme::dim()),
                    ])
                    .style(if index == manager.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }));
            }
        }
        NetworkServicesTab::LoadBalancers => {
            if manager.load_balancers().is_empty() {
                lines.push(Line::styled(
                    "No load balancers (OVN networks only)",
                    theme::dim(),
                ));
            } else {
                lines.extend(
                    manager
                        .load_balancers()
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            Line::from(vec![
                                Span::styled(
                                    if index == manager.selected() {
                                        "▌ "
                                    } else {
                                        "  "
                                    },
                                    theme::label(),
                                ),
                                Span::styled(format!("{:<28}", item.listen_address), theme::text()),
                                Span::styled(
                                    format!(
                                        "{} backend(s) · {} port rule(s)  ",
                                        item.backends.len(),
                                        item.ports.len()
                                    ),
                                    theme::subtle(),
                                ),
                                Span::styled(&item.description, theme::dim()),
                            ])
                            .style(if index == manager.selected() {
                                theme::selected_row()
                            } else {
                                Style::default()
                            })
                        }),
                );
            }
        }
        NetworkServicesTab::Peers => {
            if manager.peers().is_empty() {
                lines.push(Line::styled(
                    "No network peers (OVN networks only)",
                    theme::dim(),
                ));
            } else {
                lines.extend(manager.peers().iter().enumerate().map(|(index, item)| {
                    Line::from(vec![
                        Span::styled(
                            if index == manager.selected() {
                                "▌ "
                            } else {
                                "  "
                            },
                            theme::label(),
                        ),
                        Span::styled(format!("{:<22}", item.name), theme::text()),
                        Span::styled(
                            format!("{}/{}  ", item.target_project, item.target_network),
                            theme::subtle(),
                        ),
                        Span::styled(&item.status, theme::dim()),
                    ])
                    .style(if index == manager.selected() {
                        theme::selected_row()
                    } else {
                        Style::default()
                    })
                }));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(
        [
            theme::chip("↹/◂▸", "section"),
            theme::chip("j/k", "select"),
            theme::chip("n", "new service"),
            theme::chip("L/R", "ACL log/rename"),
            theme::chip("S", "LB state"),
            theme::chip("N", "network state"),
            theme::chip("d", "delete"),
            theme::chip("esc", "close"),
        ]
        .concat(),
    ));
    frame.render_widget(
        Paragraph::new(lines).block(theme::panel("network services")),
        popup,
    );

    if let Some((title, content)) = manager.detail() {
        let detail = centered_rect(88, 72, area);
        frame.render_widget(Clear, detail);
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: false })
                .block(theme::panel(format!("{title} · esc close"))),
            detail,
        );
    }
    if let Some(input) = manager.rename_input() {
        let rename = centered_rect(64, 28, area);
        frame.render_widget(Clear, rename);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Rename selected ACL", theme::title()),
                Line::raw(""),
                form_line("New name", input, true),
                Line::raw(""),
                Line::styled("Enter rename · Esc cancel", theme::subtle()),
            ])
            .block(theme::panel("rename ACL")),
            rename,
        );
    }

    if let Some(rules) = manager.acl_rules() {
        let popup = centered_rect(86, 74, area);
        frame.render_widget(Clear, popup);
        let active = |ingress| {
            if rules.ingress() == ingress {
                Style::default()
                    .fg(theme::SELECTED_TAB_TEXT)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme::subtle()
            }
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled("ACL  ", theme::label()),
                Span::styled(&rules.acl().name, theme::title()),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    format!(" ingress ({}) ", rules.acl().ingress.len()),
                    active(true),
                ),
                Span::styled(" · ", theme::dim()),
                Span::styled(
                    format!(" egress ({}) ", rules.acl().egress.len()),
                    active(false),
                ),
            ]),
            Line::raw(""),
        ];
        if rules.visible_rules().is_empty() {
            lines.push(Line::styled("No rules in this direction", theme::dim()));
        } else {
            lines.extend(
                rules
                    .visible_rules()
                    .iter()
                    .enumerate()
                    .map(|(index, rule)| {
                        Line::from(vec![
                            Span::styled(
                                if index == rules.selected() {
                                    "▌ "
                                } else {
                                    "  "
                                },
                                theme::label(),
                            ),
                            Span::styled(format!("{:<9}", rule.action), theme::text()),
                            Span::styled(format!("{:<9}", rule.protocol), theme::subtle()),
                            Span::styled(
                                format!(
                                    "{} → {}  ",
                                    if rule.source.is_empty() {
                                        "any"
                                    } else {
                                        &rule.source
                                    },
                                    if rule.destination.is_empty() {
                                        "any"
                                    } else {
                                        &rule.destination
                                    }
                                ),
                                theme::text(),
                            ),
                            Span::styled(&rule.description, theme::dim()),
                        ])
                        .style(if index == rules.selected() {
                            theme::selected_row()
                        } else {
                            Style::default()
                        })
                    }),
            );
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(
            [
                theme::chip("↹/◂▸", "direction"),
                theme::chip("a", "add"),
                theme::chip("e/enter", "edit"),
                theme::chip("d", "delete"),
                theme::chip("esc", "back"),
            ]
            .concat(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(theme::panel("ACL rules")),
            popup,
        );
        if let Some(fields) = rules.editor_fields() {
            let edit = centered_rect(76, 68, area);
            frame.render_widget(Clear, edit);
            let mut lines = vec![Line::styled("ACL rule", theme::title()), Line::raw("")];
            for (label, value, choice, focused) in fields {
                lines.push(Line::from(vec![
                    Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                    Span::styled(format!("{label:<18}"), theme::label()),
                    Span::styled(
                        value,
                        if focused {
                            Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                        } else {
                            theme::subtle()
                        },
                    ),
                    Span::styled(if choice { " ◂ ▸" } else { "" }, theme::dim()),
                ]));
            }
            frame.render_widget(
                Paragraph::new(lines).block(
                    theme::panel("structured ACL rule")
                        .border_style(Style::default().fg(theme::ACCENT)),
                ),
                edit,
            );
        }
        if let Some(input) = rules.delete_input() {
            render_typed_confirmation(
                frame,
                area,
                "Delete ACL rule",
                &rules.delete_phrase(),
                input,
            );
        }
    }

    if let Some(fields) = manager.form_fields() {
        let form_popup = centered_rect(72, 58, area);
        frame.render_widget(Clear, form_popup);
        let mut lines = vec![
            Line::styled("Create network service", theme::title()),
            Line::raw(""),
        ];
        for (label, value, choice, focused) in fields {
            lines.push(Line::from(vec![
                Span::styled(if focused { "▌ " } else { "  " }, theme::label()),
                Span::styled(format!("{label:<18}"), theme::label()),
                Span::styled(
                    value,
                    if focused {
                        Style::default().bg(theme::SURFACE).fg(theme::TEXT)
                    } else {
                        theme::subtle()
                    },
                ),
                Span::styled(if choice { "  ◂ ▸" } else { "" }, theme::dim()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Tab changes field; Space/Left/Right changes protocol; final Enter creates.",
            theme::dim(),
        ));
        frame.render_widget(
            Paragraph::new(lines).block(
                theme::panel("new network service")
                    .border_style(Style::default().fg(theme::ACCENT)),
            ),
            form_popup,
        );
    }

    if let Some(input) = manager.delete_input() {
        let confirm = centered_rect(72, 24, area);
        frame.render_widget(Clear, confirm);
        let expected = manager.delete_phrase().unwrap_or_default();
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Delete network service", Style::default().fg(theme::RED)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Type ", theme::dim()),
                    Span::styled(&expected, theme::label()),
                ]),
                Line::from(vec![
                    Span::styled("> ", Style::default().fg(theme::RED)),
                    Span::styled(input, theme::text()),
                ]),
            ])
            .block(
                theme::panel("confirm network deletion")
                    .border_style(Style::default().fg(theme::RED)),
            ),
            confirm,
        );
    }
}

fn output_lines(output: &str) -> Vec<Line<'_>> {
    if output.is_empty() {
        vec![Line::styled("(no output)", theme::muted())]
    } else {
        output.lines().take(16).map(Line::from).collect()
    }
}

fn form_field_name(field: usize) -> &'static str {
    match field {
        0 => "Name",
        1 => "Image",
        2 => "Type",
        3 => "Project",
        4 => "Profiles",
        5 => "CPU limit",
        6 => "Memory limit",
        7 => "Storage pool",
        8 => "Root disk size",
        9 => "Network",
        10 => "Cloud-init data",
        11 => "Target member",
        12 => "Privileged",
        13 => "Nesting",
        14 => "Secure boot",
        15 => "Autostart",
        16 => "Start after create",
        _ => "",
    }
}

fn operation_style(status: &str) -> Style {
    match status {
        "Running" => Style::default()
            .fg(theme::WARNING)
            .add_modifier(Modifier::BOLD),
        "Success" => Style::default().fg(theme::SUCCESS),
        _ => Style::default().fg(theme::DANGER),
    }
}

fn event_style(event_type: &str) -> Style {
    match event_type {
        "lifecycle" => Style::default().fg(theme::ACCENT),
        "operation" => Style::default().fg(theme::WARNING),
        "logging" => Style::default().fg(theme::MUTED),
        _ => Style::default().fg(theme::TEXT),
    }
}

fn format_bytes(bytes: i64) -> String {
    const MIB: i64 = 1024 * 1024;
    const GIB: i64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes > 0 {
        format!("{bytes} B")
    } else {
        "-".to_owned()
    }
}

fn render_error(frame: &mut Frame, area: Rect, error: &str) {
    let popup = centered_rect(78, 24, area);
    frame.render_widget(Clear, popup);
    let message = Paragraph::new(vec![
        Line::from(Span::styled("Unable to load Incus", theme::heading())),
        Line::raw(""),
        Line::from(error),
        Line::raw(""),
        Line::styled("Press r to retry, q to quit.", theme::muted()),
    ])
    .wrap(Wrap { trim: true })
    .block(theme::panel(" Connection error "));
    frame.render_widget(message, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    use crate::tui::App;

    use super::{render, theme};

    #[test]
    fn dashboard_renders_the_connection_shell() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let app = App::new("local:/var/lib/incus/unix.socket".into());

        terminal
            .draw(|frame| render(frame, &app))
            .expect("draws UI");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Dashboard"));
        assert!(rendered.contains("server"));
        assert!(rendered.contains("instance state"));
    }

    #[test]
    fn dashboard_renders_normalized_host_resources() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new("local".into());
        app.model.host_resources = Some(serde_json::json!({
            "cpu": { "total": 24 },
            "memory": { "total": 66571993088_u64, "used": 13526585344_u64 },
            "storage": { "disks": [{}, {}, {}, {}, {}, {}] },
            "network": { "cards": [{}, {}, {}] }
        }));

        terminal
            .draw(|frame| render(frame, &app))
            .expect("draws resources");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("24 thread(s)"));
        assert!(rendered.contains("6 disk(s) · 3 NIC(s)"));
    }

    #[test]
    fn wide_shell_matches_nastty_side_panel_tabs_and_footer() {
        let backend = TestBackend::new(140, 36);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let app = App::new("local".into());
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Dashboard"));
        assert!(rendered.contains("Dashboard / Dashboard"));
        assert!(rendered.contains("server"));
        assert!(rendered.contains("instance state"));
        assert!(buffer.content().iter().any(|cell| cell.fg == theme::ACCENT));
    }

    #[test]
    fn overlays_apply_nastty_dimmed_backdrop() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new("local".into());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let dimmed = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.bg == theme::SURFACE_LO)
            .count();
        assert!(dimmed > 500);
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.bg == theme::COLOR0)
        );
    }
}
