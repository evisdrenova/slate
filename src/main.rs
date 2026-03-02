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

fn main() {
    Application::new()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .unwrap();
            cx.activate(true);
        });
}
