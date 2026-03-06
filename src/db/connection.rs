use std::sync::Arc;

use anyhow::Result;
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Pool, Row, Value};
use tokio::runtime::Runtime;
use tokio_postgres::types::Type;

use super::types::*;

enum DatabaseBackend {
    MySQL(Pool),
    Postgres(Arc<tokio_postgres::Client>),
}

pub struct DatabaseService {
    backend: DatabaseBackend,
    runtime: Arc<Runtime>,
    db_type: DbType,
}

impl DatabaseService {
    pub fn db_type(&self) -> DbType {
        self.db_type
    }

    pub fn connect(config: &ConnectionConfig, password: &str) -> Result<Self> {
        let runtime = Arc::new(Runtime::new()?);

        match config.db_type {
            DbType::MySQL => Self::connect_mysql(config, password, runtime),
            DbType::PostgreSQL => Self::connect_postgres(config, password, runtime),
        }
    }

    fn connect_mysql(
        config: &ConnectionConfig,
        password: &str,
        runtime: Arc<Runtime>,
    ) -> Result<Self> {
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

        Ok(Self {
            backend: DatabaseBackend::MySQL(pool),
            runtime,
            db_type: DbType::MySQL,
        })
    }

    fn connect_postgres(
        config: &ConnectionConfig,
        password: &str,
        runtime: Arc<Runtime>,
    ) -> Result<Self> {
        // Use percent-encoded URI format to safely handle special characters in passwords
        let encode = |s: &str| -> String {
            let mut out = String::with_capacity(s.len());
            for b in s.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        out.push(b as char);
                    }
                    _ => {
                        out.push_str(&format!("%{:02X}", b));
                    }
                }
            }
            out
        };
        let conn_str = format!(
            "postgresql://{}:{}@{}:{}/{}",
            encode(&config.user),
            encode(password),
            &config.host,
            config.port,
            encode(&config.database),
        );

        let client = runtime.block_on(async {
            // Try TLS first (required by most cloud providers like Supabase)
            let tls_result: Result<tokio_postgres::Client, anyhow::Error> = async {
                let mut root_store = rustls::RootCertStore::empty();
                root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

                let tls_config = rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth();

                let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
                let (client, connection) = tokio_postgres::connect(&conn_str, tls).await?;

                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        eprintln!("Postgres connection error: {}", e);
                    }
                });

                client.query("SELECT 1", &[]).await?;
                Ok(client)
            }
            .await;

            match tls_result {
                Ok(client) => Ok(client),
                Err(tls_err) => {
                    // Fall back to plain connection for local/dev databases
                    match tokio_postgres::connect(&conn_str, tokio_postgres::NoTls).await {
                        Ok((client, connection)) => {
                            tokio::spawn(async move {
                                if let Err(e) = connection.await {
                                    eprintln!("Postgres connection error: {}", e);
                                }
                            });
                            client.query("SELECT 1", &[]).await?;
                            Ok(client)
                        }
                        Err(plain_err) => Err(anyhow::anyhow!(
                            "TLS: {} | Plain: {}",
                            tls_err,
                            plain_err
                        )),
                    }
                }
            }
        })?;

        Ok(Self {
            backend: DatabaseBackend::Postgres(Arc::new(client)),
            runtime,
            db_type: DbType::PostgreSQL,
        })
    }

    pub fn execute(&self, sql: &str) -> Result<QueryResult> {
        match &self.backend {
            DatabaseBackend::MySQL(pool) => self.execute_mysql(pool, sql),
            DatabaseBackend::Postgres(client) => self.execute_postgres(client, sql),
        }
    }

    fn execute_mysql(&self, pool: &Pool, sql: &str) -> Result<QueryResult> {
        let pool = pool.clone();
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

    fn execute_postgres(
        &self,
        client: &Arc<tokio_postgres::Client>,
        sql: &str,
    ) -> Result<QueryResult> {
        let client = client.clone();
        let sql = sql.to_string();

        self.runtime.block_on(async move {
            let start = std::time::Instant::now();
            let rows = client.query(&sql as &str, &[]).await?;
            let execution_time_ms = start.elapsed().as_millis();

            let columns: Vec<ResultColumn> = if let Some(first_row) = rows.first() {
                first_row
                    .columns()
                    .iter()
                    .map(|col| ResultColumn {
                        name: col.name().to_string(),
                        type_name: col.type_().name().to_string(),
                    })
                    .collect()
            } else {
                vec![]
            };

            let result_rows: Vec<ResultRow> = rows
                .iter()
                .map(|row| {
                    let cells = row
                        .columns()
                        .iter()
                        .enumerate()
                        .map(|(i, col)| pg_value_to_cell(row, i, col.type_()))
                        .collect();
                    ResultRow { cells }
                })
                .collect();

            let affected_rows = result_rows.len() as u64;

            Ok(QueryResult {
                columns,
                rows: result_rows,
                execution_time_ms,
                affected_rows,
            })
        })
    }

    #[allow(dead_code)]
    pub fn disconnect(self) {
        match self.backend {
            DatabaseBackend::MySQL(pool) => {
                let _ = self.runtime.block_on(async move { pool.disconnect().await });
            }
            DatabaseBackend::Postgres(_) => {
                // Client is dropped automatically; connection task will end
            }
        }
    }
}

fn pg_value_to_cell(row: &tokio_postgres::Row, idx: usize, col_type: &Type) -> CellValue {
    // Use Option<T> to handle NULLs — tokio-postgres returns None for SQL NULL
    match *col_type {
        Type::BOOL => match row.try_get::<_, Option<bool>>(idx) {
            Ok(Some(v)) => CellValue::Boolean(v),
            _ => CellValue::Null,
        },
        Type::INT2 => match row.try_get::<_, Option<i16>>(idx) {
            Ok(Some(v)) => CellValue::Integer(v as i64),
            _ => CellValue::Null,
        },
        Type::INT4 => match row.try_get::<_, Option<i32>>(idx) {
            Ok(Some(v)) => CellValue::Integer(v as i64),
            _ => CellValue::Null,
        },
        Type::INT8 => match row.try_get::<_, Option<i64>>(idx) {
            Ok(Some(v)) => CellValue::Integer(v),
            _ => CellValue::Null,
        },
        Type::FLOAT4 => match row.try_get::<_, Option<f32>>(idx) {
            Ok(Some(v)) => CellValue::Float(v as f64),
            _ => CellValue::Null,
        },
        Type::FLOAT8 => match row.try_get::<_, Option<f64>>(idx) {
            Ok(Some(v)) => CellValue::Float(v),
            _ => CellValue::Null,
        },
        Type::NUMERIC => {
            // NUMERIC doesn't impl FromSql for f64 by default, use string
            match row.try_get::<_, Option<String>>(idx) {
                Ok(Some(s)) => CellValue::String(s),
                _ => CellValue::Null,
            }
        }
        Type::BYTEA => match row.try_get::<_, Option<Vec<u8>>>(idx) {
            Ok(Some(v)) => CellValue::Bytes(v),
            _ => CellValue::Null,
        },
        _ => {
            // Fallback: try to get as optional string
            match row.try_get::<_, Option<String>>(idx) {
                Ok(Some(s)) => CellValue::String(s),
                _ => CellValue::Null,
            }
        }
    }
}
