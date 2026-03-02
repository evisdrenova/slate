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

pub struct QueryEditor {
    focus_handle: FocusHandle,
    sql_input: Entity<InputState>,
    db: Option<Arc<DatabaseService>>,
    error_message: Option<String>,
    is_executing: bool,
    pending_sql: Option<String>,
}

impl QueryEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            sql_input: cx.new(|cx| InputState::new(window, cx).placeholder("Enter SQL query...")),
            db: None,
            error_message: None,
            is_executing: false,
            pending_sql: None,
        }
    }

    pub fn set_connection(&mut self, db: Arc<DatabaseService>) {
        self.db = Some(db);
    }

    pub fn set_sql(&mut self, sql: &str, cx: &mut Context<Self>) {
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
        let error_color = theme.danger;

        div()
            .flex()
            .flex_col()
            .w_full()
            .bg(surface)
            .border_b_1()
            .border_color(border_color)
            .key_context("QueryEditor")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::on_execute))
            // Tab bar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(32.))
                    .bg(bg)
                    .border_b_1()
                    .border_color(border_color)
                    .px_2()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .bg(surface)
                            .rounded_t_md()
                            .text_size(px(12.))
                            .text_color(text_color)
                            .child("Query 1"),
                    ),
            )
            // SQL input area
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .child(div().flex_1().child(Input::new(&self.sql_input)))
                    .child(
                        Button::new("execute-btn")
                            .primary()
                            .label(if is_executing { "Running..." } else { "Execute" })
                            .disabled(!has_db || is_executing)
                            .loading(is_executing)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.execute(cx);
                            })),
                    ),
            )
            // Error display
            .when_some(error, |el, err| {
                el.child(
                    div()
                        .px_2()
                        .pb_2()
                        .text_size(px(12.))
                        .text_color(error_color)
                        .child(err),
                )
            })
    }
}
