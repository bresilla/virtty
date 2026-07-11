use std::{env, error::Error, process::ExitCode};

use virtty::{incus::IncusClient, tui};

fn usage() {
    eprintln!(
        "virtty - terminal control plane for Incus\n\nUSAGE:\n    virtty [--socket PATH | --remote URL [TLS OPTIONS]]\n\nOPTIONS:\n    --socket PATH         Local Incus Unix socket (default: /var/lib/incus/unix.socket)\n    --remote URL          Remote Incus HTTPS endpoint\n    --client-cert PATH    PEM client certificate for the remote\n    --client-key PATH     PEM client private key for the remote\n    --server-ca PATH      Optional private CA certificate for the remote\n    -h, --help            Print this help"
    );
}

fn client_from_args() -> Result<Option<IncusClient>, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let mut socket = None;
    let mut remote = None;
    let mut client_cert = None;
    let mut client_key = None;
    let mut server_ca = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--socket" => socket = Some(args.next().ok_or("--socket requires a path")?),
            "--remote" => remote = Some(args.next().ok_or("--remote requires a URL")?),
            "--client-cert" => {
                client_cert = Some(args.next().ok_or("--client-cert requires a path")?)
            }
            "--client-key" => client_key = Some(args.next().ok_or("--client-key requires a path")?),
            "--server-ca" => server_ca = Some(args.next().ok_or("--server-ca requires a path")?),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    match (socket, remote) {
        (Some(_), Some(_)) => Err("--socket and --remote cannot be used together".into()),
        (Some(path), None) => Ok(Some(IncusClient::local_socket(path)?)),
        (None, Some(url)) => match (client_cert, client_key) {
            (Some(cert), Some(key)) => Ok(Some(IncusClient::remote_tls(url, cert, key, server_ca)?)),
            (None, None) if server_ca.is_none() => Ok(Some(IncusClient::remote(url)?)),
            _ => Err("remote TLS requires both --client-cert and --client-key; --server-ca is valid only with them".into()),
        },
        (None, None) => Ok(Some(IncusClient::local_default()?)),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .without_time()
        .init();

    match client_from_args() {
        Ok(Some(client)) => match tui::run(client).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("virtty terminal error: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(None) => {
            usage();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("virtty: {error}\n");
            usage();
            ExitCode::FAILURE
        }
    }
}
