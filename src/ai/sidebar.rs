use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme;

use crate::ai::provider::{ChatMessage, LlmProvider};
use crate::db::schema::DatabaseSchema;
use crate::db::types::DbType;

#[derive(Clone)]
pub enum AiEvent {
    InsertSql(String),
    RunSql(String),
}

pub struct AiSidebar {
    focus_handle: FocusHandle,
    messages: Vec<ChatMessage>,
    input: Entity<InputState>,
    provider: Option<Arc<LlmProvider>>,
    schema: Option<DatabaseSchema>,
    db_type: Option<DbType>,
    is_loading: bool,
    error: Option<String>,
    scroll_handle: UniformListScrollHandle,
    pending_clear_input: bool,
}

impl AiSidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let provider = LlmProvider::from_env().map(Arc::new);
        Self {
            focus_handle: cx.focus_handle(),
            messages: Vec::new(),
            input: cx.new(|cx| InputState::new(window, cx).placeholder("Ask about your database...")),
            provider,
            schema: None,
            db_type: None,
            is_loading: false,
            error: None,
            scroll_handle: UniformListScrollHandle::new(),
            pending_clear_input: false,
        }
    }

    pub fn set_schema(&mut self, schema: DatabaseSchema) {
        self.schema = Some(schema);
    }

    pub fn set_db_type(&mut self, db_type: DbType) {
        self.db_type = Some(db_type);
    }

    fn schema_context(&self) -> Option<String> {
        let schema = self.schema.as_ref()?;
        let mut ctx = String::new();
        for table in &schema.tables {
            ctx.push_str(&format!("Table: {}\n", table.name));
            for col in &table.columns {
                let pk = if col.is_primary_key { " [PK]" } else { "" };
                let null = if col.is_nullable { " NULL" } else { " NOT NULL" };
                ctx.push_str(&format!("  - {} {}{}{}\n", col.name, col.data_type, null, pk));
            }
            ctx.push('\n');
        }
        Some(ctx)
    }

    fn send_message(&mut self, cx: &mut Context<Self>) {
        let content = self.input.read(cx).value().to_string();
        if content.trim().is_empty() {
            return;
        }

        let Some(provider) = self.provider.clone() else {
            self.error =
                Some("No API key found. Set ANTHROPIC_API_KEY or OPENAI_API_KEY.".to_string());
            cx.notify();
            return;
        };

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.clone(),
        });
        self.pending_clear_input = true;
        self.is_loading = true;
        self.error = None;
        cx.notify();

        let messages = self.messages.clone();
        let schema_ctx = self.schema_context();
        let db_type_label = self.db_type.map(|dt| dt.label().to_string());

        cx.spawn(async move |this, cx| {
            let result: Result<String, anyhow::Error> = cx
                .background_executor()
                .spawn(async move {
                    provider.send_message(&messages, schema_ctx.as_deref(), db_type_label.as_deref())
                })
                .await;

            this.update(cx, |this, cx| {
                this.is_loading = false;
                match result {
                    Ok(response) => {
                        this.messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: response,
                        });
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

    fn extract_sql_blocks(text: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut in_block = false;
        let mut current = String::new();

        for line in text.lines() {
            if line.starts_with("```sql") || line.starts_with("```SQL") {
                in_block = true;
                current.clear();
            } else if line.starts_with("```") && in_block {
                in_block = false;
                if !current.trim().is_empty() {
                    blocks.push(current.trim().to_string());
                }
                current.clear();
            } else if in_block {
                current.push_str(line);
                current.push('\n');
            }
        }

        blocks
    }
}

impl EventEmitter<AiEvent> for AiSidebar {}

impl Focusable for AiSidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AiSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Apply pending clear
        if self.pending_clear_input {
            self.pending_clear_input = false;
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }

        let theme = cx.theme();
        let bg = theme.background;
        let _surface = theme.secondary;
        let border_color = theme.border;
        let _text_color = theme.foreground;
        let muted = theme.muted_foreground;
        let error_color = theme.danger;

        let message_count = self.messages.len();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(bg)
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
                    .child("AI Assistant"),
            )
            // Messages area
            .child(if message_count == 0 {
                div()
                    .id("ai-empty")
                    .flex_1()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child(if self.provider.is_some() {
                        "Ask me about your database, and I'll help you write SQL queries."
                    } else {
                        "Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable AI assistance."
                    })
                    .into_any_element()
            } else {
                uniform_list(
                    "ai-messages",
                    message_count,
                    cx.processor(|this: &mut Self, range: std::ops::Range<usize>, _window: &mut Window, cx: &mut Context<Self>| {
                        let theme = cx.theme();
                        range.map(|ix| {
                            let msg = &this.messages[ix];
                            let is_user = msg.role == "user";
                            let msg_bg = if is_user { theme.secondary } else { theme.background };

                            let mut msg_div = div()
                                .id(("msg", ix))
                                .p_3()
                                .bg(msg_bg)
                                .border_b_1()
                                .border_color(theme.border);

                            // Role label
                            msg_div = msg_div.child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(theme.muted_foreground)
                                    .mb_1()
                                    .child(if is_user { "You" } else { "Assistant" }),
                            );

                            // Message content
                            msg_div = msg_div.child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(theme.foreground)
                                    .child(msg.content.clone()),
                            );

                            msg_div
                        }).collect()
                    }),
                )
                .h_full()
                .into_any_element()
            })
            // Loading indicator
            .when(self.is_loading, |el| {
                el.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("Thinking..."),
                )
            })
            // Error display
            .when_some(self.error.clone(), |el, err| {
                el.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_size(px(11.))
                        .text_color(error_color)
                        .child(err),
                )
            })
            // Input area
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .border_t_1()
                    .border_color(border_color)
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .child(
                        Button::new("send-btn")
                            .primary()
                            .compact()
                            .label("Send")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.send_message(cx);
                            })),
                    ),
            )
    }
}
