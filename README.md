# virtty

Full Incus administration from the terminal.

`virtty` connects directly to the Incus API and manages local or remote
container and virtual-machine hosts without a helper daemon or `incus` CLI
subprocesses.

## What it manages

- Containers and virtual machines
- Images, profiles, projects, and resource limits
- Storage pools, custom volumes, buckets, snapshots, and backups
- Networks, ACLs, forwards, peers, load balancers, DNS zones, and BGP
- Cluster members, groups, placement, evacuation, and migration
- Trust certificates, identities, OpenFGA relationships, and server policy
- Operations, lifecycle events, metrics, warnings, logs, and audit exports
- Local and remote Incus targets

Instance creation includes container/VM selection, image source, project,
profiles, CPU, memory, storage, networking, cloud-init, cluster placement,
security, boot policy, and a final review before submission.

Destructive operations use explicit confirmations. Secrets, private keys, and
configuration payloads are excluded from the local action journal.

## Requirements

- A running Incus server
- Local socket access, normally through the `incus-admin` group, or an
  authorized remote TLS identity
- `remote-viewer` only when using VM VGA-console handoff

## Build and run

```sh
make build
make run
```

The default target is the local socket at `/var/lib/incus/unix.socket`.

Use another socket:

```sh
make run ARGS='--socket /path/to/incus.socket'
```

Connect to a remote server:

```sh
make run ARGS='--remote https://incus.example.net:8443'
```

Connect with client TLS authentication:

```sh
make run ARGS='--remote https://incus.example.net:8443 \
  --client-cert ~/.config/incus/client.crt \
  --client-key ~/.config/incus/client.key'
```

Add `--server-ca server-ca.crt` when the remote uses a private CA.

## Usage

Open **Workloads → Instances** and choose **CREATE VM / CONTAINER** to create a
workload. Press `?` anywhere for commands relevant to the current section.

The essential global controls are:

| Key | Action |
| --- | --- |
| `Left` / `Right` | Change section |
| `Tab` / `Shift-Tab` | Change view within a section |
| `j` / `k` | Move through resources |
| `Enter` | Open or activate the selected item |
| `x` | Open actions for the current view |
| `/` | Filter the current view |
| `r` | Refresh from Incus |
| `?` | Show contextual help |
| `q` | Quit |

Saved remotes, operator bookmarks, backup schedules, and the redacted action
journal are stored under `$XDG_CONFIG_HOME/virtty` or `~/.config/virtty`.

## Development

```sh
make fmt
make test
make verify
```

Build the optimized binary:

```sh
make build-release
```

## Release

```sh
make release TYPE=patch
```

Publishing a GitHub release builds Linux AMD64 and ARM64 binaries and uploads
each executable, a compressed archive, and SHA-256 checksums.

## License

MIT
