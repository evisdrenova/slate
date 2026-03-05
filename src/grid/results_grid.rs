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
}

pub struct ResultsGrid {
    focus_handle: FocusHandle,
    tabs: Vec<ResultTab>,
    active_tab: usize,
    next_tab_id: usize,
    resize_state: Option<ColumnResize>,
    schema: Option<DatabaseSchema>,
    column_info_idx: Option<usize>,
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

        let title = if sql.len() > 30 {
            format!("{}...", &sql[..30])
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
        };
        self.next_tab_id += 1;

        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;

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
    ) -> Div {
        let row = &tab.result.rows[row_idx];
        let is_even = row_idx % 2 == 0;
        let row_bg = if is_even { bg } else { surface };

        let mut row_div = div()
            .flex()
            .flex_row()
            .h(px(24.))
            .bg(row_bg)
            .w_full()
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

            row_div = row_div.child(
                div()
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
                    .overflow_x_hidden()
                    .child(text),
            );
        }

        // Trailing spacer fills remaining width
        row_div = row_div.child(div().flex_1().h_full());

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

        // Build header row with resize handles
        let mut header = div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .w_full()
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
                            .text_ellipsis()
                            .overflow_x_hidden()
                            .child(col.name.clone()),
                    )
                    // Info icon — visible on hover or when popover is open
                    .child(
                        div()
                            .id(ElementId::Name(format!("col-info-{}", i).into()))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .rounded_sm()
                            .p(px(1.))
                            .when(open_info_idx != Some(i), |el| {
                                el.opacity(0.0)
                                    .group_hover("col-header", |el| el.opacity(1.0))
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
                                    .with_size(px(11.))
                                    .text_color(muted),
                            ),
                    )
                    // Resize drag handle
                    .child(
                        div()
                            .id(ElementId::Name(format!("col-resize-{}", i).into()))
                            .absolute()
                            .right_0()
                            .top_0()
                            .h_full()
                            .w(px(6.))
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

        // Trailing spacer fills remaining header width
        header = header.child(div().flex_1().h_full());

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
                    .top(px(28.)) // below header
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

        let scroll_handle = tab.scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
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
            // Header row + info popover
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .child(header)
                    .when_some(column_info_popover, |el, popover| el.child(popover)),
            )
            // Data rows (virtual scrolled) with scrollbar
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
                                    let muted = theme.muted_foreground;
                                    let number_color: Hsla = gpui::rgb(0xf78c6c).into();
                                    let active = this.active_tab;
                                    if let Some(tab) = this.tabs.get(active) {
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
                    )
                    // Vertical scrollbar overlay
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
                        "{} rows \u{00b7} {}ms \u{00b7} {} columns",
                        affected, exec_time, col_count
                    )),
            )
    }
}
