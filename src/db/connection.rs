use std::sync::Arc;

use anyhow::Result;
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Pool, Row, Value};
use tokio::runtime::Runtime;

use super::types::*;

pub struct DatabaseService {
    pool: Pool,
    runtime: Arc<Runtime>,
}

impl DatabaseService {
    pub fn connect(config: &ConnectionConfig, password: &str) -> Result<Self> {
        let runtime = Arc::new(Runtime::new()?);

        let opts = OptsBuilder::default()
            .ip_or_hostname(&config.host)
            .tcp_port(config.port)
            .user(Some(&config.user))
            .pass(Some(password))
            .db_name(Some(&config.database));

        let pool = Pool::new(Opts::from(opts));

        // Test connection
        let pool_clone = pool.clone();
        runtime.block_on(async move {
            let mut conn = pool_clone.get_conn().await?;
            let _: Vec<Row> = conn.query("SELECT 1").await?;
            Ok::<_, anyhow::Error>(())
        })?;

        Ok(Self { pool, runtime })
    }

    pub fn execute(&self, sql: &str) -> Result<QueryResult> {
        let pool = self.pool.clone();
        let sql = sql.to_string();

        self.runtime.block_on(async move {
            let start = std::time::Instant::now();
            let mut conn = pool.get_conn().await?;
            let result: Vec<Row> = conn.query(&sql).await?;
            let execution_time_ms = start.elapsed().as_millis();
            let affected = conn.affected_rows();

            let columns = if let Some(first_row) = result.first() {
                first_row
                    .columns_ref()
                    .iter()
                    .map(|col| ResultColumn {
                        name: col.name_str().to_string(),
                        type_name: format!("{:?}", col.column_type()),
                    })
                    .collect()
            } else {
                vec![]
            };

            let rows: Vec<ResultRow> = result
                .into_iter()
                .map(|row| {
                    let col_count = row.columns_ref().len();
                    let cells = (0..col_count)
                        .map(|i| {
                            let val: Option<Value> = row.get(i);
                            match val {
                                None | Some(Value::NULL) => CellValue::Null,
                                Some(Value::Int(n)) => CellValue::Integer(n),
                                Some(Value::UInt(n)) => CellValue::Integer(n as i64),
                                Some(Value::Float(n)) => CellValue::Float(n as f64),
                                Some(Value::Double(n)) => CellValue::Float(n),
                                Some(Value::Bytes(b)) => match String::from_utf8(b.clone()) {
                                    Ok(s) => CellValue::String(s),
                                    Err(_) => CellValue::Bytes(b),
                                },
                                Some(Value::Date(y, m, d, h, mi, s, _us)) => {
                                    CellValue::DateTime(format!(
                                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                                        y, m, d, h, mi, s
                                    ))
                                }
                                Some(Value::Time(neg, d, h, m, s, _us)) => {
                                    let sign = if neg { "-" } else { "" };
                                    CellValue::DateTime(format!(
                                        "{}{}:{:02}:{:02}",
                                        sign,
                                        d * 24 + h as u32,
                                        m,
                                        s
                                    ))
                                }
                            }
                        })
                        .collect();
                    ResultRow { cells }
                })
                .collect();

            let affected_rows = if rows.is_empty() { affected } else { rows.len() as u64 };

            Ok(QueryResult {
                columns,
                rows,
                execution_time_ms,
                affected_rows,
            })
        })
    }

    pub fn disconnect(self) {
        let pool = self.pool;
        let _ = self.runtime.block_on(async move { pool.disconnect().await });
    }
}
