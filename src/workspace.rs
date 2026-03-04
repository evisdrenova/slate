use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::theme::{Theme, ThemeMode};
use gpui_component::{ActiveTheme, Icon, IconName};

use crate::ai::sidebar::{AiEvent, AiSidebar};
use crate::connection::dialog::{ConnectionDialog, ConnectionEvent};
use crate::db::connection::DatabaseService;
use crate::db::schema;
use crate::db::types::{ConnectionConfig, DbType};
use crate::editor::query_editor::{QueryEditor, QueryEvent};
use crate::grid::results_grid::ResultsGrid;
use crate::schema::explorer::{SchemaEvent, SchemaExplorer};

actions!(workspace, [ToggleAiSidebar, ShowConnectionDialog, ToggleSchemaSidebar]);

struct PanelResize {
    panel: PanelSide,
    start_x: Pixels,
    original_width: Pixels,
}

#[derive(Clone, Copy, PartialEq)]
enum PanelSide {
    Left,
    Right,
}

pub struct Workspace {
    focus_handle: FocusHandle,
    db: Option<Arc<DatabaseService>>,
    db_type: DbType,
    connection_config: Option<ConnectionConfig>,
    schema_explorer: Entity<SchemaExplorer>,
    query_editor: Entity<QueryEditor>,
    results_grid: Entity<ResultsGrid>,
    ai_sidebar: Entity<AiSidebar>,
    connection_dialog: Option<Entity<ConnectionDialog>>,
    ai_visible: bool,
    left_panel_visible: bool,
    left_panel_width: Pixels,
    right_panel_width: Pixels,
    panel_resize: Option<PanelResize>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let schema_explorer = cx.new(|cx| SchemaExplorer::new(window, cx));
        let query_editor = cx.new(|cx| QueryEditor::new(window, cx));
        let results_grid = cx.new(|cx| ResultsGrid::new(cx));
        let ai_sidebar = cx.new(|cx| AiSidebar::new(window, cx));
        let connection_dialog = Some(cx.new(|cx| ConnectionDialog::new(window, cx)));

        // Subscribe to schema explorer events
        cx.subscribe(&schema_explorer, |this: &mut Self, _, event: &SchemaEvent, cx| {
            match event {
                SchemaEvent::TableSelected(table) => {
                    let quoted = this.db_type.quote_identifier(table);
                    let sql = format!("SELECT * FROM {} LIMIT 100", quoted);
                    this.query_editor.update(cx, |editor, cx| {
                        editor.set_sql(&sql, cx);
                        editor.execute(cx);
                    });
                }
            }
        })
        .detach();

        // Subscribe to query editor events
        cx.subscribe(&query_editor, |this: &mut Self, _, event: &QueryEvent, cx| {
            match event {
                QueryEvent::QueryExecuted(result, sql) => {
                    this.results_grid.update(cx, |grid, cx| {
                        grid.add_result(result.clone(), sql, cx);
                    });
                }
                QueryEvent::QueryError(_) => {}
            }
        })
        .detach();

        // Subscribe to AI sidebar events
        cx.subscribe(&ai_sidebar, |this: &mut Self, _, event: &AiEvent, cx| {
            match event {
                AiEvent::InsertSql(sql) => {
                    this.query_editor.update(cx, |editor, cx| {
                        editor.set_sql(sql, cx);
                    });
                }
                AiEvent::RunSql(sql) => {
                    this.query_editor.update(cx, |editor, cx| {
                        editor.set_sql(sql, cx);
                        editor.execute(cx);
                    });
                }
            }
        })
        .detach();

        // Subscribe to connection dialog events
        if let Some(dialog) = &connection_dialog {
            cx.subscribe(dialog, |this: &mut Self, _, event: &ConnectionEvent, cx| {
                match event {
                    ConnectionEvent::Connected(db, config) => {
                        this.setup_connection(db.clone(), config.clone(), cx);
                    }
                    ConnectionEvent::Cancelled => {
                        this.connection_dialog = None;
                        cx.notify();
                    }
                }
            })
            .detach();
        }

        // Try auto-connect to last used database
        let try_auto_connect = if let Some(last_id) = crate::connection::store::load_last_connection_id() {
            let connections = crate::connection::store::load_connections();
            if let Some(config) = connections.into_iter().find(|c| c.id == last_id) {
                if let Some(password) = crate::connection::store::load_password(&config.id) {
                    Some((config, password))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let connection_dialog = if try_auto_connect.is_some() {
            None // Skip dialog, we'll auto-connect
        } else {
            connection_dialog
        };

        let ws = Self {
            focus_handle: cx.focus_handle(),
            db: None,
            db_type: DbType::MySQL,
            connection_config: None,
            schema_explorer,
            query_editor,
            results_grid,
            ai_sidebar,
            connection_dialog,
            ai_visible: false,
            left_panel_visible: true,
            left_panel_width: px(260.),
            right_panel_width: px(300.),
            panel_resize: None,
        };

        if let Some((config, password)) = try_auto_connect {
            cx.spawn(async move |this, cx| {
                let config_clone = config.clone();
                let pw = password.clone();
                let result: Result<DatabaseService, anyhow::Error> = cx
                    .background_executor()
                    .spawn(async move { DatabaseService::connect(&config_clone, &pw) })
                    .await;

                this.update(cx, |this, cx| {
                    match result {
                        Ok(db) => {
                            this.setup_connection(Arc::new(db), config, cx);
                        }
                        Err(_) => {
                            // Auto-connect failed, show connection dialog
                            // We need window access to create the dialog, so dispatch the action
                            cx.dispatch_action(&ShowConnectionDialog);
                        }
                    }
                }).ok();
            })
            .detach();
        }

        ws
    }

    fn setup_connection(&mut self, db: Arc<DatabaseService>, config: ConnectionConfig, cx: &mut Context<Self>) {
        self.db = Some(db.clone());
        self.db_type = config.db_type;
        self.connection_config = Some(config.clone());
        self.connection_dialog = None;

        self.query_editor.update(cx, |editor, _cx| {
            editor.set_connection(db.clone());
        });

        let db_clone = db.clone();
        let config_clone = config.clone();
        self.schema_explorer.update(cx, |explorer, cx| {
            explorer.load_schema(db_clone, &config_clone, cx);
        });

        self.ai_sidebar.update(cx, |sidebar, _cx| {
            sidebar.set_db_type(config.db_type);
        });

        let db_for_schema = db.clone();
        let db_name = config.database.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<crate::db::schema::DatabaseSchema, anyhow::Error> = cx
                .background_executor()
                .spawn(async move {
                    schema::fetch_schema(&db_for_schema, &db_name)
                })
                .await;

            if let Ok(db_schema) = result {
                this.update(cx, |this, cx| {
                    this.ai_sidebar.update(cx, |sidebar, _cx| {
                        sidebar.set_schema(db_schema.clone());
                    });
                    this.query_editor.update(cx, |editor, _cx| {
                        editor.set_schema(db_schema);
                    });
                }).ok();
            }
        })
        .detach();

        cx.notify();
    }

    fn toggle_ai_sidebar(
        &mut self,
        _: &ToggleAiSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_visible = !self.ai_visible;
        cx.notify();
    }

    fn toggle_schema_sidebar(
        &mut self,
        _: &ToggleSchemaSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.left_panel_visible = !self.left_panel_visible;
        cx.notify();
    }

    fn show_connection_dialog(
        &mut self,
        _: &ShowConnectionDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| ConnectionDialog::new(window, cx));
        cx.subscribe(&dialog, |this: &mut Self, _, event: &ConnectionEvent, cx| {
            match event {
                ConnectionEvent::Connected(db, config) => {
                    this.setup_connection(db.clone(), config.clone(), cx);
                }
                ConnectionEvent::Cancelled => {
                    this.connection_dialog = None;
                    cx.notify();
                }
            }
        })
        .detach();
        self.connection_dialog = Some(dialog);
        cx.notify();
    }

    fn connection_status(&self, _cx: &App) -> String {
        if let Some(config) = &self.connection_config {
            format!(
                "Slate - {}@{}:{}/{}",
                config.user, config.host, config.port, config.database
            )
        } else {
            "Slate - Not Connected".to_string()
        }
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let bg = theme.background;
        let surface = theme.secondary;
        let border_color = theme.border;
        let text_color = theme.foreground;
        let _muted = theme.muted_foreground;
        let accent = theme.primary;

        let status = self.connection_status(cx);
        let is_connected = self.db.is_some();
        let ai_visible = self.ai_visible;
        let left_panel_visible = self.left_panel_visible;
        let is_dark = theme.mode.is_dark();
        let left_w = self.left_panel_width;
        let right_w = self.right_panel_width;
        let is_resizing = self.panel_resize.is_some();
        let resize_hover = Hsla { a: 0.5, ..accent };

        div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(bg)
            .text_color(text_color)
            .text_size(px(14.))
            .key_context("Workspace")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::toggle_ai_sidebar))
            .on_action(cx.listener(Self::show_connection_dialog))
            .on_action(cx.listener(Self::toggle_schema_sidebar))
            // Global mouse tracking for panel resize
            .when(is_resizing, |el| el.cursor_col_resize())
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(ref resize) = this.panel_resize {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.panel_resize = None;
                        cx.notify();
                        return;
                    }
                    let delta = event.position.x - resize.start_x;
                    match resize.panel {
                        PanelSide::Left => {
                            this.left_panel_width =
                                (resize.original_width + delta).max(px(150.)).min(px(500.));
                        }
                        PanelSide::Right => {
                            // Right panel: dragging left increases width
                            this.right_panel_width =
                                (resize.original_width - delta).max(px(200.)).min(px(600.));
                        }
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    if this.panel_resize.is_some() {
                        this.panel_resize = None;
                        cx.notify();
                    }
                }),
            )
            // Title bar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(36.))
                    .px_4()
                    .bg(surface)
                    .border_b_1()
                    .border_color(border_color)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(8.))
                                    .h(px(8.))
                                    .rounded_full()
                                    .bg(if is_connected {
                                        gpui::rgb(0x4ade80).into()
                                    } else {
                                        _muted
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                Button::new("reconnect-btn")
                                    .ghost()
                                    .compact()
                                    .icon(Icon::new(IconName::Settings2))
                                    .tooltip(if is_connected { "Switch DB" } else { "Connect" })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_connection_dialog(
                                            &ShowConnectionDialog,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("theme-toggle-btn")
                                    .ghost()
                                    .compact()
                                    .icon(Icon::new(if is_dark {
                                        IconName::Sun
                                    } else {
                                        IconName::Moon
                                    }))
                                    .tooltip(if is_dark { "Light mode" } else { "Dark mode" })
                                    .on_click(|_, window, cx| {
                                        let new_mode = if cx.theme().mode.is_dark() {
                                            ThemeMode::Light
                                        } else {
                                            ThemeMode::Dark
                                        };
                                        Theme::change(new_mode, Some(window), cx);
                                    }),
                            )
                            .child(
                                Button::new("ai-toggle-btn")
                                    .compact()
                                    .when(ai_visible, |btn| btn.primary())
                                    .when(!ai_visible, |btn| btn.ghost())
                                    .icon(Icon::new(IconName::Bot))
                                    .tooltip("AI Assistant")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_ai_sidebar(
                                            &ToggleAiSidebar,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            // Main content area
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .overflow_hidden()
                    // Left sidebar - Schema Explorer (resizable)
                    .when(left_panel_visible, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_shrink_0()
                                .w(left_w)
                                .h_full()
                                .border_r_1()
                                .border_color(border_color)
                                .child(self.schema_explorer.clone())
                                // Left panel drag handle
                                .child(
                                    div()
                                        .id("left-resize-handle")
                                        .w(px(4.))
                                        .h_full()
                                        .flex_shrink_0()
                                        .cursor_col_resize()
                                        .hover(|el| el.bg(resize_hover))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |this, event: &MouseDownEvent, _window, _cx| {
                                                    this.panel_resize = Some(PanelResize {
                                                        panel: PanelSide::Left,
                                                        start_x: event.position.x,
                                                        original_width: this.left_panel_width,
                                                    });
                                                },
                                            ),
                                        ),
                                ),
                        )
                    })
                    // Center - Query Editor + Results Grid
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            .child(self.query_editor.clone())
                            .child(self.results_grid.clone()),
                    )
                    // Right sidebar - AI (conditional, resizable)
                    .when(ai_visible, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_shrink_0()
                                .w(right_w)
                                .h_full()
                                .border_l_1()
                                .border_color(border_color)
                                // Right panel drag handle
                                .child(
                                    div()
                                        .id("right-resize-handle")
                                        .w(px(4.))
                                        .h_full()
                                        .flex_shrink_0()
                                        .cursor_col_resize()
                                        .hover(|el| el.bg(resize_hover))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |this, event: &MouseDownEvent, _window, _cx| {
                                                    this.panel_resize = Some(PanelResize {
                                                        panel: PanelSide::Right,
                                                        start_x: event.position.x,
                                                        original_width: this.right_panel_width,
                                                    });
                                                },
                                            ),
                                        ),
                                )
                                .child(self.ai_sidebar.clone()),
                        )
                    }),
            )
            // Connection dialog overlay
            .when_some(self.connection_dialog.clone(), |el, dialog| {
                el.child(dialog)
            })
    }
}
