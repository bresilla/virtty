//! Persistent named Incus target inventory.

use std::{env, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::incus::Transport;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SavedRemote {
    pub name: String,
    pub transport: Transport,
}

pub fn inventory_path() -> PathBuf {
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("virtty/remotes.json");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/virtty/remotes.json")
}

pub fn load() -> io::Result<Vec<SavedRemote>> {
    let path = inventory_path();
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub fn save(remotes: &[SavedRemote]) -> io::Result<()> {
    let path = inventory_path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid inventory path"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(remotes).map_err(io::Error::other)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::SavedRemote;
    use crate::incus::Transport;

    #[test]
    fn inventory_round_trips_transport_paths() {
        let remotes = vec![SavedRemote {
            name: "lab".into(),
            transport: Transport::remote_tls(
                "https://lab.example:8443",
                "/cert.pem",
                "/key.pem",
                Some("/ca.pem"),
            ),
        }];
        let encoded = serde_json::to_vec(&remotes).expect("encode");
        assert_eq!(
            serde_json::from_slice::<Vec<SavedRemote>>(&encoded).expect("decode"),
            remotes
        );
    }
}
