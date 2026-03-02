use anyhow::Result;

use super::connection::DatabaseService;
use super::types::{CellValue, DbType};

#[derive(Debug, Clone)]
pub struct DatabaseSchema {
    pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub column_key: String,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}

pub fn fetch_schema(db: &DatabaseService, database: &str) -> Result<DatabaseSchema> {
    match db.db_type() {
        DbType::MySQL => fetch_schema_mysql(db, database),
        DbType::PostgreSQL => fetch_schema_postgres(db),
    }
}

fn fetch_schema_mysql(db: &DatabaseService, database: &str) -> Result<DatabaseSchema> {
    let table_result = db.execute(&format!(
        "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME",
        database.replace('\'', "''")
    ))?;

    let mut tables = Vec::new();

    for row in &table_result.rows {
        let table_name = match &row.cells[0] {
            CellValue::String(s) => s.clone(),
            _ => continue,
        };

        let col_result = db.execute(&format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' \
             ORDER BY ORDINAL_POSITION",
            database.replace('\'', "''"),
            table_name.replace('\'', "''")
        ))?;

        let columns: Vec<ColumnInfo> = col_result
            .rows
            .iter()
            .map(|r| {
                let get_str = |i: usize| match &r.cells[i] {
                    CellValue::String(s) => s.clone(),
                    CellValue::Null => String::new(),
                    other => other.display(),
                };
                ColumnInfo {
                    name: get_str(0),
                    data_type: get_str(1),
                    is_nullable: get_str(2) == "YES",
                    is_primary_key: get_str(3) == "PRI",
                    column_key: get_str(3),
                    default_value: match &r.cells[4] {
                        CellValue::Null => None,
                        other => Some(other.display()),
                    },
                }
            })
            .collect();

        let idx_result = db.execute(&format!(
            "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE \
             FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' \
             ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            database.replace('\'', "''"),
            table_name.replace('\'', "''")
        ))?;

        let mut index_map: std::collections::BTreeMap<String, (Vec<String>, bool)> =
            std::collections::BTreeMap::new();
        for r in &idx_result.rows {
            let get_str = |i: usize| match &r.cells[i] {
                CellValue::String(s) => s.clone(),
                other => other.display(),
            };
            let idx_name = get_str(0);
            let col_name = get_str(1);
            let non_unique = get_str(2) != "0";
            let entry = index_map
                .entry(idx_name)
                .or_insert_with(|| (Vec::new(), !non_unique));
            entry.0.push(col_name);
        }

        let indexes: Vec<IndexInfo> = index_map
            .into_iter()
            .map(|(name, (columns, is_unique))| IndexInfo {
                name,
                columns,
                is_unique,
            })
            .collect();

        tables.push(TableInfo {
            name: table_name,
            columns,
            indexes,
        });
    }

    Ok(DatabaseSchema { tables })
}

fn fetch_schema_postgres(db: &DatabaseService) -> Result<DatabaseSchema> {
    // Fetch table names from public schema
    let table_result = db.execute(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
         ORDER BY table_name",
    )?;

    let mut tables = Vec::new();

    for row in &table_result.rows {
        let table_name = match &row.cells[0] {
            CellValue::String(s) => s.clone(),
            _ => continue,
        };

        // Fetch columns
        let col_result = db.execute(&format!(
            "SELECT column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = '{}' \
             ORDER BY ordinal_position",
            table_name.replace('\'', "''")
        ))?;

        // Fetch primary key columns
        let pk_result = db.execute(&format!(
            "SELECT kcu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
             WHERE tc.table_schema = 'public' \
               AND tc.table_name = '{}' \
               AND tc.constraint_type = 'PRIMARY KEY'",
            table_name.replace('\'', "''")
        ))?;

        let pk_columns: Vec<String> = pk_result
            .rows
            .iter()
            .filter_map(|r| match &r.cells[0] {
                CellValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        let columns: Vec<ColumnInfo> = col_result
            .rows
            .iter()
            .map(|r| {
                let get_str = |i: usize| match &r.cells[i] {
                    CellValue::String(s) => s.clone(),
                    CellValue::Null => String::new(),
                    other => other.display(),
                };
                let col_name = get_str(0);
                let is_pk = pk_columns.contains(&col_name);
                ColumnInfo {
                    name: col_name,
                    data_type: get_str(1),
                    is_nullable: get_str(2) == "YES",
                    is_primary_key: is_pk,
                    column_key: if is_pk { "PRI".to_string() } else { String::new() },
                    default_value: match &r.cells[3] {
                        CellValue::Null => None,
                        other => Some(other.display()),
                    },
                }
            })
            .collect();

        // Fetch indexes
        let idx_result = db.execute(&format!(
            "SELECT indexname, indexdef FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = '{}'",
            table_name.replace('\'', "''")
        ))?;

        let indexes: Vec<IndexInfo> = idx_result
            .rows
            .iter()
            .filter_map(|r| {
                let name = match &r.cells[0] {
                    CellValue::String(s) => s.clone(),
                    _ => return None,
                };
                let indexdef = match &r.cells[1] {
                    CellValue::String(s) => s.clone(),
                    _ => return None,
                };
                let is_unique = indexdef.contains("UNIQUE");
                // Parse columns from indexdef: "CREATE INDEX ... ON ... (col1, col2)"
                let idx_columns = parse_pg_index_columns(&indexdef);
                Some(IndexInfo {
                    name,
                    columns: idx_columns,
                    is_unique,
                })
            })
            .collect();

        tables.push(TableInfo {
            name: table_name,
            columns,
            indexes,
        });
    }

    Ok(DatabaseSchema { tables })
}

fn parse_pg_index_columns(indexdef: &str) -> Vec<String> {
    // indexdef looks like: "CREATE [UNIQUE] INDEX name ON table USING btree (col1, col2)"
    if let Some(start) = indexdef.rfind('(') {
        if let Some(end) = indexdef.rfind(')') {
            let cols_str = &indexdef[start + 1..end];
            return cols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
        }
    }
    vec![]
}
