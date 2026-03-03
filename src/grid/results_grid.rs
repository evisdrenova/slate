use gpui::prelude::*;
use gpui::*;
use gpui_component::ActiveTheme;

use crate::db::types::{CellValue, QueryResult};

const MIN_COL_WIDTH: f32 = 40.0;

struct ColumnResize {
    col_idx: usize,
    start_x: Pixels,
    original_width: Pixels,
}

pub struct ResultsGrid {
    focus_handle: FocusHandle,
    result: Option<QueryResult>,
    column_widths: Vec<Pixels>,
    scroll_handle: UniformListScrollHandle,
    resize_state: Option<ColumnResize>,
}

impl ResultsGrid {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            result: None,
            column_widths: Vec::new(),
            scroll_handle: UniformListScrollHandle::new(),
            resize_state: None,
        }
    }

    pub fn set_result(&mut self, result: QueryResult, cx: &mut Context<Self>) {
        self.column_widths = result
            .columns
            .iter()
            .map(|col| -> Pixels {
                let min_width = px(80.);
                let max_width = px(300.);
                let header_width = px(col.name.len() as f32 * 8.0 + 24.0);
                header_width.max(min_width).min(max_width)
            })
            .collect();

        self.result = Some(result);
        self.scroll_handle = UniformListScrollHandle::new();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.result = None;
        self.column_widths.clear();
        cx.notify();
    }

    fn render_data_row(
        &self,
        row_idx: usize,
        bg: Hsla,
        surface: Hsla,
        border_color: Hsla,
        text_color: Hsla,
        muted: Hsla,
        number_color: Hsla,
    ) -> Div {
        let Some(result) = &self.result else {
            return div();
        };
        let row = &result.rows[row_idx];
        let is_even = row_idx % 2 == 0;
        let row_bg = if is_even { bg } else { surface };

        let mut row_div = div().flex().flex_row().h(px(24.)).bg(row_bg).w_full();

        // Row number
        row_div = row_div.child(
            div()
                .flex_shrink_0()
                .w(px(50.))
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
            let width = self
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
        row_div = row_div.child(div().flex_1().h_full().border_b_1().border_color(border_color));

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

        if self.result.is_none() {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(bg)
                .text_size(px(13.))
                .text_color(muted)
                .child("Execute a query to see results");
        }

        let result = self.result.as_ref().unwrap();
        let row_count = result.rows.len();
        let exec_time = result.execution_time_ms;
        let affected = result.affected_rows;
        let col_count = result.columns.len();
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
                .w(px(50.))
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
        for (i, col) in result.columns.iter().enumerate() {
            let width = self.column_widths.get(i).copied().unwrap_or(px(100.));
            let col_idx = i;

            header = header.child(
                div()
                    .flex_shrink_0()
                    .relative()
                    .w(width)
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .border_r_1()
                    .border_color(border_color)
                    .overflow_x_hidden()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color)
                            .text_ellipsis()
                            .child(col.name.clone()),
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
                                        this.resize_state = Some(ColumnResize {
                                            col_idx,
                                            start_x: event.position.x,
                                            original_width: this
                                                .column_widths
                                                .get(col_idx)
                                                .copied()
                                                .unwrap_or(px(100.)),
                                        });
                                    },
                                ),
                            ),
                    ),
            );
        }

        // Trailing spacer fills remaining header width
        header = header.child(div().flex_1().h_full());

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
                        if let Some(w) = this.column_widths.get_mut(resize.col_idx) {
                            *w = new_width;
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
            // Header row
            .child(header)
            // Data rows (virtual scrolled)
            .child(
                div().flex_1().child(
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
                                range
                                    .map(|ix| {
                                        this.render_data_row(
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
                            },
                        ),
                    )
                    .h_full()
                    .track_scroll(self.scroll_handle.clone()),
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
