use anyhow::Result;

use super::connection::DatabaseService;

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
    // Fetch table names
    let table_result = db.execute(&format!(
        "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME",
        database.replace('\'', "''")
    ))?;

    let mut tables = Vec::new();

    for row in &table_result.rows {
        let table_name = match &row.cells[0] {
            super::types::CellValue::String(s) => s.clone(),
            _ => continue,
        };

        // Fetch columns for this table
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
                    super::types::CellValue::String(s) => s.clone(),
                    super::types::CellValue::Null => String::new(),
                    other => other.display(),
                };
                ColumnInfo {
                    name: get_str(0),
                    data_type: get_str(1),
                    is_nullable: get_str(2) == "YES",
                    is_primary_key: get_str(3) == "PRI",
                    column_key: get_str(3),
                    default_value: match &r.cells[4] {
                        super::types::CellValue::Null => None,
                        other => Some(other.display()),
                    },
                }
            })
            .collect();

        // Fetch indexes for this table
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
                super::types::CellValue::String(s) => s.clone(),
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
