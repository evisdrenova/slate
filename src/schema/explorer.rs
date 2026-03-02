use std::collections::HashSet;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme;

use crate::db::connection::DatabaseService;
use crate::db::schema::{self, DatabaseSchema};

#[derive(Clone)]
pub enum SchemaEvent {
    TableSelected(String),
}

#[derive(Clone)]
enum TreeRow {
    Table(String),
    Column(String, String, String, bool, bool), // table, name, type, nullable, pk
    IndexHeader(String),
    Index(String, String, String, bool), // table, name, cols_str, unique
}

pub struct SchemaExplorer {
    focus_handle: FocusHandle,
    schema: Option<DatabaseSchema>,
    expanded_tables: HashSet<String>,
    flattened_rows: Vec<TreeRow>,
    filter_input: Entity<InputState>,
    filter_text: String,
    loading: bool,
    error: Option<String>,
}

impl SchemaExplorer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables..."));
        Self {
            focus_handle: cx.focus_handle(),
            schema: None,
            expanded_tables: HashSet::new(),
            flattened_rows: Vec::new(),
            filter_input,
            filter_text: String::new(),
            loading: false,
            error: None,
        }
    }

    pub fn load_schema(
        &mut self,
        db: Arc<DatabaseService>,
        database: String,
        cx: &mut Context<Self>,
    ) {
        self.loading = true;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result: Result<DatabaseSchema, anyhow::Error> = cx
                .background_executor()
                .spawn(async move { schema::fetch_schema(&db, &database) })
                .await;

            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(schema) => {
                        this.schema = Some(schema);
                        this.rebuild_tree();
                    }
                    Err(e) => {
                        this.error = Some(e.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn rebuild_tree(&mut self) {
        let Some(schema) = &self.schema else {
            self.flattened_rows.clear();
            return;
        };

        let mut rows = Vec::new();
        let filter = self.filter_text.to_lowercase();

        for table in &schema.tables {
            if !filter.is_empty() && !table.name.to_lowercase().contains(&filter) {
                continue;
            }
            rows.push(TreeRow::Table(table.name.clone()));

            if self.expanded_tables.contains(&table.name) {
                for col in &table.columns {
                    rows.push(TreeRow::Column(
                        table.name.clone(),
                        col.name.clone(),
                        col.data_type.clone(),
                        col.is_nullable,
                        col.is_primary_key,
                    ));
                }
                if !table.indexes.is_empty() {
                    rows.push(TreeRow::IndexHeader(table.name.clone()));
                    for idx in &table.indexes {
                        rows.push(TreeRow::Index(
                            table.name.clone(),
                            idx.name.clone(),
                            idx.columns.join(", "),
                            idx.is_unique,
                        ));
                    }
                }
            }
        }

        self.flattened_rows = rows;
    }

    fn toggle_table(&mut self, table_name: &str, cx: &mut Context<Self>) {
        if self.expanded_tables.contains(table_name) {
            self.expanded_tables.remove(table_name);
        } else {
            self.expanded_tables.insert(table_name.to_string());
        }
        self.rebuild_tree();
        cx.notify();
    }

    fn update_filter(&mut self, cx: &mut Context<Self>) {
        let new_filter = self.filter_input.read(cx).value().to_string();
        if new_filter != self.filter_text {
            self.filter_text = new_filter;
            self.rebuild_tree();
            cx.notify();
        }
    }
}

impl EventEmitter<SchemaEvent> for SchemaExplorer {}

impl Focusable for SchemaExplorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SchemaExplorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_filter(cx);

        let theme = cx.theme();
        let bg = theme.background;
        let border_color = theme.border;
        let muted = theme.muted_foreground;

        let row_count = self.flattened_rows.len();

        div()
            .flex()
            .flex_col()
            .w(px(260.))
            .h_full()
            .bg(bg)
            .border_r_1()
            .border_color(border_color)
            // Header
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(32.))
                    .px_3()
                    .border_b_1()
                    .border_color(border_color)
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Schema Explorer"),
            )
            // Filter
            .child(div().p_2().child(Input::new(&self.filter_input)))
            // Tree content
            .child(if self.loading {
                div()
                    .id("schema-loading")
                    .flex_1()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Loading schema...")
                    .into_any_element()
            } else if let Some(ref error) = self.error {
                div()
                    .id("schema-error")
                    .flex_1()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(theme.danger)
                    .child(error.clone())
                    .into_any_element()
            } else if self.flattened_rows.is_empty() {
                div()
                    .id("schema-empty")
                    .flex_1()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(if self.schema.is_some() {
                        "No tables found"
                    } else {
                        "Connect to view schema"
                    })
                    .into_any_element()
            } else {
                uniform_list(
                    "schema-tree",
                    row_count,
                    cx.processor(
                        |this: &mut Self,
                         range: std::ops::Range<usize>,
                         _window: &mut Window,
                         cx: &mut Context<Self>| {
                            let theme = cx.theme();
                            let text_color = theme.foreground;
                            let muted_color = theme.muted_foreground;
                            let surface_color = theme.secondary;
                            range
                                .map(|ix| {
                                    let row = &this.flattened_rows[ix];
                                    match row {
                                        TreeRow::Table(name) => {
                                            let expanded =
                                                this.expanded_tables.contains(name);
                                            let arrow =
                                                if expanded { "v " } else { "> " };
                                            let name_toggle = name.clone();
                                            let name_select = name.clone();
                                            let label = format!("{}{}", arrow, name);
                                            div()
                                                .id(("tree-row", ix))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .px_2()
                                                .py(px(2.))
                                                .text_size(px(13.))
                                                .text_color(text_color)
                                                .cursor_pointer()
                                                .hover(|s| s.bg(surface_color))
                                                .on_click(cx.listener(
                                                    move |this: &mut Self,
                                                          _,
                                                          _window,
                                                          cx| {
                                                        this.toggle_table(
                                                            &name_toggle,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        move |_this: &mut Self,
                                                              event: &MouseDownEvent,
                                                              _window,
                                                              cx| {
                                                            if event.click_count == 2 {
                                                                cx.emit(
                                                                    SchemaEvent::TableSelected(
                                                                        name_select.clone(),
                                                                    ),
                                                                );
                                                            }
                                                        },
                                                    ),
                                                )
                                                .child(label)
                                        }
                                        TreeRow::Column(
                                            _table,
                                            name,
                                            data_type,
                                            is_nullable,
                                            is_pk,
                                        ) => {
                                            let pk_str =
                                                if *is_pk { " PK" } else { "" };
                                            let null_str =
                                                if *is_nullable { "?" } else { "" };
                                            div()
                                                .id(("tree-row", ix))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .pl(px(24.))
                                                .pr_2()
                                                .py(px(2.))
                                                .text_size(px(12.))
                                                .child(
                                                    div()
                                                        .text_color(text_color)
                                                        .child(name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(muted_color)
                                                        .ml_1()
                                                        .child(format!(
                                                            "{}{}{}",
                                                            data_type, null_str, pk_str
                                                        )),
                                                )
                                        }
                                        TreeRow::IndexHeader(_) => div()
                                            .id(("tree-row", ix))
                                            .pl(px(24.))
                                            .py(px(2.))
                                            .text_size(px(11.))
                                            .text_color(muted_color)
                                            .child("Indexes"),
                                        TreeRow::Index(
                                            _,
                                            name,
                                            cols,
                                            is_unique,
                                        ) => {
                                            let u =
                                                if *is_unique { " UNIQUE" } else { "" };
                                            div()
                                                .id(("tree-row", ix))
                                                .pl(px(36.))
                                                .pr_2()
                                                .py(px(2.))
                                                .text_size(px(11.))
                                                .text_color(muted_color)
                                                .child(format!(
                                                    "{}{} ({})",
                                                    name, u, cols
                                                ))
                                        }
                                    }
                                })
                                .collect()
                        },
                    ),
                )
                .h_full()
                .into_any_element()
            })
    }
}
