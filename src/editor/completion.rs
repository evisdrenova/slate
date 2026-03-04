use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use gpui::{Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, RopeExt};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse,
    CompletionTextEdit, TextEdit,
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

    /// Extract the word (including dots) before the cursor.
    /// Returns (word, has_dot).
    fn extract_word_before_cursor(text: &Rope, offset: usize) -> (String, bool) {
        let text_str = text.to_string();
        let clamped = offset.min(text_str.len());
        let safe_offset = if text_str.is_char_boundary(clamped) {
            clamped
        } else {
            let mut o = clamped;
            while o > 0 && !text_str.is_char_boundary(o) {
                o -= 1;
            }
            o
        };
        let before_cursor = &text_str[..safe_offset];

        let word: String = before_cursor
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let has_dot = word.contains('.');
        (word, has_dot)
    }

    /// Build a CompletionItem with an explicit text_edit so the replacement
    /// range is always correct, regardless of the menu's stale trigger_start_offset.
    fn make_item(
        label: String,
        kind: CompletionItemKind,
        detail: Option<String>,
        edit_range: lsp_types::Range,
    ) -> CompletionItem {
        CompletionItem {
            label: label.clone(),
            kind: Some(kind),
            detail,
            // filter_text = label prevents the highlight range (0..filter_text.len())
            // from exceeding the label length when the menu query grows.
            filter_text: Some(label.clone()),
            // Explicit text_edit overrides the menu's trigger_start_offset..cursor
            // default, which can be stale and eat too much text.
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: edit_range,
                new_text: label,
            })),
            ..Default::default()
        }
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

        let end_pos = text.offset_to_position(offset);

        let mut items: Vec<CompletionItem> = Vec::new();
        let schema = self.schema.borrow();

        if has_dot {
            // Dot-completion: "tablename.col" -> suggest that table's columns
            let parts: Vec<&str> = word.splitn(2, '.').collect();
            let table_name = parts[0].to_lowercase();
            let col_prefix = parts.get(1).map(|s| s.to_string()).unwrap_or_default();

            // Replace only the part after the dot
            let replace_start = offset - col_prefix.len();
            let start_pos = text.offset_to_position(replace_start);
            let edit_range = lsp_types::Range {
                start: start_pos,
                end: end_pos,
            };

            if let Some(ref schema) = *schema {
                for table in &schema.tables {
                    if table.name.to_lowercase() == table_name {
                        for col in &table.columns {
                            if col_prefix.is_empty()
                                || col.name.to_lowercase().starts_with(&col_prefix.to_lowercase())
                            {
                                items.push(Self::make_item(
                                    col.name.clone(),
                                    CompletionItemKind::FIELD,
                                    Some(col.data_type.clone()),
                                    edit_range,
                                ));
                            }
                        }
                        break;
                    }
                }
            }
        } else {
            let prefix = word.to_lowercase();

            // Replace the current word
            let replace_start = offset - word.len();
            let start_pos = text.offset_to_position(replace_start);
            let edit_range = lsp_types::Range {
                start: start_pos,
                end: end_pos,
            };

            // SQL keywords
            for &kw in SQL_KEYWORDS {
                if kw.to_lowercase().starts_with(&prefix) {
                    items.push(Self::make_item(
                        kw.to_string(),
                        CompletionItemKind::KEYWORD,
                        None,
                        edit_range,
                    ));
                }
            }

            // Table names
            if let Some(ref schema) = *schema {
                for table in &schema.tables {
                    if table.name.to_lowercase().starts_with(&prefix) {
                        items.push(Self::make_item(
                            table.name.clone(),
                            CompletionItemKind::CLASS,
                            Some(format!("{} columns", table.columns.len())),
                            edit_range,
                        ));
                    }
                }

                // Column names (all tables)
                for table in &schema.tables {
                    for col in &table.columns {
                        if col.name.to_lowercase().starts_with(&prefix) {
                            items.push(Self::make_item(
                                col.name.clone(),
                                CompletionItemKind::FIELD,
                                Some(format!("{}.{}", table.name, col.data_type)),
                                edit_range,
                            ));
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
            ch.is_alphanumeric() || ch == '_'
        } else {
            false
        }
    }
}
