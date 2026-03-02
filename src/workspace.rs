use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::ActiveTheme;

use crate::ai::sidebar::{AiEvent, AiSidebar};
use crate::connection::dialog::{ConnectionDialog, ConnectionEvent};
use crate::db::connection::DatabaseService;
use crate::db::schema;
use crate::db::types::ConnectionConfig;
use crate::editor::query_editor::{QueryEditor, QueryEvent};
use crate::grid::results_grid::ResultsGrid;
use crate::schema::explorer::{SchemaEvent, SchemaExplorer};

actions!(workspace, [ToggleAiSidebar, ShowConnectionDialog]);

pub struct Workspace {
    focus_handle: FocusHandle,
    db: Option<Arc<DatabaseService>>,
    connection_config: Option<ConnectionConfig>,
    schema_explorer: Entity<SchemaExplorer>,
    query_editor: Entity<QueryEditor>,
    results_grid: Entity<ResultsGrid>,
    ai_sidebar: Entity<AiSidebar>,
    connection_dialog: Option<Entity<ConnectionDialog>>,
    ai_visible: bool,
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
                    let sql = format!("SELECT * FROM `{}` LIMIT 100", table);
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
                QueryEvent::QueryExecuted(result) => {
                    this.results_grid.update(cx, |grid, cx| {
                        grid.set_result(result.clone(), cx);
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
                        this.db = Some(db.clone());
                        this.connection_config = Some(config.clone());
                        this.connection_dialog = None;

                        // Pass DB to query editor
                        this.query_editor.update(cx, |editor, _cx| {
                            editor.set_connection(db.clone());
                        });

                        // Load schema
                        let db_clone = db.clone();
                        let database = config.database.clone();
                        this.schema_explorer.update(cx, |explorer, cx| {
                            explorer.load_schema(db_clone.clone(), database, cx);
                        });

                        // Load schema for AI sidebar too
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
                                        sidebar.set_schema(db_schema);
                                    });
                                }).ok();
                            }
                        })
                        .detach();

                        cx.notify();
                    }
                    ConnectionEvent::Cancelled => {
                        this.connection_dialog = None;
                        cx.notify();
                    }
                }
            })
            .detach();
        }

        Self {
            focus_handle: cx.focus_handle(),
            db: None,
            connection_config: None,
            schema_explorer,
            query_editor,
            results_grid,
            ai_sidebar,
            connection_dialog,
            ai_visible: false,
        }
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
                    this.db = Some(db.clone());
                    this.connection_config = Some(config.clone());
                    this.connection_dialog = None;

                    this.query_editor.update(cx, |editor, _cx| {
                        editor.set_connection(db.clone());
                    });

                    let db_clone = db.clone();
                    let database = config.database.clone();
                    this.schema_explorer.update(cx, |explorer, cx| {
                        explorer.load_schema(db_clone, database, cx);
                    });

                    cx.notify();
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
        let _accent = theme.primary;

        let status = self.connection_status(cx);
        let is_connected = self.db.is_some();
        let ai_visible = self.ai_visible;

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
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(status),
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
                                    .label(if is_connected { "Switch DB" } else { "Connect" })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_connection_dialog(
                                            &ShowConnectionDialog,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("ai-toggle-btn")
                                    .compact()
                                    .when(ai_visible, |btn| btn.primary())
                                    .when(!ai_visible, |btn| btn.ghost())
                                    .label("AI")
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
                    // Left sidebar - Schema Explorer
                    .child(self.schema_explorer.clone())
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
                    // Right sidebar - AI (conditional)
                    .when(ai_visible, |el| el.child(self.ai_sidebar.clone())),
            )
            // Connection dialog overlay
            .when_some(self.connection_dialog.clone(), |el, dialog| {
                el.child(dialog)
            })
    }
}
