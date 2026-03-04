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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sql: String,
    pub timestamp: String,
    pub success: bool,
}

const MAX_HISTORY: usize = 200;

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

fn history_file() -> PathBuf {
    config_dir().join("query_history.json")
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

pub fn append_history(entry: HistoryEntry) -> Result<()> {
    let mut history = load_history();
    history.insert(0, entry);
    history.truncate(MAX_HISTORY);
    let json = serde_json::to_string_pretty(&history)?;
    fs::write(history_file(), json)?;
    Ok(())
}

pub fn load_history() -> Vec<HistoryEntry> {
    let path = history_file();
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn clear_history() -> Result<()> {
    let path = history_file();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn now_timestamp() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple human-readable: just use epoch seconds for now, formatted in render
    format!("{}", secs)
}
