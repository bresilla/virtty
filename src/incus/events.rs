use std::{io::BufReader, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::mpsc,
};
use tokio_tungstenite::{
    Connector, WebSocketStream, client_async, connect_async, connect_async_tls_with_config,
};

use super::Transport;

/// An event emitted by Incus's `/1.0/events` WebSocket endpoint.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct EventEntry {
    #[serde(rename = "type", default)]
    pub event_type: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub metadata: Value,
}

impl EventEntry {
    /// Condenses variable Incus event payloads into a line suitable for the TUI.
    pub fn summary(&self) -> String {
        match self.event_type.as_str() {
            "lifecycle" => {
                let action = metadata_string(&self.metadata, "action").unwrap_or("changed");
                let source = metadata_string(&self.metadata, "source").unwrap_or("resource");
                format!("{action}: {source}")
            }
            "operation" => {
                let id = metadata_string(&self.metadata, "id").unwrap_or("operation");
                let status = metadata_string(&self.metadata, "status").unwrap_or("updated");
                format!("{status}: {id}")
            }
            "logging" => metadata_string(&self.metadata, "message")
                .or_else(|| metadata_string(&self.metadata, "context"))
                .unwrap_or("log message")
                .to_owned(),
            "" => "event received".to_owned(),
            event_type => format!("{event_type} event"),
        }
    }

    /// Lifecycle and operation events may change data shown by the active dashboard.
    pub fn needs_refresh(&self) -> bool {
        matches!(self.event_type.as_str(), "lifecycle" | "operation")
    }
}

fn metadata_string<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata.get(key).and_then(Value::as_str)
}

/// Starts a resilient background event listener. It reconnects after a short delay.
pub(crate) fn subscribe(transport: Transport) -> mpsc::Receiver<EventEntry> {
    let (sender, receiver) = mpsc::channel(128);
    tokio::spawn(async move {
        loop {
            let result = listen_once(&transport, &sender).await;
            if sender.is_closed() {
                break;
            }
            let _ = result;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    receiver
}

async fn listen_once(
    transport: &Transport,
    sender: &mpsc::Sender<EventEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = transport.event_url();
    match transport.local_socket_path() {
        Some(socket_path) => {
            let stream = UnixStream::connect(socket_path).await?;
            let (stream, _) = client_async(url, stream).await?;
            forward_events(stream, sender).await
        }
        None => {
            let (stream, _) = if let Some((certificate, key, server_ca)) = transport.tls_paths() {
                let connector =
                    Connector::Rustls(Arc::new(tls_config(certificate, key, server_ca)?));
                connect_async_tls_with_config(url, None, false, Some(connector)).await?
            } else {
                connect_async(url).await?
            };
            forward_events(stream, sender).await
        }
    }
}

pub(crate) async fn interactive_session(
    transport: &Transport,
    path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = transport.websocket_url(path);
    match transport.local_socket_path() {
        Some(socket_path) => {
            let stream = UnixStream::connect(socket_path).await?;
            let (stream, _) = client_async(url, stream).await?;
            bridge_terminal(stream).await
        }
        None => {
            let (stream, _) = if let Some((certificate, key, server_ca)) = transport.tls_paths() {
                let connector =
                    Connector::Rustls(Arc::new(tls_config(certificate, key, server_ca)?));
                connect_async_tls_with_config(url, None, false, Some(connector)).await?
            } else {
                connect_async(url).await?
            };
            bridge_terminal(stream).await
        }
    }
}

pub(crate) async fn vga_session(
    transport: &Transport,
    path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let mut viewer = std::process::Command::new("remote-viewer")
        .arg(format!("spice://{address}"))
        .spawn()
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot launch remote-viewer: {error}"),
            )
        })?;
    let accepted = tokio::time::timeout(Duration::from_secs(15), listener.accept()).await;
    let (viewer_stream, _) = match accepted {
        Ok(result) => result?,
        Err(_) => {
            let _ = viewer.kill();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "remote-viewer did not connect within 15 seconds",
            )
            .into());
        }
    };
    let result = connect_and_proxy(transport, path, viewer_stream).await;
    let _ = viewer.kill();
    let _ = viewer.wait();
    result
}

async fn connect_and_proxy<T>(
    transport: &Transport,
    path: &str,
    client: T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let url = transport.websocket_url(path);
    match transport.local_socket_path() {
        Some(socket_path) => {
            let stream = UnixStream::connect(socket_path).await?;
            let (stream, _) = client_async(url, stream).await?;
            bridge_binary(stream, client).await
        }
        None => {
            let (stream, _) = if let Some((certificate, key, server_ca)) = transport.tls_paths() {
                let connector =
                    Connector::Rustls(Arc::new(tls_config(certificate, key, server_ca)?));
                connect_async_tls_with_config(url, None, false, Some(connector)).await?
            } else {
                connect_async(url).await?
            };
            bridge_binary(stream, client).await
        }
    }
}

/// Opens a loopback TCP listener and bridges the first client to an Incus binary WebSocket.
pub(crate) async fn start_binary_proxy(
    transport: Transport,
    path: String,
    port: u16,
) -> Result<std::net::SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        if let Ok((client, _)) = listener.accept().await {
            let _ = connect_and_proxy(&transport, &path, client).await;
        }
    });
    Ok(address)
}

async fn bridge_binary<S, T>(
    stream: WebSocketStream<S>,
    client: T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
{
    let (mut socket_out, mut socket_in) = stream.split();
    let (mut client_in, mut client_out) = tokio::io::split(client);
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        tokio::select! {
            read = client_in.read(&mut buffer) => {
                let count = read?;
                if count == 0 { break; }
                socket_out.send(tokio_tungstenite::tungstenite::Message::Binary(buffer[..count].to_vec().into())).await?;
            }
            message = socket_in.next() => match message {
                Some(Ok(message)) if message.is_binary() => {
                    client_out.write_all(&message.into_data()).await?;
                    client_out.flush().await?;
                }
                Some(Ok(message)) if message.is_close() => break,
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(Box::new(error)),
                None => break,
            }
        }
    }
    Ok(())
}

async fn bridge_terminal<S>(
    stream: WebSocketStream<S>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut socket_out, mut socket_in) = stream.split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut input = [0_u8; 4096];
    loop {
        tokio::select! {
            read = stdin.read(&mut input) => {
                let count = read?;
                if count == 0 || input[..count].contains(&0x1d) {
                    let _ = socket_out.close().await;
                    break;
                }
                socket_out.send(tokio_tungstenite::tungstenite::Message::Binary(input[..count].to_vec().into())).await?;
            }
            message = socket_in.next() => {
                match message {
                    Some(Ok(message)) if message.is_binary() || message.is_text() => {
                        stdout.write_all(&message.into_data()).await?;
                        stdout.flush().await?;
                    }
                    Some(Ok(message)) if message.is_close() => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(Box::new(error)),
                    None => break,
                }
            }
        }
    }
    Ok(())
}

fn tls_config(
    certificate: &std::path::Path,
    key: &std::path::Path,
    server_ca: Option<&std::path::Path>,
) -> Result<rustls::ClientConfig, Box<dyn std::error::Error + Send + Sync>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = server_ca {
        let mut reader = BufReader::new(std::fs::File::open(path)?);
        for certificate in rustls_pemfile::certs(&mut reader) {
            roots.add(certificate?)?;
        }
    }
    let mut certificate_reader = BufReader::new(std::fs::File::open(certificate)?);
    let certificates =
        rustls_pemfile::certs(&mut certificate_reader).collect::<Result<Vec<_>, _>>()?;
    let mut key_reader = BufReader::new(std::fs::File::open(key)?);
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .ok_or("client TLS key file contains no private key")?;
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(certificates, key)?)
}

async fn forward_events<S>(
    mut stream: WebSocketStream<S>,
    sender: &mpsc::Sender<EventEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(message) = stream.next().await {
        let message = message?;
        if !message.is_text() {
            continue;
        }
        let event = serde_json::from_str::<EventEntry>(message.into_text()?.as_ref())?;
        if sender.send(event).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::EventEntry;

    #[test]
    fn summarizes_lifecycle_events() {
        let event: EventEntry = serde_json::from_str(
            r#"{"type":"lifecycle","metadata":{"action":"instance-started","source":"/1.0/instances/demo"}}"#,
        )
        .expect("valid event");

        assert_eq!(event.summary(), "instance-started: /1.0/instances/demo");
        assert!(event.needs_refresh());
    }
}
