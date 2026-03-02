use anyhow::Result;
use serde_json;
use std::fs;
use std::path::PathBuf;

use crate::db::types::ConnectionConfig;

fn config_dir() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("slate");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn connections_file() -> PathBuf {
    config_dir().join("connections.json")
}

pub fn save_connections(connections: &[ConnectionConfig]) -> Result<()> {
    let json = serde_json::to_string_pretty(connections)?;
    fs::write(connections_file(), json)?;
    Ok(())
}

pub fn load_connections() -> Vec<ConnectionConfig> {
    let path = connections_file();
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_password(connection_id: &str, password: &str) -> Result<()> {
    let entry = keyring::Entry::new("slate", connection_id)?;
    entry.set_password(password)?;
    Ok(())
}

pub fn load_password(connection_id: &str) -> Option<String> {
    let entry = keyring::Entry::new("slate", connection_id).ok()?;
    entry.get_password().ok()
}

pub fn delete_password(connection_id: &str) -> Result<()> {
    let entry = keyring::Entry::new("slate", connection_id)?;
    let _ = entry.delete_credential();
    Ok(())
}
