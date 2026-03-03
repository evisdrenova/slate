use std::collections::HashSet;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme;

use crate::db::connection::DatabaseService;
use crate::db::schema::{self, DatabaseSchema};
use crate::db::types::ConnectionConfig;

#[derive(Clone)]
pub enum SchemaEvent {
    TableSelected(String),
}

#[derive(Clone)]
enum TreeRow {
    Connection(String),
    Database(String),
    Table(String),
    Column(String, String, String, bool, bool), // table, name, type, nullable, pk
    IndexHeader(String),
    Index(String, String, String, bool), // table, name, cols_str, unique
}

pub struct SchemaExplorer {
    focus_handle: FocusHandle,
    schema: Option<DatabaseSchema>,
    connection_label: Option<String>,
    database_name: Option<String>,
    connection_expanded: bool,
    database_expanded: bool,
    expanded_tables: HashSet<String>,
    flattened_rows: Vec<TreeRow>,
    filter_input: Entity<InputState>,
    filter_text: String,
    loading: bool,
    error: Option<String>,
}

impl SchemaExplorer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables..."));
        Self {
            focus_handle: cx.focus_handle(),
            schema: None,
            connection_label: None,
            database_name: None,
            connection_expanded: true,
            database_expanded: true,
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
        config: &ConnectionConfig,
        cx: &mut Context<Self>,
    ) {
        self.connection_label = Some(format!("{}@{}:{}", config.user, config.host, config.port));
        self.database_name = Some(config.database.clone());
        self.connection_expanded = true;
        self.database_expanded = true;
        self.expanded_tables.clear();
        self.loading = true;
        self.error = None;
        self.rebuild_tree();
        cx.notify();

        let database = config.database.clone();
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
                        this.rebuild_tree();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn rebuild_tree(&mut self) {
        let mut rows = Vec::new();

        if let Some(ref label) = self.connection_label {
            rows.push(TreeRow::Connection(label.clone()));

            if self.connection_expanded {
                if let Some(ref db_name) = self.database_name {
                    rows.push(TreeRow::Database(db_name.clone()));

                    if self.database_expanded {
                        if let Some(ref schema) = self.schema {
                            let filter = self.filter_text.to_lowercase();
                            for table in &schema.tables {
                                if !filter.is_empty()
                                    && !table.name.to_lowercase().contains(&filter)
                                {
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
                        }
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
        let danger = theme.danger;

        let row_count = self.flattened_rows.len();
        let has_connection = self.connection_label.is_some();
        let loading = self.loading;
        let error = self.error.clone();

        let tree_list = uniform_list(
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
                    let hover_bg = theme.secondary;
                    let pk_color: Hsla = gpui::rgb(0xf78c6c).into();

                    range
                        .map(|ix| {
                            let row = &this.flattened_rows[ix];
                            match row {
                                TreeRow::Connection(label) => {
                                    let expanded = this.connection_expanded;
                                    render_expandable_row(
                                        ix,
                                        px(6.),
                                        expanded,
                                        label,
                                        text_color,
                                        muted_color,
                                        hover_bg,
                                        true,
                                        cx.listener(|this, _, _, cx| {
                                            this.connection_expanded =
                                                !this.connection_expanded;
                                            this.rebuild_tree();
                                            cx.notify();
                                        }),
                                    )
                                }
                                TreeRow::Database(name) => {
                                    let expanded = this.database_expanded;
                                    render_expandable_row(
                                        ix,
                                        px(22.),
                                        expanded,
                                        name,
                                        text_color,
                                        muted_color,
                                        hover_bg,
                                        true,
                                        cx.listener(|this, _, _, cx| {
                                            this.database_expanded =
                                                !this.database_expanded;
                                            this.rebuild_tree();
                                            cx.notify();
                                        }),
                                    )
                                }
                                TreeRow::Table(name) => {
                                    let expanded =
                                        this.expanded_tables.contains(name);
                                    let name_toggle = name.clone();
                                    let name_select = name.clone();
                                    render_expandable_row(
                                        ix,
                                        px(38.),
                                        expanded,
                                        name,
                                        text_color,
                                        muted_color,
                                        hover_bg,
                                        false,
                                        cx.listener(move |this, _, _, cx| {
                                            this.toggle_table(
                                                &name_toggle,
                                                cx,
                                            );
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |_this, event: &MouseDownEvent, _, cx| {
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
                                }
                                TreeRow::Column(
                                    _,
                                    name,
                                    data_type,
                                    is_nullable,
                                    is_pk,
                                ) => {
                                    let null_str =
                                        if *is_nullable { "?" } else { "" };
                                    div()
                                        .id(("tree-row", ix))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .h(px(22.))
                                        .pl(px(54.))
                                        .pr_2()
                                        .gap(px(6.))
                                        .text_size(px(12.))
                                        .overflow_x_hidden()
                                        .child(
                                            div()
                                                .flex_1()
                                                .overflow_x_hidden()
                                                .text_color(text_color)
                                                .child(name.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_color(muted_color)
                                                .child(format!(
                                                    "{}{}",
                                                    data_type, null_str
                                                )),
                                        )
                                        .when(*is_pk, |el| {
                                            el.child(
                                                div()
                                                    .flex_shrink_0()
                                                    .text_size(px(10.))
                                                    .text_color(pk_color)
                                                    .font_weight(
                                                        FontWeight::SEMIBOLD,
                                                    )
                                                    .child("PK"),
                                            )
                                        })
                                }
                                TreeRow::IndexHeader(_) => div()
                                    .id(("tree-row", ix))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .pl(px(54.))
                                    .text_size(px(11.))
                                    .text_color(muted_color)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Indexes"),
                                TreeRow::Index(_, name, cols, is_unique) => {
                                    let u = if *is_unique {
                                        " UNIQUE"
                                    } else {
                                        ""
                                    };
                                    div()
                                        .id(("tree-row", ix))
                                        .h(px(20.))
                                        .flex()
                                        .items_center()
                                        .pl(px(66.))
                                        .pr_2()
                                        .text_size(px(11.))
                                        .text_color(muted_color)
                                        .overflow_x_hidden()
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
        .flex_1();

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
            .child(if !has_connection && !loading {
                div()
                    .id("schema-empty")
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Connect to view schema")
                    .into_any_element()
            } else {
                div()
                    .id("schema-content")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(tree_list)
                    .when(loading, |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child("Loading schema..."),
                        )
                    })
                    .when_some(error, |el, err| {
                        el.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(danger)
                                .child(err),
                        )
                    })
                    .into_any_element()
            })
    }
}

fn render_expandable_row(
    ix: usize,
    indent: Pixels,
    expanded: bool,
    label: &str,
    text_color: Hsla,
    muted_color: Hsla,
    hover_bg: Hsla,
    bold: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let arrow = if expanded { "▾" } else { "▸" };
    div()
        .id(("tree-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .h(px(24.))
        .pl(indent)
        .pr_2()
        .gap(px(4.))
        .cursor_pointer()
        .hover(|s| s.bg(hover_bg))
        .on_click(on_click)
        .child(
            div()
                .flex_shrink_0()
                .w(px(12.))
                .text_size(px(9.))
                .text_color(muted_color)
                .child(arrow),
        )
        .child(
            div()
                .flex_1()
                .overflow_x_hidden()
                .text_size(px(13.))
                .text_color(text_color)
                .when(bold, |el| el.font_weight(FontWeight::MEDIUM))
                .child(label.to_string()),
        )
}
