use std::collections::HashSet;

use gpui::prelude::*;
use gpui::*;
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable};

use crate::db::schema::DatabaseSchema;
use crate::db::types::{CellValue, QueryResult};

const MIN_COL_WIDTH: f32 = 40.0;
const MAX_TABS: usize = 20;

struct ColumnResize {
    col_idx: usize,
    start_x: Pixels,
    original_width: Pixels,
}

struct ResultTab {
    id: usize,
    title: String,
    result: QueryResult,
    column_widths: Vec<Pixels>,
    scroll_handle: UniformListScrollHandle,
    h_scroll_handle: ScrollHandle,
}

pub struct ResultsGrid {
    focus_handle: FocusHandle,
    tabs: Vec<ResultTab>,
    active_tab: usize,
    next_tab_id: usize,
    resize_state: Option<ColumnResize>,
    schema: Option<DatabaseSchema>,
    column_info_idx: Option<usize>,
    detail_cell: Option<(usize, usize)>, // (row_idx, col_idx) for detail panel
    selected_cell: Option<(usize, usize)>, // (row_idx, col_idx) for highlight
    json_collapsed: HashSet<String>,       // collapsed JSON tree paths (expanded by default)
    initial_render_ms: Option<f64>,        // set once on first render with results
}

impl ResultsGrid {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            tabs: Vec::new(),
            active_tab: 0,
            next_tab_id: 1,
            resize_state: None,
            schema: None,
            column_info_idx: None,
            detail_cell: None,
            selected_cell: None,
            json_collapsed: HashSet::new(),
            initial_render_ms: None,
        }
    }

    pub fn add_result(&mut self, result: QueryResult, sql: &str, cx: &mut Context<Self>) {
        let column_widths: Vec<Pixels> = result
            .columns
            .iter()
            .map(|col| -> Pixels {
                let min_width = px(80.);
                let max_width = px(300.);
                let header_width = px(col.name.len() as f32 * 8.0 + 24.0);
                header_width.max(min_width).min(max_width)
            })
            .collect();

        let title = if sql.chars().count() > 30 {
            format!("{}...", sql.chars().take(30).collect::<String>())
        } else if sql.is_empty() {
            format!("Result {}", self.next_tab_id)
        } else {
            sql.to_string()
        };

        let tab = ResultTab {
            id: self.next_tab_id,
            title,
            result,
            column_widths,
            scroll_handle: UniformListScrollHandle::new(),
            h_scroll_handle: ScrollHandle::new(),
        };
        self.next_tab_id += 1;

        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.initial_render_ms = None;
        self.json_collapsed.clear();

        // Cap at MAX_TABS by removing oldest
        while self.tabs.len() > MAX_TABS {
            self.tabs.remove(0);
            if self.active_tab > 0 {
                self.active_tab -= 1;
            }
        }

        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.active_tab = 0;
        cx.notify();
    }

    pub fn set_schema(&mut self, schema: DatabaseSchema) {
        self.schema = Some(schema);
    }

    fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if idx < self.active_tab {
            self.active_tab -= 1;
        } else if idx == self.active_tab && self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        cx.notify();
    }

    fn switch_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
            self.column_info_idx = None;
            cx.notify();
        }
    }

    fn collect_json_paths(value: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                out.push(path.to_string());
                for (key, val) in map {
                    Self::collect_json_paths(val, &format!("{}.{}", path, key), out);
                }
            }
            serde_json::Value::Array(arr) => {
                out.push(path.to_string());
                for (i, val) in arr.iter().enumerate() {
                    Self::collect_json_paths(val, &format!("{}[{}]", path, i), out);
                }
            }
            _ => {}
        }
    }

    fn render_json_tree(&self, value: &serde_json::Value, path: &str, depth: usize, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme();
        let text_color = theme.foreground;
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        let string_color: Hsla = gpui::rgb(0x89dceb).into();
        let number_color: Hsla = gpui::rgb(0xf78c6c).into();
        let indent = px(16. * depth as f32);

        match value {
            serde_json::Value::Object(map) => {
                let is_expanded = !self.json_collapsed.contains(path);
                let path_owned = path.to_string();
                let summary = format!("{{{} keys}}", map.len());
                let icon = if is_expanded { IconName::ChevronDown } else { IconName::ChevronRight };

                let mut rows: Vec<AnyElement> = vec![
                    div()
                        .id(ElementId::Name(format!("json-{}", path).into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .pl(indent)
                        .py(px(1.))
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.secondary))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            if this.json_collapsed.contains(&path_owned) {
                                this.json_collapsed.remove(&path_owned);
                            } else {
                                this.json_collapsed.insert(path_owned.clone());
                            }
                            cx.notify();
                        }))
                        .child(Icon::new(icon).with_size(px(12.)).text_color(muted))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(summary),
                        )
                        .into_any_element(),
                ];

                if is_expanded {
                    for (key, val) in map {
                        let child_path = format!("{}.{}", path, key);
                        match val {
                            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                                rows.push(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .pl(px(16. * (depth + 1) as f32))
                                        .py(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(text_color)
                                                .child(format!("{}:", key)),
                                        )
                                        .into_any_element(),
                                );
                                rows.extend(self.render_json_tree(val, &child_path, depth + 1, cx));
                            }
                            _ => {
                                let val_el = self.render_json_primitive(val, string_color, number_color, accent, muted);
                                rows.push(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .pl(px(16. * (depth + 1) as f32))
                                        .py(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(text_color)
                                                .child(format!("{}:", key)),
                                        )
                                        .child(val_el)
                                        .into_any_element(),
                                );
                            }
                        }
                    }
                }
                rows
            }
            serde_json::Value::Array(arr) => {
                let is_expanded = !self.json_collapsed.contains(path);
                let path_owned = path.to_string();
                let summary = format!("[{} items]", arr.len());
                let icon = if is_expanded { IconName::ChevronDown } else { IconName::ChevronRight };

                let mut rows: Vec<AnyElement> = vec![
                    div()
                        .id(ElementId::Name(format!("json-{}", path).into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .pl(indent)
                        .py(px(1.))
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.secondary))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            if this.json_collapsed.contains(&path_owned) {
                                this.json_collapsed.remove(&path_owned);
                            } else {
                                this.json_collapsed.insert(path_owned.clone());
                            }
                            cx.notify();
                        }))
                        .child(Icon::new(icon).with_size(px(12.)).text_color(muted))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(summary),
                        )
                        .into_any_element(),
                ];

                if is_expanded {
                    for (i, val) in arr.iter().enumerate() {
                        let child_path = format!("{}[{}]", path, i);
                        match val {
                            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                                rows.push(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .pl(px(16. * (depth + 1) as f32))
                                        .py(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(text_color)
                                                .child(format!("[{}]:", i)),
                                        )
                                        .into_any_element(),
                                );
                                rows.extend(self.render_json_tree(val, &child_path, depth + 1, cx));
                            }
                            _ => {
                                let val_el = self.render_json_primitive(val, string_color, number_color, accent, muted);
                                rows.push(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .pl(px(16. * (depth + 1) as f32))
                                        .py(px(1.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(text_color)
                                                .child(format!("[{}]:", i)),
                                        )
                                        .child(val_el)
                                        .into_any_element(),
                                );
                            }
                        }
                    }
                }
                rows
            }
            _ => {
                let val_el = self.render_json_primitive(value, string_color, number_color, accent, muted);
                vec![
                    div()
                        .pl(indent)
                        .py(px(1.))
                        .child(val_el)
                        .into_any_element(),
                ]
            }
        }
    }

    fn render_json_primitive(&self, value: &serde_json::Value, string_color: Hsla, number_color: Hsla, accent: Hsla, muted: Hsla) -> Div {
        match value {
            serde_json::Value::String(s) => {
                div()
                    .text_size(px(12.))
                    .text_color(string_color)
                    .child(format!("\"{}\"", s))
            }
            serde_json::Value::Number(n) => {
                div()
                    .text_size(px(12.))
                    .text_color(number_color)
                    .child(n.to_string())
            }
            serde_json::Value::Bool(b) => {
                div()
                    .text_size(px(12.))
                    .text_color(accent)
                    .child(b.to_string())
            }
            serde_json::Value::Null => {
                div()
                    .text_size(px(12.))
                    .text_color(muted)
                    .italic()
                    .child("null")
            }
            _ => div().text_size(px(12.)).child(value.to_string()),
        }
    }

    fn render_data_row(
        &self,
        tab: &ResultTab,
        row_idx: usize,
        bg: Hsla,
        surface: Hsla,
        border_color: Hsla,
        text_color: Hsla,
        muted: Hsla,
        number_color: Hsla,
        content_width: Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = &tab.result.rows[row_idx];
        let is_even = row_idx % 2 == 0;
        let row_bg = if is_even { bg } else { surface };

        let mut row_div = div()
            .flex()
            .flex_row()
            .h(px(24.))
            .bg(row_bg)
            .min_w(content_width)
            .border_b_1()
            .border_color(border_color);

        // Row number
        row_div = row_div.child(
            div()
                .flex_shrink_0()
                .w(px(70.))
                .h_full()
                .flex()
                .items_center()
                .justify_end()
                .px_2()
                .border_r_1()
                .border_color(border_color)
                .text_size(px(11.))
                .text_color(muted)
                .child(format!("{}", row_idx + 1)),
        );

        let selected = self.selected_cell;
        let accent = cx.theme().primary;

        for (i, cell) in row.cells.iter().enumerate() {
            let width = tab
                .column_widths
                .get(i)
                .copied()
                .unwrap_or(px(100.));

            let (text, color) = match cell {
                CellValue::Null => ("NULL".to_string(), muted),
                CellValue::Integer(_) | CellValue::Float(_) => (cell.display(), number_color),
                _ => (cell.display(), text_color),
            };

            let is_null = cell.is_null();
            let is_selected = selected == Some((row_idx, i));
            let ri = row_idx;
            let ci = i;

            row_div = row_div.child(
                div()
                    .id(ElementId::Name(
                        format!("cell-{}-{}", row_idx, i).into(),
                    ))
                    .flex_shrink_0()
                    .w(width)
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .border_r_1()
                    .border_color(border_color)
                    .text_size(px(12.))
                    .text_color(color)
                    .when(is_null, |el| el.italic())
                    .when(cell.is_numeric(), |el| el.justify_end())
                    .when(is_selected, |el| el.bg(accent.opacity(0.15)))
                    .overflow_x_hidden()
                    .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
                        if event.click_count() == 2 {
                            this.detail_cell = Some((ri, ci));
                        }
                        this.selected_cell = Some((ri, ci));
                        cx.notify();
                    }))
                    .child(text),
            );
        }

        // Trailing spacer with padding after last column
        row_div = row_div.child(div().w(px(32.)).flex_shrink_0().h_full());

        row_div
    }
}

impl Focusable for ResultsGrid {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ResultsGrid {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_start = std::time::Instant::now();
        let theme = cx.theme();
        let bg = theme.background;
        let surface = theme.secondary;
        let border_color = theme.border;
        let text_color = theme.foreground;
        let muted = theme.muted_foreground;
        let accent = theme.primary;
        let error_color = theme.danger;

        if self.tabs.is_empty() {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(bg)
                .text_size(px(13.))
                .text_color(muted)
                .child("Execute a query to see results");
        }

        let active_tab = self.active_tab;

        // Tab bar (scrollable)
        let mut tab_bar = div()
            .id("results-tab-bar")
            .flex()
            .flex_row()
            .items_center()
            .h(px(28.))
            .bg(bg)
            .border_b_1()
            .border_color(border_color)
            .px_1()
            .gap(px(1.))
            .overflow_x_scroll();

        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = idx == active_tab;
            let tab_id = tab.id;
            let title = tab.title.clone();

            let mut tab_el = div()
                .id(ElementId::Name(format!("rtab-{}", tab_id).into()))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .px_2()
                .py(px(3.))
                .rounded_t_md()
                .cursor_pointer()
                .text_size(px(11.))
                .flex_shrink_0()
                .max_w(px(220.))
                .when(is_active, |el| el.bg(surface).text_color(text_color))
                .when(!is_active, |el| {
                    el.text_color(muted)
                        .hover(|el| el.bg(surface.opacity(0.5)))
                })
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.switch_tab(idx, cx);
                }))
                .child(
                    div()
                        .overflow_x_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                );

            tab_el = tab_el.child(
                div()
                    .id(ElementId::Name(format!("close-rtab-{}", tab_id).into()))
                    .cursor_pointer()
                    .text_size(px(10.))
                    .text_color(muted)
                    .hover(|el| el.text_color(error_color))
                    .rounded_sm()
                    .px(px(2.))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.close_tab(idx, cx);
                    }))
                    .child("×"),
            );

            tab_bar = tab_bar.child(tab_el);
        }

        let tab = &self.tabs[self.active_tab];
        let row_count = tab.result.rows.len();
        let exec_time = tab.result.execution_time_ms;
        let affected = tab.result.affected_rows;
        let col_count = tab.result.columns.len();
        let is_resizing = self.resize_state.is_some();

        // Total content width for horizontal scrolling (row num + columns + trailing pad)
        let total_content_width = px(70.)
            + tab.column_widths.iter().copied().fold(px(0.), |a, b| a + b)
            + px(32.);

        // Build header row with resize handles
        let mut header = div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .min_w(total_content_width)
            .h(px(28.))
            .bg(surface)
            .border_b_1()
            .border_color(border_color);

        // Row number column header
        header = header.child(
            div()
                .flex_shrink_0()
                .w(px(70.))
                .h_full()
                .flex()
                .items_center()
                .px_2()
                .border_r_1()
                .border_color(border_color)
                .text_size(px(11.))
                .text_color(muted)
                .child("#"),
        );

        // Data column headers with resize handles
        let resize_hover = Hsla { a: 0.5, ..accent };
        let open_info_idx = self.column_info_idx;
        for (i, col) in tab.result.columns.iter().enumerate() {
            let width = tab.column_widths.get(i).copied().unwrap_or(px(100.));
            let col_idx = i;

            header = header.child(
                div()
                    .id(ElementId::Name(format!("col-header-{}", i).into()))
                    .flex_shrink_0()
                    .relative()
                    .w(width)
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .gap(px(2.))
                    .border_r_1()
                    .border_color(border_color)
                    .overflow_x_hidden()
                    .group("col-header")
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_x_hidden()
                            .child(col.name.clone()),
                    )
                    // Info icon — subtle normally, full opacity on header hover or when popover open
                    .child(
                        div()
                            .id(ElementId::Name(format!("col-info-{}", i).into()))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .rounded_sm()
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(open_info_idx == Some(i), |el| {
                                el.bg(surface)
                            })
                            .when(open_info_idx != Some(i), |el| {
                                el.opacity(0.15)
                                    .group_hover("col-header", |el| el.opacity(0.7))
                            })
                            .hover(|el| el.bg(surface))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                if this.column_info_idx == Some(col_idx) {
                                    this.column_info_idx = None;
                                } else {
                                    this.column_info_idx = Some(col_idx);
                                }
                                cx.notify();
                            }))
                            .child(
                                Icon::new(IconName::Info)
                                    .with_size(px(12.))
                                    .text_color(muted),
                            ),
                    )
                    // Resize drag handle — on left edge to avoid overlapping info icon
                    .child(
                        div()
                            .id(ElementId::Name(format!("col-resize-{}", i).into()))
                            .absolute()
                            .right_0()
                            .top_0()
                            .h_full()
                            .w(px(4.))
                            .cursor_col_resize()
                            .hover(|el| el.bg(resize_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(
                                    move |this, event: &MouseDownEvent, _window, _cx| {
                                        let width = this
                                            .tabs
                                            .get(this.active_tab)
                                            .and_then(|t| t.column_widths.get(col_idx).copied())
                                            .unwrap_or(px(100.));
                                        this.resize_state = Some(ColumnResize {
                                            col_idx,
                                            start_x: event.position.x,
                                            original_width: width,
                                        });
                                    },
                                ),
                            ),
                    ),
            );
        }

        // Trailing spacer with padding after last column
        header = header.child(div().w(px(32.)).flex_shrink_0().h_full());

        // Column info popover
        let column_info_popover = if let Some(info_idx) = self.column_info_idx {
            let tab = &self.tabs[self.active_tab];
            if let Some(col) = tab.result.columns.get(info_idx) {
                // Calculate x offset: 70px row-number col + sum of widths before this col
                let col_widths_sum: Pixels = tab
                    .column_widths
                    .iter()
                    .take(info_idx)
                    .copied()
                    .fold(px(0.), |a, b| a + b);
                let x_offset = px(70.) + col_widths_sum;

                let col_name = col.name.clone();
                let col_type = col.type_name.clone();

                // Look up schema info
                let schema_info = self.schema.as_ref().and_then(|schema| {
                    for table in &schema.tables {
                        for c in &table.columns {
                            if c.name == col_name {
                                return Some(c.clone());
                            }
                        }
                    }
                    None
                });

                let mut popover = div()
                    .absolute()
                    .top(px(56.)) // below tab bar (28) + header (28)
                    .left(x_offset)
                    .w(px(220.))
                    .bg(bg)
                    .border_1()
                    .border_color(border_color)
                    .rounded_md()
                    .shadow_md()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_size(px(11.))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.))
                            .text_color(text_color)
                            .child(col_name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(muted)
                                    .child("Type:"),
                            )
                            .child(
                                div()
                                    .text_color(text_color)
                                    .child(if col_type.is_empty() {
                                        "unknown".to_string()
                                    } else {
                                        col_type
                                    }),
                            ),
                    );

                if let Some(info) = schema_info {
                    popover = popover
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(muted)
                                        .child("Nullable:"),
                                )
                                .child(
                                    div()
                                        .text_color(text_color)
                                        .child(if info.is_nullable { "Yes" } else { "No" }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(muted)
                                        .child("Primary Key:"),
                                )
                                .child(
                                    div()
                                        .text_color(text_color)
                                        .child(if info.is_primary_key { "Yes" } else { "No" }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(muted)
                                        .child("Default:"),
                                )
                                .child(
                                    div().text_color(text_color).child(
                                        info.default_value
                                            .unwrap_or_else(|| "none".to_string()),
                                    ),
                                ),
                        );
                }

                Some(popover)
            } else {
                None
            }
        } else {
            None
        };

        // Detail panel for cell value inspection
        let detail_panel = if let Some((row_idx, col_idx)) = self.detail_cell {
            let tab = &self.tabs[self.active_tab];
            if let (Some(row), Some(col)) = (
                tab.result.rows.get(row_idx),
                tab.result.columns.get(col_idx),
            ) {
                let raw = row.cells.get(col_idx).map(|c| c.display()).unwrap_or_default();
                let col_name = col.name.clone();

                // Build content: JSON tree or plain text
                let json_parsed = serde_json::from_str::<serde_json::Value>(&raw).ok();
                let is_json = json_parsed.is_some();

                let content = if let Some(ref json_val) = json_parsed {
                    let tree_rows = self.render_json_tree(json_val, "root", 0, cx);
                    div()
                        .flex()
                        .flex_col()
                        .font_family("Monaco")
                        .children(tree_rows)
                } else {
                    div()
                        .text_size(px(12.))
                        .text_color(text_color)
                        .font_family("Monaco")
                        .whitespace_nowrap()
                        .child(raw)
                };

                // Collect all collapsible paths for collapse-all
                let all_paths = if let Some(ref json_val) = json_parsed {
                    let mut paths = Vec::new();
                    Self::collect_json_paths(json_val, "root", &mut paths);
                    paths
                } else {
                    Vec::new()
                };

                let header_row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(32.))
                    .px_3()
                    .bg(surface)
                    .border_b_1()
                    .border_color(border_color)
                    .flex_shrink_0()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color)
                            .child(format!("Row {} \u{00b7} {}", row_idx + 1, col_name)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .when(is_json, |el| {
                                el.child(
                                    div()
                                        .id("detail-expand-all")
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(20.))
                                        .rounded_sm()
                                        .hover(|el| el.bg(bg))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.json_collapsed.clear();
                                            cx.notify();
                                        }))
                                        .child(
                                            Icon::new(IconName::Maximize)
                                                .with_size(px(12.))
                                                .text_color(muted),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("detail-collapse-all")
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(20.))
                                        .rounded_sm()
                                        .hover(|el| el.bg(bg))
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            for p in &all_paths {
                                                this.json_collapsed.insert(p.clone());
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            Icon::new(IconName::Minimize)
                                                .with_size(px(12.))
                                                .text_color(muted),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("detail-close")
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .text_color(muted)
                                    .hover(|el| el.text_color(error_color))
                                    .px(px(4.))
                                    .rounded_sm()
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.detail_cell = None;
                                        cx.notify();
                                    }))
                                    .child("\u{00d7}"),
                            ),
                    );

                Some(
                    div()
                        .occlude()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .w(px(340.))
                        .bg(bg)
                        .border_l_1()
                        .border_color(border_color)
                        .shadow_md()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(header_row)
                        // Content
                        .child(
                            div()
                                .id("detail-content")
                                .flex_1()
                                .overflow_y_scroll()
                                .p_3()
                                .child(content),
                        ),
                )
            } else {
                None
            }
        } else {
            None
        };

        let scroll_handle = tab.scroll_handle.clone();
        let h_scroll_handle = tab.h_scroll_handle.clone();

        let result = div()
            .flex()
            .flex_col()
            .flex_1()
            .relative()
            .bg(bg)
            // Show col-resize cursor across the whole grid while dragging
            .when(is_resizing, |el| el.cursor_col_resize())
            // Track mouse movement for active column resize
            .on_mouse_move(cx.listener(
                |this, event: &MouseMoveEvent, _window, cx| {
                    if let Some(ref resize) = this.resize_state {
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.resize_state = None;
                            return;
                        }
                        let delta = event.position.x - resize.start_x;
                        let new_width =
                            (resize.original_width + delta).max(px(MIN_COL_WIDTH));
                        if let Some(tab) = this.tabs.get_mut(this.active_tab) {
                            if let Some(w) = tab.column_widths.get_mut(resize.col_idx) {
                                *w = new_width;
                            }
                        }
                        cx.notify();
                    }
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, _cx| {
                    this.resize_state = None;
                }),
            )
            // Tab bar
            .child(tab_bar)
            // Header + data area wrapper (relative for scrollbar overlays)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .overflow_hidden()
                    // Horizontally scrollable content
                    .child({
                        let mut h_scroll = div()
                            .id("results-h-scroll")
                            .size_full()
                            .overflow_x_scroll()
                            .overflow_y_hidden()
                            .track_scroll(&h_scroll_handle);
                        h_scroll.interactivity().base_style.restrict_scroll_to_axis = Some(true);
                        h_scroll.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w(total_content_width)
                                    .h_full()
                                    // Header row
                                    .child(header)
                                    // Data rows (virtual scrolled)
                                    .child(
                                        div()
                                            .id("results-scroll-area")
                                            .flex_1()
                                            .relative()
                                            .overflow_hidden()
                                            .child(
                                                uniform_list(
                                                    "results-grid",
                                                    row_count,
                                                    cx.processor(
                                                        move |this: &mut Self,
                                                              range: std::ops::Range<usize>,
                                                              _window: &mut Window,
                                                              cx: &mut Context<Self>| {
                                                            let theme = cx.theme();
                                                            let bg = theme.background;
                                                            let surface = theme.secondary;
                                                            let border_color = theme.border;
                                                            let text_color = theme.foreground;
                                                            let muted =
                                                                theme.muted_foreground;
                                                            let number_color: Hsla =
                                                                gpui::rgb(0xf78c6c).into();
                                                            let active = this.active_tab;
                                                            if let Some(tab) =
                                                                this.tabs.get(active)
                                                            {
                                                                let cw = px(70.)
                                                                    + tab
                                                                        .column_widths
                                                                        .iter()
                                                                        .copied()
                                                                        .fold(
                                                                            px(0.),
                                                                            |a, b| a + b,
                                                                        );
                                                                range
                                                                    .map(|ix| {
                                                                        this.render_data_row(
                                                                            tab,
                                                                            ix,
                                                                            bg,
                                                                            surface,
                                                                            border_color,
                                                                            text_color,
                                                                            muted,
                                                                            number_color,
                                                                            cw,
                                                                            cx,
                                                                        )
                                                                    })
                                                                    .collect()
                                                            } else {
                                                                vec![]
                                                            }
                                                        },
                                                    ),
                                                )
                                                .h_full()
                                                .track_scroll(scroll_handle.clone()),
                                            ),
                                    ),
                            )
                    })
                    // Vertical scrollbar overlay (outside h-scroll so it stays fixed)
                    .child(
                        div()
                            .occlude()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(16.))
                            .child(
                                Scrollbar::vertical(&scroll_handle)
                                    .scrollbar_show(ScrollbarShow::Always),
                            ),
                    )
                    // Horizontal scrollbar overlay (outside h-scroll so it stays fixed)
                    .child(
                        div()
                            .occlude()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(12.))
                            .child(
                                Scrollbar::horizontal(&h_scroll_handle)
                                    .scrollbar_show(ScrollbarShow::Always),
                            ),
                    ),
            )
            // Status bar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(24.))
                    .px_3()
                    .bg(surface)
                    .border_t_1()
                    .border_color(border_color)
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(format!(
                        "{} rows \u{00b7} query: {}ms \u{00b7} render: {:.1}ms \u{00b7} {} columns",
                        affected,
                        exec_time,
                        self.initial_render_ms.unwrap_or(0.0),
                        col_count
                    )),
            )
            // Column info popover overlay — rendered last so it paints on top
            .when_some(column_info_popover, |el, popover| el.child(popover))
            // Detail panel overlay
            .when_some(detail_panel, |el, panel| el.child(panel));

        // Capture initial render time once
        if self.initial_render_ms.is_none() {
            self.initial_render_ms = Some(render_start.elapsed().as_secs_f64() * 1000.0);
        }

        result
    }
}
