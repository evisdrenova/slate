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

fn passwords_file() -> PathBuf {
    config_dir().join("passwords.json")
}

fn load_passwords_map() -> std::collections::HashMap<String, String> {
    let path = passwords_file();
    if !path.exists() {
        return std::collections::HashMap::new();
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    }
}

fn save_passwords_map(map: &std::collections::HashMap<String, String>) -> Result<()> {
    let json = serde_json::to_string_pretty(map)?;
    fs::write(passwords_file(), json)?;
    Ok(())
}

pub fn save_password(connection_id: &str, password: &str) -> Result<()> {
    let mut map = load_passwords_map();
    map.insert(connection_id.to_string(), password.to_string());
    save_passwords_map(&map)
}

pub fn load_password(connection_id: &str) -> Option<String> {
    let map = load_passwords_map();
    map.get(connection_id).cloned()
}

pub fn delete_password(connection_id: &str) -> Result<()> {
    let mut map = load_passwords_map();
    map.remove(connection_id);
    save_passwords_map(&map)
}

fn last_connection_file() -> PathBuf {
    config_dir().join("last_connection.json")
}

pub fn save_last_connection_id(connection_id: &str) -> Result<()> {
    let json = serde_json::to_string(connection_id)?;
    fs::write(last_connection_file(), json)?;
    Ok(())
}

pub fn load_last_connection_id() -> Option<String> {
    let path = last_connection_file();
    if !path.exists() {
        return None;
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).ok(),
        Err(_) => None,
    }
}
