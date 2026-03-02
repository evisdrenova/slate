use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Disableable};

use crate::db::connection::DatabaseService;
use crate::db::types::QueryResult;

actions!(query_editor, [ExecuteQuery]);

#[derive(Clone)]
pub enum QueryEvent {
    QueryExecuted(QueryResult),
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
    db: Option<Arc<DatabaseService>>,
    error_message: Option<String>,
    is_executing: bool,
    pending_sql: Option<String>,
    tabs: Vec<QueryTab>,
    active_tab: usize,
    next_tab_id: usize,
}

impl QueryEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            sql_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("sql")
                    .placeholder("Enter SQL query...")
            }),
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
        }
    }

    pub fn set_connection(&mut self, db: Arc<DatabaseService>) {
        self.db = Some(db);
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
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result: Result<crate::db::types::QueryResult, anyhow::Error> = cx
                .background_executor()
                .spawn(async move { db.execute(&sql) })
                .await;

            this.update(cx, |this, cx| {
                this.is_executing = false;
                match result {
                    Ok(qr) => {
                        cx.emit(QueryEvent::QueryExecuted(qr));
                    }
                    Err(e) => {
                        this.error_message = Some(e.to_string());
                        cx.emit(QueryEvent::QueryError(e.to_string()));
                    }
                }
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
            .border_b_1()
            .border_color(border_color)
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
                .rounded_t_md()
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

        div()
            .flex()
            .flex_col()
            .w_full()
            .flex_1()
            .overflow_hidden()
            .bg(surface)
            .border_b_1()
            .border_color(border_color)
            .key_context("QueryEditor")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::on_execute))
            // Tab bar
            .child(tab_bar)
            // Multi-line SQL editor
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(Input::new(&self.sql_input).h_full()),
            )
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
                    )
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
                    }),
            )
    }
}
