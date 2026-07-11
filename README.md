# virtty

A full terminal control plane for
[Incus](https://linuxcontainers.org/incus/) containers and virtual machines.

`virtty` talks directly to the Incus API. It manages instances, images,
profiles, projects, storage, networks, clusters, operations, events, access,
backups, and server configuration through structured controls rather than raw
JSON or CLI subprocesses.

## Run

The current user needs access to Incus, normally through `incus-admin`.

```sh
make run
```

Remote server:

```sh
make run ARGS='--remote https://incus.example.net:8443'
```

Press `?` for help specific to the current section. Open **Workloads →
Instances** and select **CREATE VM / CONTAINER** to create a workload.

## Develop

```sh
make build
make verify
```

## Release

```sh
make release TYPE=patch
```

## License

MIT
