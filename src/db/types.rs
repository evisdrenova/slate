use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbType {
    MySQL,
    PostgreSQL,
}

impl Default for DbType {
    fn default() -> Self {
        DbType::MySQL
    }
}

impl DbType {
    pub fn default_port(self) -> u16 {
        match self {
            DbType::MySQL => 3306,
            DbType::PostgreSQL => 5432,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DbType::MySQL => "MySQL",
            DbType::PostgreSQL => "PostgreSQL",
        }
    }

    pub fn quote_identifier(self, name: &str) -> String {
        match self {
            DbType::MySQL => format!("`{}`", name),
            DbType::PostgreSQL => format!("\"{}\"", name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    #[serde(default)]
    pub db_type: DbType,
}

impl ConnectionConfig {
    pub fn new(
        db_type: DbType,
        name: String,
        host: String,
        port: u16,
        user: String,
        database: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            db_type,
            name,
            host,
            port,
            user,
            database,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<ResultRow>,
    pub execution_time_ms: u128,
    pub affected_rows: u64,
}

#[derive(Debug, Clone)]
pub struct ResultColumn {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct ResultRow {
    pub cells: Vec<CellValue>,
}

#[derive(Debug, Clone)]
pub enum CellValue {
    Null,
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Bytes(Vec<u8>),
    DateTime(String),
}

impl CellValue {
    pub fn display(&self) -> String {
        match self {
            CellValue::Null => "NULL".to_string(),
            CellValue::String(s) => s.clone(),
            CellValue::Integer(i) => i.to_string(),
            CellValue::Float(f) => f.to_string(),
            CellValue::Boolean(b) => b.to_string(),
            CellValue::Bytes(b) => format!("[{} bytes]", b.len()),
            CellValue::DateTime(dt) => dt.clone(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, CellValue::Null)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, CellValue::Integer(_) | CellValue::Float(_))
    }
}
