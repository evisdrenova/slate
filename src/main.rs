mod ai;
mod connection;
mod db;
mod editor;
mod grid;
mod schema;
mod workspace;

use gpui::*;
use gpui_component::Root;
use workspace::Workspace;

use crate::editor::query_editor::{NewTab, CloseTab, SaveQuery, DismissPanel};
use crate::workspace::{ToggleSchemaSidebar, ToggleAiSidebar};

fn main() {
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            cx.bind_keys([
                // QueryEditor shortcuts
                KeyBinding::new("cmd-n", NewTab, Some("QueryEditor")),
                KeyBinding::new("cmd-w", CloseTab, Some("QueryEditor")),
                KeyBinding::new("cmd-s", SaveQuery, Some("QueryEditor")),
                KeyBinding::new("escape", DismissPanel, Some("QueryEditor")),
                // Workspace shortcuts
                KeyBinding::new("cmd-b", ToggleSchemaSidebar, Some("Workspace")),
                KeyBinding::new("cmd-j", ToggleAiSidebar, Some("Workspace")),
            ]);

            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .unwrap();
            cx.activate(true);
        });
}
