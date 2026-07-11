# virtty

`virtty` is a keyboard-first terminal control plane for local and remote
[Incus](https://linuxcontainers.org/incus/) container and VM hosts. It talks to
the Incus REST API directly: no helper daemon and no `incus` CLI subprocess in
the normal control path.

`virtty` provides structured administration for instances, operations, events,
files, snapshots, backups, images, storage pools and volumes, networks and
services, projects, profiles, trust certificates, warnings, cluster members,
cluster groups, and local/remote connection targets. Configuration is performed
with labelled forms, choices, toggles, and guarded confirmations rather than raw
JSON or YAML editors.

## Requirements

- Incus must be running and the current user must be allowed to access its
  Unix socket (normally through the `incus-admin` group), or a remote endpoint
  must already accept the configured TLS identity.
- The default `images:debian/12` create source pulls from
  `images.linuxcontainers.org`; a different image value is interpreted as a
  local Incus image alias.
- `remote-viewer` is optional and used only for VM VGA-console handoff.

## Run

By default, virtty connects to the local Incus Unix socket:

```sh
make run
```

Use a different socket or a remote HTTPS endpoint when required:

```sh
make run ARGS='--socket /var/lib/incus/unix.socket'
make run ARGS='--remote https://incus.example.net:8443'
make run ARGS='--remote https://incus.example.net:8443 --client-cert ~/.config/incus/client.crt --client-key ~/.config/incus/client.key'
make run ARGS='--remote https://incus.example.net:8443 --client-cert client.crt --client-key client.key --server-ca server-ca.crt'
```

Client certificates are applied to both REST and WebSocket event connections.
`--server-ca` adds a private CA while retaining the standard public root store.
Targets can also be changed at runtime with `c` from the Settings page.

## Keys

- `Tab`, `l`, `Right`: next page
- `Shift-Tab`, `h`, `Left`: previous page
- `j`/`k` or `Up`/`Down`: move the selected row
- `/`: filter the current page; `L` clears the filter
- `x`: open the current page's action menu
- `?`: complete in-application key reference
- `r`: refresh from Incus
- `s`, `X`, `R` on **Instances**: start, stop, restart the selected instance
  after typing the requested confirmation phrase
- `f`, `u`, `d` on **Instances**: freeze, unfreeze, or delete when the current
  instance state permits it
- `n` on **Instances**: open the container/VM creation form
- `e` on a running **Instances** row: run a one-shot `/bin/sh -lc` command and
  capture stdout/stderr (VMs require a working `incus-agent`)
- `I`, `V`, `C` on **Instances**: interactive shell, serial console, or VM VGA
  console; `Ctrl-]` detaches terminal sessions
- `Ctrl-V` and `Ctrl-C` force serial/VGA attachment when `console_force` is
  available; this can disconnect an existing console client
- `v` on **Instances**: inspect and refresh the retained console log
- `E`, `g`, `M`, `T`: configure, manage devices, rename, or migrate
- `W` on a stopped **Instances** row: guarded root-disk rebuild from an image
  alias, or as an empty instance when the alias is left blank
- `J` on **Instances**: browse retained instance/exec logs, inspect their
  contents, delete individual logs, and clear the serial-console log
- `H` on **Instances**: searchable live Prometheus metrics for the selected
  instance, including CPU, memory, disk, network, process, filesystem, and OOM series
- `U` on **Instances**: image metadata properties, metadata-template create/edit/delete,
  guarded memory-dump export, VM VGA screenshot export, and instance repair controls
- In Instance maintenance Diagnostics, `p` starts a one-client loopback SFTP or
  NBD bridge directly to the selected instance's binary Incus endpoint
- `p` on **Instances**: create a named, optionally stateful snapshot
- `b` on **Instances**: create an exportable backup with instance-only or
  optimized-storage options
- `o` on **Instances**: file editor; `Ctrl-S` saves, `Ctrl-P` pulls, `Ctrl-U`
  pushes, and `Ctrl-D` opens a typed deletion confirmation
- `m` on **Instances**: snapshot/backup manager with restore, rename,
  import/export, and deletion
- `y` on **Instances** opens placement and evacuation preflight: member status,
  architecture compatibility, current location, failure domain, groups, roles,
  and the selected instance's evacuation mode. The normal structured instance
  editor exposes `auto`, `stop`, `migrate`, and `live-migrate` choices.
- `Ctrl-b` on **Instances** configures a persistent local backup schedule using
  enabled, hourly/6-hourly/daily/weekly interval, name pattern, instance-only,
  and optimized-storage fields. Due schedules execute while virtty is running;
  `{timestamp}` expands into a collision-resistant backup name.
- `c` on **Operations**: cancel after typing the operation ID; `g` in details
  jumps to an affected resource
- `Enter` on **Projects**: switch the active project used by creation forms
- `X`, `z`, `B`, `A`, `N` on **Networks**: services, DNS zones, BGP, address
  sets, and OVN integrations
- Inside network services, `L`/`R` inspect or rename an ACL and `S` samples
  structured per-backend load-balancer health. Repeated samples retain a
  bounded in-session history instead of exposing raw state JSON.
- Load-balancer create/edit forms include extension-gated backend health checks,
  intervals, timeouts, and success/failure thresholds
- Network configuration includes structured DHCP expiry, DNS mode, native/OVS
  bridge driver, tagged VLAN, GVRP, physical uplink gateway/range, and OVN
  ingress-mode controls.
- `N` inside network services inspects live interfaces, counters, addresses,
  routes, and driver-specific network state
- `R` inside DNS-zone, address-set, and OVN-integration managers renames the
  selected resource through a dedicated input popup
- `Y` on **Networks**: searchable deployment-wide address/MAC/NAT allocation inspector
- `v`, `b` on **Storage**: custom volumes, backup/ISO import and recovery, snapshots, backups, rebuild, instance/profile attach/detach, and editable bucket credentials/backup export/import
- Custom-volume creation includes initial UID/GID/mode and guarded shared-block
  attachment policy when advertised by the Incus server
- `R` on a storage pool shows detailed capacity/resource data; `C` inside the
  custom-volume manager copies or refreshes a volume across pools/projects with snapshot control
- `P` inside the custom-volume manager starts a loopback SFTP/NBD bridge; port
  `0` automatically selects a free local port
- `S` inside the custom-volume manager inspects live usage and filesystem/block
  state when supported by the server
- `F` inside the custom-volume manager opens a structured file browser/editor
  with directory navigation, read/save, pull/push, and guarded deletion
- `A` on **Projects**: inspect Incus identity/provider/role access for the selected project
- `A` on **Instances**: inspect Incus identity/provider/role access for the selected instance
- `S` on **Images**: push the selected image to a remote Incus image-receive
  endpoint with structured target, project, secret, certificate, alias, and profile fields
- `F`, `C` on **Images**: force-refresh a source-backed image when supported,
  or copy it into another local project with public/private selection
- `V` on **Images** calculates an exact cached-image retention dry run from the
  live `images.remote_cache_expiry` server setting and each cached image's
  upload timestamp, showing explicit `DELETE`/`KEEP` candidates.
- Image imports accept extension-gated aliases during upload, and image editing
  exposes privileged, nesting, secure-boot, agent-CD-ROM, and cloud-init-CD-ROM requirements
- `s` then **Cluster** in Settings: edit members, generate join tokens, evacuate/restore, and safely remove members
- In the Settings cluster section, `R` renames a member, `S` inspects live
  member state, `I` opens initialization/join configuration, and `C` rotates cluster TLS
- Cluster groups support `R` rename in addition to create, edit, membership, and delete
- `c`, `E`, `s`, `K` on **Settings**: change target, server config,
  trust/warnings/member administration, and cluster groups
- `A` on **Settings** opens the focused OIDC/OpenFGA configuration workflow
  with a live completeness preview. OpenFGA API tokens are always masked and
  remain unchanged unless the operator deliberately types a replacement.
- `G` on **Settings** opens direct OpenFGA relationship management using the
  configured external service. Structured subject/object types and relation
  choices support effective entitlement checks plus exact-confirmed grants and
  revocations; the configured bearer token is never rendered.
- `J` on **Settings** opens the persistent local action journal. It records
  payload-free mutation names, target, submission time, and status, supports
  interactive filtering, caps retention, and never stores command bodies,
  configuration values, tokens, passwords, certificates, or private keys.
- `F` on **Settings** opens fleet health and drift control. It concurrently
  probes every saved remote, reports reachability, Incus version, total/running
  instances, warnings, and exact server-configuration key drift against the
  first reachable target; `Enter` switches directly to the selected remote.
- `Q` inside Settings opens a searchable matrix of the connected server's API
  extensions so specialist controls can be audited without leaving the TUI
- Generic instance, network, and server forms now remove extension-dependent
  fields before editing when the connected server lacks the required API
  extension, and show the exact omitted capability names in the popup.
- `M` inside Settings opens the server-generated metadata configuration schema;
  cluster-member editing also includes failure-domain placement
- `L` inside Settings opens extension-gated, dynamically named Loki, syslog,
  and webhook targets with structured event, lifecycle, authentication, TLS,
  retry, facility, labels, and log-level fields; secrets are not displayed
- `T` inside Settings opens the staged placement/authorization Starlark studio:
  `F2` validates local structure, `F3` loads a safe template, `Ctrl-a` performs
  typed-confirm activation with authoritative Incus validation, and `Ctrl-r`
  restores the last accepted source
- `P` on Settings opens typed preseed/bootstrap administration with selectable
  server, cluster join/group, storage, network, and profile sections, structured import/export,
  validation/preview, dependency-ordered apply, and per-section results
  (`Ctrl-s` stages another focused pool/network/profile entry and `Ctrl-d`
  removes the most recently staged entry in that section); profile pool and
  network associations are live choices populated from the staged resources
- `G` on Storage opens dedicated loop-pool growth and driver-health controls:
  growth validates old/new sizes, requires an exact typed phrase, and is gated
  by `storage_pool_loop_resize`; health normalizes capacity/inodes plus
  ZFS/Btrfs/LVM/Ceph/LINSTOR-specific configuration
- New storage-pool forms expose extension-gated, creation-only source wiping;
  it requires an explicit `/dev/...` identity and typing the complete device
  and pool phrase before Incus performs mounted/in-use/signature checks
- Custom-volume copy now exposes disabled/enabled capability-aware controls for
  live migration and refresh exclusion of snapshots older than the target's
  latest snapshot
- Instance details combine definition and runtime-state requests to show
  started-at, boot ID, guest-agent status, OS/kernel/hostname, PID/processes,
  memory, and CPU state when advertised
- Explicit VM CPU topology values such as `sockets=2,cores=4,threads=2` receive
  a live vCPU preview and are rejected before submission when malformed
- Network live state is normalized into addresses, routes, counters,
  bond/LACP, bridge/FDB, VLAN, and OVN logical-router/chassis sections instead
  of displaying the raw response document
- `Y` on a selected VM or profile opens the same staged workflow for
  `raw.qemu.scriptlet`, gated by `qemu_scriptlet`, with a stage-aware safe
  template and ETag-protected activation/rollback
- `Z` on a selected VM opens the extension-gated structured QMP hook manager:
  startup stage and command are dropdowns, commands come from a strict
  allow-list, arguments are typed fields, and applying the generated command
  list requires a VM/stage-specific phrase
- `B` on Instances opens bulk lifecycle control. It defaults to dry-run,
  validates every selected target, bounds concurrency, requires an exact typed
  phrase before mutation, reports per-target results, and supports failed-only
  retry selection
- Applied bulk jobs run in the background so the TUI stays responsive. The
  popup shows live completed/total progress; `c` cooperatively cancels targets
  that have not been dispatched, while in-flight results and failures remain
  visible and failed targets can still be reselected for retry.
- The instance log manager lists ordinary logs and recorded exec output as
  separate typed entries. `X` performs an exact-phrase guarded cleanup of all
  recorded stdout/stderr files while preserving ordinary daemon logs.
- `D` in custom-volume management opens the `storage_volume_nbd`-gated dirty
  bitmap manager. It shows dirty-byte count, granularity, recording/persistent/
  busy/inconsistent state, creates bitmaps through typed fields, and requires
  an exact phrase before deletion.
- `b` in a VM's Diagnostics maintenance tab creates one coordinated dirty
  bitmap across the VM and its dependent block volumes using typed name,
  granularity, persistence, and initial-state controls.
- `O` on an instance opens its structured snapshot policy directly. Schedule
  and expiry use preset choices, naming remains a labelled input, stopped-state
  snapshots are a toggle, and the form displays the resulting retention rule
  before applying the ETag-protected instance update.
- `O` on an image jumps directly to its per-image auto-update toggle. `I` in
  Settings opens the global image refresh policy with update-interval and
  remote-cache-expiry choices, cached-image refresh toggle, and a live summary
  before the ETag-protected server update.
- `V` in the snapshots/backups manager runs a non-mutating retention
  simulation. It evaluates RFC3339 expiry timestamps and lists every exact
  artifact as `DELETE` or `KEEP`; it never performs deletion itself.
- `'` opens persistent operator bookmarks. `n` saves the current page and
  filter under a name, `Enter` restores both, and `d` removes the selected
  view. Views are atomically stored under the virtty XDG configuration folder.
- `X` on Events or Operations opens structured audit export: destination and
  filter inputs, event/operation toggles, live match counts, and recursive
  secret redaction before writing the versioned JSON export.
- `P` in instance maintenance's Templates section edits target path, UID, GID,
  and octal mode when `image_template_permissions` is advertised
- `q` or `Esc`: quit

Settings connection management keeps a named remote inventory in
`$XDG_CONFIG_HOME/virtty/remotes.json` (or `~/.config/virtty/remotes.json`).
Open it with `c`, use `Up`/`Down` to load a saved target, `Ctrl-S` to create or
replace one, `Ctrl-D` to remove one, and `Enter` to switch the active client and
event stream without restarting virtty. TLS files are stored as paths; private
key contents are never copied into the inventory.

## Development

```sh
make build
make test
make verify
make release TYPE=patch
```
The TUI follows the visual language of the sibling `nastty` project: the same
terminal-derived `color1` accent, `color0` selected-tab text, Catppuccin-like
supporting palette, rounded low-contrast cards, top tabs, responsive left-side
navigation panel, workspace, compact status/footer strip, full-row focus tint,
dimmed modal backdrop, and hard `color0` popup shadows. The resources and
controls remain Incus-specific rather than copying nastty's storage content.
