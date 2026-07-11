//! Persistent local backup schedules executed while virtty is running.

use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct BackupSchedule {
    pub instance: String,
    pub project: String,
    pub interval_seconds: u64,
    pub name_pattern: String,
    pub instance_only: bool,
    pub optimized_storage: bool,
    pub enabled: bool,
    pub last_run: Option<u64>,
    pub last_result: String,
}

impl BackupSchedule {
    pub fn due(&self, now: u64) -> bool {
        self.enabled
            && self.interval_seconds > 0
            && self
                .last_run
                .is_none_or(|last| now.saturating_sub(last) >= self.interval_seconds)
    }

    pub fn backup_name(&self, now: u64) -> String {
        self.name_pattern.replace("{timestamp}", &now.to_string())
    }
}

pub fn path() -> PathBuf {
    if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(root).join("virtty/backup-schedules.json");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/virtty/backup-schedules.json")
}

pub fn load() -> io::Result<Vec<BackupSchedule>> {
    match fs::read(path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub fn save(schedules: &[BackupSchedule]) -> io::Result<()> {
    let path = path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("invalid schedule path"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(schedules).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::BackupSchedule;

    #[test]
    fn schedule_due_and_name_are_deterministic() {
        let mut schedule = BackupSchedule {
            instance: "vm1".into(),
            project: "default".into(),
            interval_seconds: 3600,
            name_pattern: "auto-{timestamp}".into(),
            instance_only: false,
            optimized_storage: true,
            enabled: true,
            last_run: Some(100),
            last_result: String::new(),
        };
        assert!(!schedule.due(3699));
        assert!(schedule.due(3700));
        assert_eq!(schedule.backup_name(3700), "auto-3700");
        schedule.enabled = false;
        assert!(!schedule.due(9999));
    }
}
