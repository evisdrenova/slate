use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use gpui::{Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, RopeExt};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse,
};
use ropey::Rope;

use crate::db::schema::DatabaseSchema;

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS", "NULL",
    "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE",
    "DROP", "ALTER", "ADD", "COLUMN", "INDEX", "JOIN", "INNER", "LEFT",
    "RIGHT", "OUTER", "FULL", "CROSS", "ON", "AS", "ORDER", "BY", "GROUP",
    "HAVING", "LIMIT", "OFFSET", "DISTINCT", "UNION", "ALL", "EXISTS",
    "BETWEEN", "LIKE", "CASE", "WHEN", "THEN", "ELSE", "END", "ASC", "DESC",
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "CAST", "IF",
    "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "CONSTRAINT", "UNIQUE",
    "DEFAULT", "AUTO_INCREMENT", "SERIAL", "VARCHAR", "INT", "INTEGER",
    "BIGINT", "TEXT", "BOOLEAN", "DATE", "TIMESTAMP", "FLOAT", "DOUBLE",
    "DECIMAL", "TRUNCATE", "EXPLAIN", "ANALYZE", "WITH", "RECURSIVE",
];

pub struct SqlCompletionProvider {
    schema: RefCell<Option<DatabaseSchema>>,
}

impl SqlCompletionProvider {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            schema: RefCell::new(None),
        })
    }

    pub fn set_schema(&self, schema: DatabaseSchema) {
        *self.schema.borrow_mut() = Some(schema);
    }

    fn extract_word_before_cursor(text: &Rope, offset: usize) -> (String, bool) {
        // Walk backwards from offset to find the current word
        // Returns (word, has_dot_prefix) where has_dot_prefix means "tablename." pattern
        if offset == 0 {
            return (String::new(), false);
        }

        let mut start = offset;

        // Walk backwards collecting word chars
        while start > 0 {
            let prev = start - 1;
            if let Some(ch) = text.char_at(prev) {
                if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                    start = prev;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let word: String = (start..offset)
            .filter_map(|i| text.char_at(i))
            .collect();

        let has_dot = word.contains('.');
        (word, has_dot)
    }
}

impl CompletionProvider for SqlCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let (word, has_dot) = Self::extract_word_before_cursor(text, offset);

        if word.is_empty() {
            return Task::ready(Ok(CompletionResponse::Array(vec![])));
        }

        let mut items: Vec<CompletionItem> = Vec::new();
        let schema = self.schema.borrow();

        if has_dot {
            // Dot-completion: "tablename." -> suggest that table's columns
            let parts: Vec<&str> = word.splitn(2, '.').collect();
            let table_name = parts[0].to_lowercase();
            let col_prefix = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

            if let Some(ref schema) = *schema {
                for table in &schema.tables {
                    if table.name.to_lowercase() == table_name {
                        for col in &table.columns {
                            if col_prefix.is_empty()
                                || col.name.to_lowercase().starts_with(&col_prefix)
                            {
                                items.push(CompletionItem {
                                    label: col.name.clone(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(col.data_type.clone()),
                                    ..Default::default()
                                });
                            }
                        }
                        break;
                    }
                }
            }
        } else {
            let prefix = word.to_lowercase();

            // SQL keywords
            for &kw in SQL_KEYWORDS {
                if kw.to_lowercase().starts_with(&prefix) {
                    items.push(CompletionItem {
                        label: kw.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        ..Default::default()
                    });
                }
            }

            // Table names
            if let Some(ref schema) = *schema {
                for table in &schema.tables {
                    if table.name.to_lowercase().starts_with(&prefix) {
                        items.push(CompletionItem {
                            label: table.name.clone(),
                            kind: Some(CompletionItemKind::CLASS),
                            detail: Some(format!("{} columns", table.columns.len())),
                            ..Default::default()
                        });
                    }
                }

                // Column names (all tables)
                for table in &schema.tables {
                    for col in &table.columns {
                        if col.name.to_lowercase().starts_with(&prefix) {
                            items.push(CompletionItem {
                                label: col.name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(format!("{}.{}", table.name, col.data_type)),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        // Deduplicate by label
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);

        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(
        &self,
        _offset: usize,
        new_text: &str,
        _cx: &mut Context<InputState>,
    ) -> bool {
        if let Some(ch) = new_text.chars().last() {
            ch.is_alphanumeric() || ch == '_' || ch == '.'
        } else {
            false
        }
    }
}
