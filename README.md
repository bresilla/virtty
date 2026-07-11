# virtty

`virtty` is a Linux terminal control plane for Incus.

It builds one executable:

```sh
virtty   # containers, virtual machines, storage, networks, and clusters
```

## Install

Download the binary for your architecture from the
[GitHub Releases](https://github.com/bresilla/virtty/releases) page.

```sh
# amd64 example
tar -xzf virtty-linux-amd64.tar.gz
sudo install -m 0755 virtty-linux-amd64 /usr/local/bin/virtty
virtty --version
```

Use `arm64` instead of `amd64` on a 64-bit ARM system.

## Requirements

- Linux
- [Incus](https://linuxcontainers.org/incus/)
- membership in `incus-admin` for local access
- a trusted client certificate for remote TLS access

On Ubuntu:

```sh
sudo usermod -aG incus-admin "$USER"
```

## Run

Open the TUI:

```sh
virtty
```

The default socket is `/var/lib/incus/unix.socket`.

Useful options:

```sh
virtty --socket /path/to/incus.socket
virtty --remote https://192.168.1.10:8443
virtty --remote https://192.168.1.10:8443 \
  --client-cert client.crt --client-key client.key
```

## API

`virtty` connects directly to the Incus REST API at `/1.0` and its event
WebSocket at `/1.0/events`. It does not require a helper daemon.

## Releases

Publishing a GitHub Release builds native Linux `amd64` and `arm64` binaries
and attaches each binary, a `.tar.gz` archive, and SHA-256 checksums.
