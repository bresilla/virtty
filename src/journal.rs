//! Persistent, secret-free journal of actions submitted from the TUI.

use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ActionEntry {
    pub timestamp: u64,
    pub target: String,
    pub action: String,
    pub status: String,
}

pub fn path() -> PathBuf {
    if let Some(root) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(root).join("virtty/action-journal.json");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/virtty/action-journal.json")
}

pub fn load() -> io::Result<Vec<ActionEntry>> {
    match fs::read(path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub fn save(entries: &[ActionEntry]) -> io::Result<()> {
    let path = path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid action journal path"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(entries).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::ActionEntry;

    #[test]
    fn entry_round_trips_without_payloads() {
        let entry = ActionEntry {
            timestamp: 42,
            target: "local".into(),
            action: "UpdateResource".into(),
            status: "submitted".into(),
        };
        let bytes = serde_json::to_vec(&entry).unwrap();
        assert_eq!(
            serde_json::from_slice::<ActionEntry>(&bytes).unwrap(),
            entry
        );
    }
}
