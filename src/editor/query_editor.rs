use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{CompletionProvider, Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Disableable, Icon, IconName};

use crate::db::connection::DatabaseService;
use crate::db::schema::DatabaseSchema;
use crate::db::types::QueryResult;
use super::completion::SqlCompletionProvider;
use super::saved_queries::{self, HistoryEntry, SavedQuery};

actions!(query_editor, [ExecuteQuery, NewTab, CloseTab, SaveQuery, DismissPanel, HistoryPrev, HistoryNext]);

#[derive(Clone)]
pub enum QueryEvent {
    QueryExecuted(QueryResult, String),
    #[allow(dead_code)]
    QueryError(String),
}

struct QueryTab {
    id: usize,
    title: String,
    sql: String,
}

pub struct QueryEditor {
    focus_handle: FocusHandle,
    sql_input: Entity<InputState>,
    completion_provider: Rc<SqlCompletionProvider>,
    db: Option<Arc<DatabaseService>>,
    error_message: Option<String>,
    is_executing: bool,
    pending_sql: Option<String>,
    tabs: Vec<QueryTab>,
    active_tab: usize,
    next_tab_id: usize,
    saved_queries: Vec<SavedQuery>,
    show_saved: bool,
    history: Vec<HistoryEntry>,
    show_history: bool,
    history_cursor: Option<usize>,
    pre_history_sql: Option<String>,
}

impl QueryEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let completion_provider = SqlCompletionProvider::new();

        let sql_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .code_editor("sql")
                .placeholder("Enter SQL query...");
            state.lsp.completion_provider = Some(completion_provider.clone() as Rc<dyn CompletionProvider>);
            state
        });

        cx.subscribe(&sql_input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { secondary: true }) {
                // Undo the newline the Input just inserted
                let current = this.sql_input.read(cx).value().to_string();
                let cleaned = current.trim().to_string();
                this.pending_sql = Some(cleaned.clone());
                if let Some(tab) = this.tabs.get_mut(this.active_tab) {
                    tab.sql = cleaned;
                }
                this.execute(cx);
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            sql_input,
            completion_provider,
            db: None,
            error_message: None,
            is_executing: false,
            pending_sql: None,
            tabs: vec![QueryTab {
                id: 1,
                title: "Query 1".to_string(),
                sql: String::new(),
            }],
            active_tab: 0,
            next_tab_id: 2,
            saved_queries: saved_queries::load_queries(),
            show_saved: false,
            history: saved_queries::load_history(),
            show_history: false,
            history_cursor: None,
            pre_history_sql: None,
        }
    }

    pub fn set_connection(&mut self, db: Arc<DatabaseService>) {
        self.db = Some(db);
    }

    pub fn set_schema(&mut self, schema: DatabaseSchema) {
        self.completion_provider.set_schema(schema);
    }

    pub fn set_sql(&mut self, sql: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.sql = sql.to_string();
        }
        self.pending_sql = Some(sql.to_string());
        cx.notify();
    }

    pub fn execute(&mut self, cx: &mut Context<Self>) {
        let Some(db) = self.db.clone() else {
            self.error_message = Some("Not connected to a database".to_string());
            cx.notify();
            return;
        };

        let sql = self.sql_input.read(cx).value().to_string();
        if sql.trim().is_empty() {
            return;
        }

        self.is_executing = true;
        self.error_message = None;
        self.history_cursor = None;
        self.pre_history_sql = None;
        cx.notify();

        let sql_for_history = sql.clone();
        let timestamp = saved_queries::now_timestamp();

        cx.spawn(async move |this, cx| {
            let result: Result<crate::db::types::QueryResult, anyhow::Error> = cx
                .background_executor()
                .spawn(async move { db.execute(&sql) })
                .await;

            this.update(cx, |this, cx| {
                this.is_executing = false;
                let success = result.is_ok();
                match result {
                    Ok(qr) => {
                        cx.emit(QueryEvent::QueryExecuted(qr, sql_for_history.clone()));
                    }
                    Err(e) => {
                        this.error_message = Some(e.to_string());
                        cx.emit(QueryEvent::QueryError(e.to_string()));
                    }
                }

                // Record in history
                let entry = HistoryEntry {
                    sql: sql_for_history,
                    timestamp,
                    success,
                };
                this.history.insert(0, entry.clone());
                this.history.truncate(200);
                let _ = saved_queries::append_history(entry);

                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_active_tab(&mut self, cx: &App) {
        let sql = self.sql_input.read(cx).value().to_string();
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.sql = sql;
        }
    }

    fn switch_to_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx == self.active_tab || idx >= self.tabs.len() {
            return;
        }
        self.save_active_tab(cx);
        self.active_tab = idx;
        self.pending_sql = Some(self.tabs[idx].sql.clone());
        self.error_message = None;
        self.history_cursor = None;
        self.pre_history_sql = None;
        cx.notify();
    }

    fn add_tab(&mut self, cx: &mut Context<Self>) {
        self.save_active_tab(cx);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(QueryTab {
            id,
            title: format!("Query {}", id),
            sql: String::new(),
        });
        self.active_tab = self.tabs.len() - 1;
        self.pending_sql = Some(String::new());
        self.error_message = None;
        cx.notify();
    }

    fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        // Save current tab before removing
        self.save_active_tab(cx);
        self.tabs.remove(idx);
        // Adjust active tab index
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if idx < self.active_tab {
            self.active_tab -= 1;
        } else if idx == self.active_tab {
            // We removed the active tab; clamp and load the new active
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
        self.pending_sql = Some(self.tabs[self.active_tab].sql.clone());
        self.error_message = None;
        cx.notify();
    }

    fn on_execute(&mut self, _: &ExecuteQuery, _window: &mut Window, cx: &mut Context<Self>) {
        self.execute(cx);
    }

    fn on_new_tab(&mut self, _: &NewTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.add_tab(cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active_tab, cx);
    }

    fn on_save_query(&mut self, _: &SaveQuery, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_current_query(cx);
    }

    fn on_dismiss_panel(&mut self, _: &DismissPanel, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_saved = false;
        self.show_history = false;
        cx.notify();
    }

    fn on_history_prev(&mut self, _: &HistoryPrev, _window: &mut Window, cx: &mut Context<Self>) {
        if self.history.is_empty() {
            return;
        }
        let new_cursor = match self.history_cursor {
            None => {
                // Save current editor text before entering history
                let current = self.sql_input.read(cx).value().to_string();
                self.pre_history_sql = Some(current);
                0
            }
            Some(c) => (c + 1).min(self.history.len() - 1),
        };
        self.history_cursor = Some(new_cursor);
        let sql = self.history[new_cursor].sql.clone();
        self.pending_sql = Some(sql.clone());
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.sql = sql;
        }
        cx.notify();
    }

    fn on_history_next(&mut self, _: &HistoryNext, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        if cursor == 0 {
            // Restore the pre-history text
            let restored = self.pre_history_sql.take().unwrap_or_default();
            self.history_cursor = None;
            self.pending_sql = Some(restored.clone());
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.sql = restored;
            }
        } else {
            let new_cursor = cursor - 1;
            self.history_cursor = Some(new_cursor);
            let sql = self.history[new_cursor].sql.clone();
            self.pending_sql = Some(sql.clone());
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.sql = sql;
            }
        }
        cx.notify();
    }

    fn save_current_query(&mut self, cx: &mut Context<Self>) {
        let sql = self.sql_input.read(cx).value().to_string();
        if sql.trim().is_empty() {
            return;
        }
        let name = if let Some(tab) = self.tabs.get(self.active_tab) {
            tab.title.clone()
        } else {
            "Untitled".to_string()
        };
        let query = SavedQuery {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            sql,
        };
        self.saved_queries.push(query);
        let _ = saved_queries::save_queries(&self.saved_queries);
        cx.notify();
    }

    fn load_saved_query(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(query) = self.saved_queries.get(idx) {
            self.pending_sql = Some(query.sql.clone());
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.sql = query.sql.clone();
            }
            self.show_saved = false;
            cx.notify();
        }
    }

    fn delete_saved_query(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.saved_queries.len() {
            self.saved_queries.remove(idx);
            let _ = saved_queries::save_queries(&self.saved_queries);
            cx.notify();
        }
    }

    fn load_history_entry(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.history.get(idx) {
            self.pending_sql = Some(entry.sql.clone());
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.sql = entry.sql.clone();
            }
            self.show_history = false;
            cx.notify();
        }
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.history.clear();
        let _ = saved_queries::clear_history();
        cx.notify();
    }

    fn format_timestamp(ts: &str) -> String {
        let secs: u64 = ts.parse().unwrap_or(0);
        if secs == 0 {
            return "unknown".to_string();
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let diff = now.saturating_sub(secs);
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }
}

impl EventEmitter<QueryEvent> for QueryEditor {}

impl Focusable for QueryEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for QueryEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Apply pending SQL
        if let Some(sql) = self.pending_sql.take() {
            self.sql_input
                .update(cx, |input, cx| input.set_value(&sql, window, cx));
        }

        let theme = cx.theme();
        let has_db = self.db.is_some();
        let is_executing = self.is_executing;
        let error = self.error_message.clone();
        let bg = theme.background;
        let surface = theme.secondary;
        let border_color = theme.border;
        let text_color = theme.foreground;
        let muted = theme.muted_foreground;
        let error_color = theme.danger;
        let active_tab = self.active_tab;

        // Build tab bar
        let mut tab_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(32.))
            .bg(bg)
            .px_1()
            .gap(px(1.));

        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = idx == active_tab;
            let tab_id = tab.id;
            let title = tab.title.clone();
            let can_close = self.tabs.len() > 1;

            let mut tab_el = div()
                .id(ElementId::Name(format!("tab-{}", tab_id).into()))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_size(px(12.))
                .when(is_active, |el| el.bg(surface).text_color(text_color))
                .when(!is_active, |el| {
                    el.text_color(muted)
                        .hover(|el| el.bg(surface.opacity(0.5)))
                })
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.switch_to_tab(idx, cx);
                }))
                .child(title);

            if can_close {
                tab_el = tab_el.child(
                    div()
                        .id(ElementId::Name(format!("close-tab-{}", tab_id).into()))
                        .cursor_pointer()
                        .text_size(px(10.))
                        .text_color(muted)
                        .hover(|el| el.text_color(text_color))
                        .rounded_sm()
                        .px(px(2.))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.close_tab(idx, cx);
                        }))
                        .child("×"),
                );
            }

            tab_bar = tab_bar.child(tab_el);
        }

        // Add tab button
        tab_bar = tab_bar.child(
            div()
                .id("add-tab")
                .cursor_pointer()
                .px_2()
                .py_1()
                .text_size(px(12.))
                .text_color(muted)
                .hover(|el| el.text_color(text_color))
                .rounded_md()
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.add_tab(cx);
                }))
                .child("+"),
        );

        // Right-aligned tab bar actions
        tab_bar = tab_bar
            .child(div().flex_1())
            .child(
                Button::new("history-btn")
                    .ghost()
                    .compact()
                    .icon(Icon::new(IconName::GalleryVerticalEnd))
                    .tooltip("Query History")
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.show_history = !this.show_history;
                        this.show_saved = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("saved-queries-btn")
                    .ghost()
                    .compact()
                    .icon(Icon::new(IconName::BookOpen))
                    .tooltip("Saved Queries")
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.show_saved = !this.show_saved;
                        this.show_history = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("save-query-btn")
                    .ghost()
                    .compact()
                    .icon(Icon::new(IconName::Star))
                    .tooltip("Save Query")
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.save_current_query(cx);
                    })),
            );

        let show_saved = self.show_saved;
        let show_history = self.show_history;
        let saved_count = self.saved_queries.len();
        let history_count = self.history.len();

        div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .overflow_hidden()
            .relative()
            .bg(surface)
            .border_b_1()
            .border_color(border_color)
            .key_context("QueryEditor")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::on_execute))
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_save_query))
            .on_action(cx.listener(Self::on_dismiss_panel))
            .on_action(cx.listener(Self::on_history_prev))
            .on_action(cx.listener(Self::on_history_next))
            // Tab bar
            .child(tab_bar)
            // Multi-line SQL editor
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(Input::new(&self.sql_input).h_full().rounded_none().bordered(false).focus_bordered(false)),
            )
            // Saved queries dropdown panel
            .when(show_saved, |el| {
                el.child(
                    div()
                        .absolute()
                        .right(px(8.))
                        .top(px(34.))
                        .w(px(300.))
                        .max_h(px(300.))
                        .overflow_hidden()
                        .bg(bg)
                        .border_1()
                        .border_color(border_color)
                        .rounded_md()
                        .shadow_md()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px_3()
                                .py_2()
                                .border_b_1()
                                .border_color(border_color)
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(text_color)
                                        .child("Saved Queries"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(muted)
                                        .child(format!("{} saved", saved_count)),
                                ),
                        )
                        .children(
                            self.saved_queries
                                .iter()
                                .enumerate()
                                .map(|(idx, query)| {
                                    let sql_preview = if query.sql.len() > 60 {
                                        format!("{}...", &query.sql[..60])
                                    } else {
                                        query.sql.clone()
                                    };
                                    div()
                                        .id(ElementId::Name(
                                            format!("saved-q-{}", idx).into(),
                                        ))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .px_3()
                                        .py_2()
                                        .cursor_pointer()
                                        .border_b_1()
                                        .border_color(border_color)
                                        .hover(|el| el.bg(surface))
                                        .on_click(cx.listener(
                                            move |this, _, _window, cx| {
                                                this.load_saved_query(idx, cx);
                                            },
                                        ))
                                        .child(
                                            div()
                                                .flex_1()
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .font_weight(
                                                            FontWeight::MEDIUM,
                                                        )
                                                        .text_color(text_color)
                                                        .child(query.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(muted)
                                                        .text_ellipsis()
                                                        .overflow_x_hidden()
                                                        .child(sql_preview),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id(ElementId::Name(
                                                    format!("del-saved-{}", idx)
                                                        .into(),
                                                ))
                                                .cursor_pointer()
                                                .text_size(px(10.))
                                                .text_color(muted)
                                                .hover(|el| {
                                                    el.text_color(error_color)
                                                })
                                                .rounded_sm()
                                                .px(px(4.))
                                                .py(px(2.))
                                                .on_click(cx.listener(
                                                    move |this, _, _window, cx| {
                                                        this.delete_saved_query(
                                                            idx, cx,
                                                        );
                                                    },
                                                ))
                                                .child("×"),
                                        )
                                })
                                .collect::<Vec<_>>(),
                        )
                        .when(saved_count == 0, |el: Div| {
                            el.child(
                                div()
                                    .p_3()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child("No saved queries yet. Click the star to save the current query."),
                            )
                        }),
                )
            })
            // History dropdown panel
            .when(show_history, |el| {
                el.child(
                    div()
                        .absolute()
                        .right(px(8.))
                        .top(px(34.))
                        .w(px(360.))
                        .max_h(px(350.))
                        .overflow_hidden()
                        .bg(bg)
                        .border_1()
                        .border_color(border_color)
                        .rounded_md()
                        .shadow_md()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px_3()
                                .py_2()
                                .border_b_1()
                                .border_color(border_color)
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(text_color)
                                        .child("Query History"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(muted)
                                                .child(format!("{} queries", history_count)),
                                        )
                                        .when(history_count > 0, |el| {
                                            el.child(
                                                div()
                                                    .id("clear-history")
                                                    .cursor_pointer()
                                                    .text_size(px(11.))
                                                    .text_color(muted)
                                                    .hover(|el| el.text_color(error_color))
                                                    .child("Clear")
                                                    .on_click(cx.listener(
                                                        |this, _, _window, cx| {
                                                            this.clear_history(cx);
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                        .children(
                            self.history
                                .iter()
                                .take(50)
                                .enumerate()
                                .map(|(idx, entry)| {
                                    let sql_preview = if entry.sql.len() > 80 {
                                        format!("{}...", &entry.sql[..80])
                                    } else {
                                        entry.sql.clone()
                                    };
                                    let time_str =
                                        Self::format_timestamp(&entry.timestamp);
                                    let success = entry.success;
                                    div()
                                        .id(ElementId::Name(
                                            format!("hist-{}", idx).into(),
                                        ))
                                        .flex()
                                        .flex_row()
                                        .items_start()
                                        .gap(px(8.))
                                        .px_3()
                                        .py_2()
                                        .cursor_pointer()
                                        .border_b_1()
                                        .border_color(border_color)
                                        .hover(|el| el.bg(surface))
                                        .on_click(cx.listener(
                                            move |this, _, _window, cx| {
                                                this.load_history_entry(idx, cx);
                                            },
                                        ))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .w(px(6.))
                                                .h(px(6.))
                                                .mt(px(5.))
                                                .rounded_full()
                                                .bg(if success {
                                                    gpui::rgb(0x4ade80).into()
                                                } else {
                                                    error_color
                                                }),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(text_color)
                                                        .text_ellipsis()
                                                        .overflow_x_hidden()
                                                        .child(sql_preview),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.))
                                                        .text_color(muted)
                                                        .child(time_str),
                                                ),
                                        )
                                })
                                .collect::<Vec<_>>(),
                        )
                        .when(history_count == 0, |el: Div| {
                            el.child(
                                div()
                                    .p_3()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child(
                                        "No query history yet. Execute a query to start building history.",
                                    ),
                            )
                        }),
                )
            })
            // Toolbar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(36.))
                    .px_2()
                    .border_t_1()
                    .border_color(border_color)
                    .bg(bg)
                    .when_some(error, |el, err| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(error_color)
                                .overflow_hidden()
                                .text_ellipsis()
                                .max_w(px(400.))
                                .child(err),
                        )
                    })
                    .child(
                        div().flex_1()
                    )
                    .child(
                        Button::new("execute-btn")
                            .primary()
                            .compact()
                            .label(if is_executing { "Running..." } else { "Execute" })
                            .disabled(!has_db || is_executing)
                            .loading(is_executing)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.execute(cx);
                            })),
                    ),
            )
    }
}
