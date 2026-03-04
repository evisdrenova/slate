use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Disableable};

use crate::db::connection::DatabaseService;
use crate::db::types::{ConnectionConfig, DbType};

#[derive(Clone)]
pub enum ConnectionEvent {
    Connected(Arc<DatabaseService>, ConnectionConfig),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Idle,
    Connecting,
    Connected,
    Error(String),
}

struct ParsedUri {
    host: String,
    port: String,
    user: String,
    password: String,
    database: String,
}

pub struct ConnectionDialog {
    focus_handle: FocusHandle,
    db_type: DbType,
    uri_input: Entity<InputState>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    port_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    password_visible: bool,
    pending_mask_toggle: bool,
    database_input: Entity<InputState>,
    status: ConnectionStatus,
    saved_connections: Vec<ConnectionConfig>,
    editing_connection_id: Option<String>,
    pending_load: Option<ConnectionConfig>,
    pending_port_change: Option<String>,
    pending_uri_parse: Option<ParsedUri>,
}

impl ConnectionDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = crate::connection::store::load_connections();
        let db_type = DbType::MySQL;

        let uri_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("postgres://user:pass@host:5432/dbname")
        });
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("My Database"));
        let host_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("localhost")
                .default_value("127.0.0.1")
        });
        let port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(&db_type.default_port().to_string())
                .default_value(&db_type.default_port().to_string())
        });
        let user_input = cx.new(|cx| InputState::new(window, cx).placeholder("root"));
        let password_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("password").masked(true));
        let database_input = cx.new(|cx| InputState::new(window, cx).placeholder("mydb"));

        Self {
            focus_handle: cx.focus_handle(),
            db_type,
            uri_input,
            name_input,
            host_input,
            port_input,
            user_input,
            password_input,
            password_visible: false,
            pending_mask_toggle: false,
            database_input,
            status: ConnectionStatus::Idle,
            saved_connections: saved,
            editing_connection_id: None,
            pending_load: None,
            pending_port_change: None,
            pending_uri_parse: None,
        }
    }

    fn set_db_type(&mut self, new_type: DbType, cx: &mut Context<Self>) {
        if self.db_type == new_type {
            return;
        }
        let old_default = self.db_type.default_port();
        self.db_type = new_type;
        let new_default = new_type.default_port();

        // Auto-switch port if it matches the old default
        let current_port: u16 = self
            .port_input
            .read(cx)
            .value()
            .parse()
            .unwrap_or(old_default);
        if current_port == old_default {
            // Store as pending so it gets applied in render() where we have window
            self.pending_port_change = Some(new_default.to_string());
        }
        cx.notify();
    }

    fn parse_uri(&mut self, cx: &mut Context<Self>) {
        let raw = self.uri_input.read(cx).value().to_string();
        if raw.trim().is_empty() {
            return;
        }

        let parsed = match url::Url::parse(&raw) {
            Ok(u) => u,
            Err(_) => {
                self.status = ConnectionStatus::Error("Invalid URI format".to_string());
                cx.notify();
                return;
            }
        };

        let host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
        let port = parsed
            .port()
            .unwrap_or(self.db_type.default_port())
            .to_string();
        let user = parsed.username().to_string();
        let password = parsed.password().unwrap_or("").to_string();
        let database = parsed.path().trim_start_matches('/').to_string();

        // Auto-detect db type from scheme
        match parsed.scheme() {
            "mysql" => {
                if self.db_type != DbType::MySQL {
                    self.db_type = DbType::MySQL;
                }
            }
            "postgres" | "postgresql" => {
                if self.db_type != DbType::PostgreSQL {
                    self.db_type = DbType::PostgreSQL;
                }
            }
            _ => {}
        }

        self.pending_uri_parse = Some(ParsedUri {
            host,
            port,
            user,
            password,
            database,
        });
        self.status = ConnectionStatus::Idle;
        cx.notify();
    }

    fn toggle_password_visibility(&mut self, cx: &mut Context<Self>) {
        self.password_visible = !self.password_visible;
        self.pending_mask_toggle = true;
        cx.notify();
    }

    fn build_config(&self, cx: &App) -> ConnectionConfig {
        ConnectionConfig::new(
            self.db_type,
            self.name_input.read(cx).value().to_string(),
            self.host_input.read(cx).value().to_string(),
            self.port_input
                .read(cx)
                .value()
                .parse::<u16>()
                .unwrap_or(self.db_type.default_port()),
            self.user_input.read(cx).value().to_string(),
            self.database_input.read(cx).value().to_string(),
        )
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.status = ConnectionStatus::Connecting;
        cx.notify();

        let mut config = self.build_config(cx);
        // Preserve original ID if editing an existing connection
        if let Some(ref existing_id) = self.editing_connection_id {
            config.id = existing_id.clone();
        }
        let password = self.password_input.read(cx).value().to_string();

        cx.spawn(async move |this, cx| {
            let config_clone = config.clone();
            let pw = password.clone();
            let result: Result<DatabaseService, anyhow::Error> = cx
                .background_executor()
                .spawn(async move { DatabaseService::connect(&config_clone, &pw) })
                .await;

            this.update(cx, |this, cx| match result {
                Ok(db) => {
                    this.status = ConnectionStatus::Connected;
                    // Save connection — retain by both name and ID to avoid duplicates
                    let mut conns = this.saved_connections.clone();
                    conns.retain(|c| c.name != config.name && c.id != config.id);
                    conns.push(config.clone());
                    let _ = crate::connection::store::save_connections(&conns);
                    let _ = crate::connection::store::save_password(&config.id, &password);
                    let _ = crate::connection::store::save_last_connection_id(&config.id);
                    cx.emit(ConnectionEvent::Connected(Arc::new(db), config));
                    cx.notify();
                }
                Err(e) => {
                    this.status = ConnectionStatus::Error(e.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn load_saved_connection(&mut self, config: &ConnectionConfig, cx: &mut Context<Self>) {
        self.db_type = config.db_type;
        self.editing_connection_id = Some(config.id.clone());
        self.pending_load = Some(config.clone());
        self.status = ConnectionStatus::Idle;
        cx.notify();
    }

    fn delete_saved_connection(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.saved_connections.len() {
            let config = self.saved_connections.remove(idx);
            let _ = crate::connection::store::delete_password(&config.id);
            let _ = crate::connection::store::save_connections(&self.saved_connections);
            cx.notify();
        }
    }

    fn apply_pending_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(config) = self.pending_load.take() {
            self.name_input
                .update(cx, |input, cx| input.set_value(&config.name, window, cx));
            self.host_input
                .update(cx, |input, cx| input.set_value(&config.host, window, cx));
            self.port_input.update(cx, |input, cx| {
                input.set_value(&config.port.to_string(), window, cx)
            });
            self.user_input
                .update(cx, |input, cx| input.set_value(&config.user, window, cx));
            self.database_input
                .update(cx, |input, cx| input.set_value(&config.database, window, cx));

            if let Some(pw) = crate::connection::store::load_password(&config.id) {
                self.password_input
                    .update(cx, |input, cx| input.set_value(&pw, window, cx));
            }
        }

        // Apply pending port change from db_type switch
        if let Some(port) = self.pending_port_change.take() {
            self.port_input
                .update(cx, |input, cx| input.set_value(&port, window, cx));
        }

        // Apply password mask toggle
        if self.pending_mask_toggle {
            self.pending_mask_toggle = false;
            let masked = !self.password_visible;
            self.password_input
                .update(cx, |input, cx| input.set_masked(masked, window, cx));
        }

        // Apply parsed URI fields
        if let Some(parsed) = self.pending_uri_parse.take() {
            self.host_input
                .update(cx, |input, cx| input.set_value(&parsed.host, window, cx));
            self.port_input
                .update(cx, |input, cx| input.set_value(&parsed.port, window, cx));
            self.user_input
                .update(cx, |input, cx| input.set_value(&parsed.user, window, cx));
            self.password_input
                .update(cx, |input, cx| input.set_value(&parsed.password, window, cx));
            if !parsed.database.is_empty() {
                self.database_input
                    .update(cx, |input, cx| input.set_value(&parsed.database, window, cx));
            }
        }
    }

    fn render_field(
        label: &str,
        input: &Entity<InputState>,
        muted_color: Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .mb_2()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(muted_color)
                    .child(label.to_string()),
            )
            .child(Input::new(&input))
    }
}

impl EventEmitter<ConnectionEvent> for ConnectionDialog {}

impl Focusable for ConnectionDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConnectionDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_load(window, cx);

        let theme = cx.theme();

        let status_text = match &self.status {
            ConnectionStatus::Idle => String::new(),
            ConnectionStatus::Connecting => "Connecting...".to_string(),
            ConnectionStatus::Connected => "Connected!".to_string(),
            ConnectionStatus::Error(e) => {
                let msg = format!("Error: {}", e);
                // Cap at 200 chars to avoid layout overflow
                if msg.len() > 200 {
                    format!("{}...", &msg[..200])
                } else {
                    msg
                }
            }
        };
        let status_color = match &self.status {
            ConnectionStatus::Error(_) => theme.danger,
            ConnectionStatus::Connected => theme.success,
            _ => theme.muted_foreground,
        };
        let is_connecting = self.status == ConnectionStatus::Connecting;
        let bg = theme.background;
        let surface = theme.secondary;
        let border_color = theme.border;
        let text_color = theme.foreground;
        let muted = theme.muted_foreground;
        let current_db_type = self.db_type;

        // Saved connections sidebar
        let saved = self.saved_connections.clone();

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000080))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w(px(720.))
                    .max_h(px(640.))
                    .bg(surface)
                    .border_1()
                    .border_color(border_color)
                    .rounded_lg()
                    .overflow_hidden()
                    // Saved connections sidebar
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(180.))
                            .bg(bg)
                            .border_r_1()
                            .border_color(border_color)
                            .p_3()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .mb_2()
                                    .child("Saved Connections"),
                            )
                            .children(saved.iter().enumerate().map(|(idx, conn)| {
                                let config = conn.clone();
                                let error_color = theme.danger;
                                div()
                                    .id(("saved", idx))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_size(px(13.))
                                    .text_color(text_color)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(surface))
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.load_saved_connection(&config, cx);
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .overflow_x_hidden()
                                            .text_ellipsis()
                                            .child(conn.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .id(("del-conn", idx))
                                            .cursor_pointer()
                                            .text_size(px(10.))
                                            .text_color(muted)
                                            .hover(|el| el.text_color(error_color))
                                            .rounded_sm()
                                            .px(px(3.))
                                            .py(px(1.))
                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                this.delete_saved_connection(idx, cx);
                                            }))
                                            .child("×"),
                                    )
                            })),
                    )
                    // Connection form
                    .child(
                        div()
                            .id("connection-form")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_y_scroll()
                            .p_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .mb_4()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .text_color(text_color)
                                            .font_weight(FontWeight::BOLD)
                                            .child(if self.editing_connection_id.is_some() {
                                                "Edit Connection"
                                            } else {
                                                "New Connection"
                                            }),
                                    )
                                    .child(
                                        Button::new("close-dialog-btn")
                                            .ghost()
                                            .compact()
                                            .label("X")
                                            .on_click(cx.listener(|_this, _, _window, cx| {
                                                cx.emit(ConnectionEvent::Cancelled);
                                            })),
                                    ),
                            )
                            // Database type toggle
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_1()
                                    .mb_3()
                                    .child(
                                        Button::new("db-type-mysql")
                                            .label("MySQL")
                                            .compact()
                                            .when(current_db_type == DbType::MySQL, |btn| {
                                                btn.primary()
                                            })
                                            .when(current_db_type != DbType::MySQL, |btn| {
                                                btn.ghost()
                                            })
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.set_db_type(DbType::MySQL, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("db-type-postgres")
                                            .label("PostgreSQL")
                                            .compact()
                                            .when(
                                                current_db_type == DbType::PostgreSQL,
                                                |btn| btn.primary(),
                                            )
                                            .when(
                                                current_db_type != DbType::PostgreSQL,
                                                |btn| btn.ghost(),
                                            )
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.set_db_type(DbType::PostgreSQL, cx);
                                            })),
                                    ),
                            )
                            // URI paste field
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .mb_2()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(muted)
                                            .child("Connection URI"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_1()
                                            .child(
                                                div().flex_1().child(Input::new(&self.uri_input)),
                                            )
                                            .child(
                                                Button::new("parse-uri-btn")
                                                    .ghost()
                                                    .compact()
                                                    .label("Parse")
                                                    .on_click(cx.listener(
                                                        |this, _, _window, cx| {
                                                            this.parse_uri(cx);
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(Self::render_field(
                                "Connection Name",
                                &self.name_input,
                                muted,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(Self::render_field(
                                            "Host",
                                            &self.host_input,
                                            muted,
                                        )),
                                    )
                                    .child(
                                        div().w(px(80.)).child(Self::render_field(
                                            "Port",
                                            &self.port_input,
                                            muted,
                                        )),
                                    ),
                            )
                            .child(Self::render_field(
                                "User",
                                &self.user_input,
                                muted,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .mb_2()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(muted)
                                            .child("Password"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .child(Input::new(&self.password_input)),
                                            )
                                            .child(
                                                Button::new("toggle-pw-btn")
                                                    .ghost()
                                                    .compact()
                                                    .label(if self.password_visible {
                                                        "Hide"
                                                    } else {
                                                        "Show"
                                                    })
                                                    .on_click(cx.listener(
                                                        |this, _, _window, cx| {
                                                            this.toggle_password_visibility(cx);
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .child(Self::render_field(
                                "Database",
                                &self.database_input,
                                muted,
                            ))
                            // Status
                            .when(!status_text.is_empty(), |el| {
                                el.child(
                                    div()
                                        .w_full()
                                        .whitespace_normal()
                                        .text_size(px(11.))
                                        .text_color(status_color)
                                        .mb_2()
                                        .child(status_text),
                                )
                            })
                            // Buttons
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_end()
                                    .gap_2()
                                    .mt_2()
                                    .child(
                                        Button::new("cancel-btn")
                                            .ghost()
                                            .label("Cancel")
                                            .on_click(cx.listener(|_this, _, _window, cx| {
                                                cx.emit(ConnectionEvent::Cancelled);
                                            })),
                                    )
                                    .child(
                                        Button::new("connect-btn")
                                            .primary()
                                            .label(if is_connecting {
                                                "Connecting..."
                                            } else {
                                                "Connect"
                                            })
                                            .disabled(is_connecting)
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.connect(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}
