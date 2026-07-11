//! Persistent operator views and bookmarks.

use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SavedView {
    pub name: String,
    pub page: String,
    pub filter: String,
}

pub fn path() -> PathBuf {
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("virtty/views.json");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/virtty/views.json")
}

pub fn load() -> io::Result<Vec<SavedView>> {
    match fs::read(path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub fn save(views: &[SavedView]) -> io::Result<()> {
    let path = path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid views path"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(views).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::SavedView;
    #[test]
    fn view_round_trips() {
        let value = vec![SavedView {
            name: "running".into(),
            page: "Instances".into(),
            filter: "running".into(),
        }];
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            serde_json::from_slice::<Vec<SavedView>>(&bytes).unwrap(),
            value
        );
    }
}
