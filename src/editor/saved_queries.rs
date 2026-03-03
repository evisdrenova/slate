use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: String,
    pub name: String,
    pub sql: String,
}

fn config_dir() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("slate");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn queries_file() -> PathBuf {
    config_dir().join("saved_queries.json")
}

pub fn save_queries(queries: &[SavedQuery]) -> Result<()> {
    let json = serde_json::to_string_pretty(queries)?;
    fs::write(queries_file(), json)?;
    Ok(())
}

pub fn load_queries() -> Vec<SavedQuery> {
    let path = queries_file();
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}
