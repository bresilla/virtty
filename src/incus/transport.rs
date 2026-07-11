use std::path::{Path, PathBuf};

/// Connection configuration for an Incus daemon.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Transport {
    /// A local Incus daemon exposed through its Unix socket.
    LocalUnix { socket_path: PathBuf },
    /// An Incus remote addressed through HTTPS (or HTTP for development).
    Remote {
        base_url: String,
        client_certificate: Option<PathBuf>,
        client_key: Option<PathBuf>,
        server_ca: Option<PathBuf>,
    },
}

impl Transport {
    /// Uses the standard local Incus socket location.
    pub fn local_default() -> Self {
        Self::LocalUnix {
            socket_path: PathBuf::from("/var/lib/incus/unix.socket"),
        }
    }

    /// Builds a local Unix-socket transport.
    pub fn local(socket_path: impl AsRef<Path>) -> Self {
        Self::LocalUnix {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Builds a remote transport from a base URL.
    pub fn remote(base_url: impl Into<String>) -> Self {
        Self::Remote {
            base_url: base_url.into(),
            client_certificate: None,
            client_key: None,
            server_ca: None,
        }
    }

    pub fn remote_tls(
        base_url: impl Into<String>,
        certificate: impl AsRef<Path>,
        key: impl AsRef<Path>,
        server_ca: Option<impl AsRef<Path>>,
    ) -> Self {
        Self::Remote {
            base_url: base_url.into(),
            client_certificate: Some(certificate.as_ref().to_path_buf()),
            client_key: Some(key.as_ref().to_path_buf()),
            server_ca: server_ca.map(|path| path.as_ref().to_path_buf()),
        }
    }

    /// A short target label for the TUI status line.
    pub fn label(&self) -> String {
        match self {
            Self::LocalUnix { socket_path } => format!("local:{}", socket_path.display()),
            Self::Remote { base_url, .. } => base_url.clone(),
        }
    }

    pub(crate) fn event_url(&self) -> String {
        const EVENTS_PATH: &str = "/1.0/events?type=lifecycle&type=operation&type=logging";

        self.websocket_url(EVENTS_PATH)
    }

    pub(crate) fn websocket_url(&self, path: &str) -> String {
        match self {
            Self::LocalUnix { .. } => format!("ws://incus{path}"),
            Self::Remote { base_url, .. } => {
                let base_url = base_url.trim_end_matches('/');
                let base_url = base_url.trim_end_matches("/1.0");
                let scheme = if let Some(url) = base_url.strip_prefix("https://") {
                    format!("wss://{url}")
                } else if let Some(url) = base_url.strip_prefix("http://") {
                    format!("ws://{url}")
                } else {
                    base_url.to_owned()
                };
                format!("{scheme}{path}")
            }
        }
    }

    pub(crate) fn local_socket_path(&self) -> Option<&Path> {
        match self {
            Self::LocalUnix { socket_path } => Some(socket_path),
            Self::Remote { .. } => None,
        }
    }

    pub(crate) fn tls_paths(&self) -> Option<(&Path, &Path, Option<&Path>)> {
        match self {
            Self::Remote {
                client_certificate: Some(cert),
                client_key: Some(key),
                server_ca,
                ..
            } => Some((cert, key, server_ca.as_deref())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Transport;

    #[test]
    fn builds_an_event_url_for_each_transport() {
        assert_eq!(
            Transport::local("/tmp/incus.sock").event_url(),
            "ws://incus/1.0/events?type=lifecycle&type=operation&type=logging"
        );
        assert_eq!(
            Transport::remote("https://incus.example.test:8443/1.0").event_url(),
            "wss://incus.example.test:8443/1.0/events?type=lifecycle&type=operation&type=logging"
        );
        assert_eq!(
            Transport::local("/tmp/incus.sock")
                .websocket_url("/1.0/operations/op/websocket?secret=token"),
            "ws://incus/1.0/operations/op/websocket?secret=token"
        );
    }

    #[test]
    fn retains_remote_tls_identity_for_rest_and_events() {
        let transport = Transport::remote_tls(
            "https://incus.example.test:8443",
            "/tmp/client.crt",
            "/tmp/client.key",
            Some("/tmp/server-ca.crt"),
        );
        let (certificate, key, ca) = transport.tls_paths().expect("TLS paths");
        assert_eq!(certificate.to_str(), Some("/tmp/client.crt"));
        assert_eq!(key.to_str(), Some("/tmp/client.key"));
        assert_eq!(
            ca.and_then(|path| path.to_str()),
            Some("/tmp/server-ca.crt")
        );
        assert_eq!(
            transport.event_url(),
            "wss://incus.example.test:8443/1.0/events?type=lifecycle&type=operation&type=logging"
        );
    }
}
