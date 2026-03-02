use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Disableable};

use crate::db::connection::DatabaseService;
use crate::db::types::ConnectionConfig;

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

pub struct ConnectionDialog {
    focus_handle: FocusHandle,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    port_input: Entity<InputState>,
    user_input: Entity<InputState>,
    password_input: Entity<InputState>,
    database_input: Entity<InputState>,
    status: ConnectionStatus,
    saved_connections: Vec<ConnectionConfig>,
    pending_load: Option<ConnectionConfig>,
}

impl ConnectionDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = crate::connection::store::load_connections();

        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("My Database"));
        let host_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("localhost")
                .default_value("127.0.0.1")
        });
        let port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("3306")
                .default_value("3306")
        });
        let user_input = cx.new(|cx| InputState::new(window, cx).placeholder("root"));
        let password_input = cx.new(|cx| InputState::new(window, cx).placeholder("password").masked(true));
        let database_input = cx.new(|cx| InputState::new(window, cx).placeholder("mydb"));

        Self {
            focus_handle: cx.focus_handle(),
            name_input,
            host_input,
            port_input,
            user_input,
            password_input,
            database_input,
            status: ConnectionStatus::Idle,
            saved_connections: saved,
            pending_load: None,
        }
    }

    fn build_config(&self, cx: &App) -> ConnectionConfig {
        ConnectionConfig::new(
            self.name_input.read(cx).value().to_string(),
            self.host_input.read(cx).value().to_string(),
            self.port_input
                .read(cx)
                .value()
                .parse::<u16>()
                .unwrap_or(3306),
            self.user_input.read(cx).value().to_string(),
            self.database_input.read(cx).value().to_string(),
        )
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.status = ConnectionStatus::Connecting;
        cx.notify();

        let config = self.build_config(cx);
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
                    // Save connection
                    let mut conns = this.saved_connections.clone();
                    conns.retain(|c| c.name != config.name);
                    conns.push(config.clone());
                    let _ = crate::connection::store::save_connections(&conns);
                    let _ = crate::connection::store::save_password(&config.id, &password);
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
        self.pending_load = Some(config.clone());
        cx.notify();
    }

    fn apply_pending_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(config) = self.pending_load.take() {
            self.name_input
                .update(cx, |input, cx| input.set_value(&config.name, window, cx));
            self.host_input
                .update(cx, |input, cx| input.set_value(&config.host, window, cx));
            self.port_input
                .update(cx, |input, cx| input.set_value(&config.port.to_string(), window, cx));
            self.user_input
                .update(cx, |input, cx| input.set_value(&config.user, window, cx));
            self.database_input
                .update(cx, |input, cx| input.set_value(&config.database, window, cx));

            if let Some(pw) = crate::connection::store::load_password(&config.id) {
                self.password_input
                    .update(cx, |input, cx| input.set_value(&pw, window, cx));
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
            ConnectionStatus::Error(e) => format!("Error: {}", e),
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
                    .w(px(640.))
                    .h(px(460.))
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
                                div()
                                    .id(("saved", idx))
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
                                    .child(conn.name.clone())
                            })),
                    )
                    // Connection form
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .p_4()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .text_color(text_color)
                                    .font_weight(FontWeight::BOLD)
                                    .mb_4()
                                    .child("New Connection"),
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
                            .child(Self::render_field(
                                "Password",
                                &self.password_input,
                                muted,
                            ))
                            .child(Self::render_field(
                                "Database",
                                &self.database_input,
                                muted,
                            ))
                            // Status
                            .when(!status_text.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(12.))
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
                                            .label(if is_connecting { "Connecting..." } else { "Connect" })
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
