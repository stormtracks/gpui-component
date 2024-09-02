use std::{cell::Cell, ops::Range, rc::Rc};

use crate::{
    h_flex,
    scroll::{ScrollableAxis, ScrollableMask, Scrollbar, ScrollbarState},
    theme::{ActiveTheme, Colorize},
    v_flex, Icon, IconName,
};
use gpui::{
    actions, canvas, div, prelude::FluentBuilder, px, uniform_list, AppContext, Bounds, Div,
    DragMoveEvent, Entity, EntityId, EventEmitter, FocusHandle, FocusableView, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, ParentElement, Pixels, Point, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled, UniformListScrollHandle, ViewContext,
    VisualContext as _, WindowContext,
};

actions!(
    table,
    [
        Cancel,
        SelectPrev,
        SelectNext,
        SelectPrevColumn,
        SelectNextColumn
    ]
);

pub fn init(cx: &mut AppContext) {
    let context = Some("Table");
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("left", SelectPrevColumn, context),
        KeyBinding::new("right", SelectNextColumn, context),
    ]);
}

#[derive(Debug, Clone, Copy)]
struct ColGroup {
    width: Option<Pixels>,
    bounds: Bounds<Pixels>,
    sort: Option<ColSort>,
}

#[derive(Clone)]
pub(crate) struct DragCol {
    pub(crate) entity_id: EntityId,
    pub(crate) name: SharedString,
    pub(crate) width: Option<Pixels>,
    pub(crate) col_ix: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColSort {
    /// No sorting.
    Default,
    /// Sort in ascending order.
    Ascending,
    /// Sort in descending order.
    Descending,
}

impl Render for DragCol {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .px_4()
            .py_1()
            .bg(cx.theme().table_head)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_md()
            .when_some(self.width, |this, width| this.w(width))
            .min_w(px(100.))
            .max_w(px(450.))
            .child(self.name.clone())
    }
}

#[derive(Clone, Render)]
pub struct ResizeCol(pub (EntityId, usize));

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SelectionState {
    Column,
    Row,
}

#[derive(Clone)]
pub enum TableEvent {
    SelectRow(usize),
    SelectCol(usize),
    ColWidthsChanged(Vec<Option<Pixels>>),
}

pub struct Table<D: TableDelegate> {
    focus_handle: FocusHandle,
    delegate: D,
    /// The bounds of the table.
    bounds: Bounds<Pixels>,
    horizontal_scroll_handle: ScrollHandle,
    vertical_scroll_handle: UniformListScrollHandle,
    col_groups: Vec<ColGroup>,

    scrollbar_state: Rc<Cell<ScrollbarState>>,

    selection_state: SelectionState,
    selected_row: Option<usize>,
    selected_col: Option<usize>,

    /// The column index that is being resized.
    resizing_col: Option<usize>,

    /// Set stripe style of the table.
    stripe: bool,
    /// Set to use border style of the table.
    border: bool,
}

#[allow(unused)]
pub trait TableDelegate: Sized + 'static {
    /// Return the number of columns in the table.
    fn cols_count(&self) -> usize;
    /// Return the number of rows in the table.
    fn rows_count(&self) -> usize;

    /// Returns the name of the column at the given index.
    fn col_name(&self, col_ix: usize) -> SharedString;

    /// Returns whether the column at the given index can be resized. Default: true
    fn can_resize_col(&self, col_ix: usize) -> bool {
        true
    }

    /// Returns whether the column at the given index can be selected. Default: false
    fn can_select_col(&self, col_ix: usize) -> bool {
        false
    }

    /// Returns the width of the column at the given index.
    /// Return None, use auto width.
    ///
    /// This is only called when the table initializes.
    fn col_width(&self, col_ix: usize) -> Option<Pixels>;

    /// Return the sort state of the column at the given index.
    ///
    /// This is only called when the table initializes.
    fn col_sort(&self, col_ix: usize) -> Option<ColSort> {
        None
    }

    /// Perform sort on the column at the given index.
    fn perform_sort(&mut self, col_ix: usize, sort: ColSort, cx: &mut ViewContext<Table<Self>>) {}

    /// Render the header cell at the given column index, default to the column name.
    fn render_th(&self, col_ix: usize, cx: &mut ViewContext<Table<Self>>) -> impl IntoElement {
        div().size_full().child(self.col_name(col_ix))
    }

    /// Render the row at the given row and column.
    fn render_tr(&self, row_ix: usize, cx: &mut ViewContext<Table<Self>>) -> Div {
        h_flex()
    }

    /// Render cell at the given row and column.
    fn render_td(
        &self,
        row_ix: usize,
        col_ix: usize,
        cx: &mut ViewContext<Table<Self>>,
    ) -> impl IntoElement;

    /// Return true to enable loop selection on the table.
    ///
    /// When the prev/next selection is out of the table bounds, the selection will loop to the other side.
    ///
    /// Default: true
    fn can_loop_select(&self) -> bool {
        true
    }

    /// Return true to enable column order change.
    fn can_move_col(&self, col_ix: usize) -> bool {
        false
    }

    /// Move the column at the given `col_ix` to insert before the column at the given `to_ix`.
    fn move_col(&mut self, col_ix: usize, to_ix: usize) {}

    /// Return a Element to show when table is empty.
    fn render_empty(&self, cx: &mut ViewContext<Table<Self>>) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .py_6()
            .text_color(cx.theme().muted_foreground.opacity(0.6))
            .child(Icon::new(IconName::Inbox).size_12())
            .into_any_element()
    }

    /// Return true to enable load more data when scrolling to the bottom.
    ///
    /// Default: true
    fn can_load_more(&self) -> bool {
        true
    }

    /// Returns a threshold value (n rows), of course, when scrolling to the bottom,
    /// the remaining number of rows triggers `load_more`.
    ///
    /// Default: 50 rows
    fn load_more_threshold(&self) -> usize {
        50
    }

    /// Load more data when the table is scrolled to the bottom.
    ///
    /// This will performed in a background task.
    ///
    /// This is always called when the table is near the bottom,
    /// so you must check if there is more data to load or lock the loading state.
    fn load_more(&mut self, cx: &mut ViewContext<Table<Self>>) {}
}

impl<D> Table<D>
where
    D: TableDelegate,
{
    pub fn new(delegate: D, cx: &mut ViewContext<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            delegate,
            col_groups: Vec::new(),
            horizontal_scroll_handle: ScrollHandle::new(),
            vertical_scroll_handle: UniformListScrollHandle::new(),
            scrollbar_state: Rc::new(Cell::new(ScrollbarState::new())),
            selection_state: SelectionState::Row,
            selected_row: None,
            selected_col: None,
            resizing_col: None,
            bounds: Bounds::default(),
            stripe: false,
            border: true,
        };

        this.prepare_col_groups(cx);
        this
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    /// Set to use stripe style of the table, default to false.
    pub fn stripe(mut self, stripe: bool) -> Self {
        self.stripe = stripe;
        self
    }

    /// Set to use border style of the table, default to true.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    fn prepare_col_groups(&mut self, cx: &mut ViewContext<Self>) {
        self.col_groups = (0..self.delegate.cols_count())
            .map(|col_ix| ColGroup {
                width: self.delegate.col_width(col_ix),
                bounds: Bounds::default(),
                sort: self.delegate.col_sort(col_ix),
            })
            .collect();
        cx.notify();
    }

    fn set_selected_row(&mut self, row_ix: usize, cx: &mut ViewContext<Self>) {
        self.selection_state = SelectionState::Row;
        self.selected_row = Some(row_ix);
        if let Some(row_ix) = self.selected_row {
            self.vertical_scroll_handle.scroll_to_item(row_ix);
        }
        cx.emit(TableEvent::SelectRow(row_ix));
        cx.notify();
    }

    fn set_selected_col(&mut self, col_ix: usize, cx: &mut ViewContext<Self>) {
        self.selection_state = SelectionState::Column;
        self.selected_col = Some(col_ix);
        if let Some(col_ix) = self.selected_col {
            self.horizontal_scroll_handle.scroll_to_item(col_ix);
        }
        cx.emit(TableEvent::SelectCol(col_ix));
        cx.notify();
    }

    fn on_row_click(&mut self, row_ix: usize, cx: &mut ViewContext<Self>) {
        self.set_selected_row(row_ix, cx)
    }

    fn on_col_head_click(&mut self, col_ix: usize, cx: &mut ViewContext<Self>) {
        if !self.delegate.can_select_col(col_ix) {
            return;
        }

        self.set_selected_col(col_ix, cx)
    }

    fn action_cancel(&mut self, _: &Cancel, cx: &mut ViewContext<Self>) {
        self.selection_state = SelectionState::Row;
        self.selected_row = None;
        self.selected_col = None;
        cx.notify();
    }

    fn action_select_prev(&mut self, _: &SelectPrev, cx: &mut ViewContext<Self>) {
        let mut selected_row = self.selected_row.unwrap_or(0);
        let rows_count = self.delegate.rows_count();
        if selected_row > 0 {
            selected_row = selected_row - 1;
        } else {
            if self.delegate.can_loop_select() {
                selected_row = rows_count - 1;
            }
        }

        self.set_selected_row(selected_row, cx);
    }

    fn action_select_next(&mut self, _: &SelectNext, cx: &mut ViewContext<Self>) {
        let mut selected_row = self.selected_row.unwrap_or(0);
        if selected_row < self.delegate.rows_count() - 1 {
            selected_row += 1;
        } else {
            if self.delegate.can_loop_select() {
                selected_row = 0;
            }
        }

        self.set_selected_row(selected_row, cx);
    }

    fn action_select_prev_col(&mut self, _: &SelectPrevColumn, cx: &mut ViewContext<Self>) {
        let mut selected_col = self.selected_col.unwrap_or(0);
        let cols_count = self.delegate.cols_count();
        if selected_col > 0 {
            selected_col -= 1;
        } else {
            if self.delegate.can_loop_select() {
                selected_col = cols_count - 1;
            }
        }
        self.set_selected_col(selected_col, cx);
    }

    fn action_select_next_col(&mut self, _: &SelectNextColumn, cx: &mut ViewContext<Self>) {
        let mut selected_col = self.selected_col.unwrap_or(0);
        if selected_col < self.delegate.cols_count() - 1 {
            selected_col += 1;
        } else {
            if self.delegate.can_loop_select() {
                selected_col = 0;
            }
        }

        self.set_selected_col(selected_col, cx);
    }

    fn render_cell(&self, col_ix: usize, _cx: &mut ViewContext<Self>) -> Div {
        let col_width = self.col_groups[col_ix].width;

        div()
            .when_some(col_width, |this, width| this.w(width))
            .overflow_hidden()
            .whitespace_nowrap()
            .py_1()
            .px_2()
    }

    /// Show Column selection style, when the column is selected and the selection state is Column.
    fn col_wrap(&self, col_ix: usize, cx: &mut ViewContext<Self>) -> Div {
        if self.delegate().can_select_col(col_ix)
            && self.selected_col == Some(col_ix)
            && self.selection_state == SelectionState::Column
        {
            h_flex().bg(cx.theme().table_active)
        } else {
            h_flex()
        }
    }

    fn render_scrollbar(&self, cx: &mut ViewContext<Self>) -> Option<impl IntoElement> {
        let state = self.scrollbar_state.clone();

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .child(Scrollbar::uniform_scroll(
                    cx.view().entity_id(),
                    state,
                    self.vertical_scroll_handle.clone(),
                    self.delegate.rows_count(),
                )),
        )
    }

    fn render_resize_handle(&self, ix: usize, cx: &mut ViewContext<Self>) -> impl IntoElement {
        const HANDLE_SIZE: Pixels = px(2.);

        if !self.delegate.can_resize_col(ix) {
            return div().into_any_element();
        }

        h_flex()
            .id(("resizable-handle", ix))
            .occlude()
            .cursor_col_resize()
            .h_full()
            .w(HANDLE_SIZE)
            .ml(-(HANDLE_SIZE))
            .justify_end()
            .items_center()
            .child(
                div()
                    .h_full()
                    .h_5()
                    .justify_center()
                    .bg(cx.theme().border)
                    .w(px(1.)),
            )
            .on_drag_move(cx.listener(move |view, e: &DragMoveEvent<ResizeCol>, cx| {
                match e.drag(cx) {
                    ResizeCol((entity_id, ix)) => {
                        if cx.entity_id() != *entity_id {
                            return;
                        }

                        // sync col widths into real widths
                        for (_, col_group) in view.col_groups.iter_mut().enumerate() {
                            col_group.width = Some(col_group.bounds.size.width);
                        }

                        let ix = *ix;
                        view.resizing_col = Some(ix);

                        let col_group = *view.col_groups.get(ix).expect("BUG: invalid col index");

                        view.resize_cols(
                            ix,
                            e.event.position.x - HANDLE_SIZE - col_group.bounds.left(),
                            cx,
                        );

                        // scroll the table if the drag is near the edge
                        view.scroll_table_by_col_resizing(e.event.position, col_group, cx);
                    }
                };
            }))
            .on_drag(ResizeCol((cx.entity_id(), ix)), |drag, cx| {
                cx.stop_propagation();
                cx.new_view(|_| drag.clone())
            })
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, cx| {
                    if view.resizing_col.is_none() {
                        return;
                    }

                    view.resizing_col = None;

                    let new_widths = view.col_groups.iter().map(|g| g.width).collect();
                    cx.emit(TableEvent::ColWidthsChanged(new_widths));
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// Scroll table when mouse position is near the edge of the table bounds.
    fn scroll_table_by_col_resizing(
        &mut self,
        pos: Point<Pixels>,
        col_group: ColGroup,
        _: &mut ViewContext<Self>,
    ) {
        let mut offset = self.horizontal_scroll_handle.offset();
        let col_bounds = col_group.bounds;

        if pos.x < self.bounds.left() && col_bounds.right() < self.bounds.left() + px(20.) {
            offset.x += px(1.);
        } else if pos.x > self.bounds.right() && col_bounds.right() > self.bounds.right() - px(20.)
        {
            offset.x -= px(1.);
        }

        self.horizontal_scroll_handle.set_offset(offset);
    }

    /// The `ix`` is the index of the col to resize,
    /// and the `size` is the new size for the col.
    fn resize_cols(&mut self, ix: usize, size: Pixels, cx: &mut ViewContext<Self>) {
        const MIN_WIDTH: Pixels = px(10.0);
        const MAX_WIDTH: Pixels = px(1200.0);

        if !self.delegate.can_resize_col(ix) {
            return;
        }
        let size = size.floor();

        let old_width = self.col_groups[ix].width.unwrap_or_default();
        let new_width = size;
        if new_width < MIN_WIDTH {
            return;
        }
        let changed_width = new_width - old_width;
        // If change size is less than 1px, do nothing.
        if changed_width > px(-1.0) && changed_width < px(1.0) {
            return;
        }
        self.col_groups[ix].width = Some(new_width.min(MAX_WIDTH));

        // Resize next col, table not need to resize the right cols.
        // let next_width = self.col_groups[ix + 1].width.unwrap_or_default();
        // let next_width = (next_width - changed_width).max(MIN_WIDTH);
        // self.col_groups[ix + 1].width = Some(next_width);

        cx.notify();
    }

    fn perform_sort(&mut self, col_ix: usize, cx: &mut ViewContext<Self>) {
        let sort = self.col_groups.get(col_ix).and_then(|g| g.sort);
        if sort.is_none() {
            return;
        }

        let sort = sort.unwrap();
        let sort = match sort {
            ColSort::Ascending => ColSort::Descending,
            ColSort::Descending => ColSort::Ascending,
            ColSort::Default => ColSort::Ascending,
        };

        for (ix, col_group) in self.col_groups.iter_mut().enumerate() {
            if ix == col_ix {
                col_group.sort = Some(sort);
            } else {
                col_group.sort = Some(ColSort::Default);
            }
        }

        self.delegate_mut().perform_sort(col_ix, sort, cx);

        cx.notify();
    }

    fn render_sort_icon(
        &self,
        col_ix: usize,
        cx: &mut ViewContext<Self>,
    ) -> Option<impl IntoElement> {
        let sort = self.delegate().col_sort(col_ix);
        if sort.is_none() {
            return None;
        }

        let sort = sort.unwrap();

        let icon = match sort {
            ColSort::Ascending => IconName::SortAscending,
            ColSort::Descending => IconName::SortDescending,
            ColSort::Default => IconName::ChevronsUpDown,
        };

        Some(
            div()
                .id(("icon-sort", col_ix))
                .cursor_pointer()
                .ml_2()
                .p(px(2.))
                .rounded_sm()
                .hover(|this| this.bg(cx.theme().secondary))
                .active(|this| this.bg(cx.theme().secondary_active))
                .on_mouse_down(MouseButton::Left, |_, cx| cx.stop_propagation())
                .on_click(cx.listener(move |table, _, cx| table.perform_sort(col_ix, cx)))
                .child(
                    Icon::new(icon)
                        .size_3()
                        .text_color(cx.theme().secondary_foreground),
                ),
        )
    }

    /// Render the column header.
    /// The children must be one by one items.
    /// Because the horizontal scroll handle will use the child_item_bounds to
    /// calculate the item position for itself's `scroll_to_item` method.
    fn render_th(&self, col_ix: usize, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let col_group = self.col_groups.get(col_ix).expect("BUG: invalid col index");

        let name = self.delegate.col_name(col_ix);
        h_flex()
            .child(
                self.render_cell(col_ix, cx)
                    .id(("col-header", col_ix))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, cx| {
                            cx.stop_propagation();
                            this.on_col_head_click(col_ix, cx);
                        }),
                    )
                    .child(
                        h_flex()
                            .size_full()
                            .justify_between()
                            .items_center()
                            .child(self.delegate.render_th(col_ix, cx))
                            .children(self.render_sort_icon(col_ix, cx)),
                    )
                    .when(self.delegate.can_move_col(col_ix), |this| {
                        this.on_drag(
                            DragCol {
                                entity_id,
                                col_ix,
                                name,
                                width: col_group.width,
                            },
                            |drag, cx| {
                                cx.stop_propagation();
                                cx.new_view(|_| drag.clone())
                            },
                        )
                        .drag_over::<DragCol>(|this, _, cx| {
                            this.rounded_l_none()
                                .border_l_2()
                                .border_r_0()
                                .border_color(cx.theme().drag_border)
                        })
                        .on_drop(cx.listener(
                            move |table, drag: &DragCol, cx| {
                                // If the drag col is not the same as the drop col, then swap the cols.
                                if drag.entity_id != cx.entity_id() {
                                    return;
                                }

                                table.move_col(drag.col_ix, col_ix, cx);
                            },
                        ))
                    }),
            )
            // resize handle
            .child(self.render_resize_handle(col_ix, cx))
            // to save the bounds of this col.
            .child({
                let view = cx.view().clone();
                canvas(
                    move |bounds, cx| view.update(cx, |r, _| r.col_groups[col_ix].bounds = bounds),
                    |_, _, _| {},
                )
                .absolute()
                .size_full()
            })
    }

    /// Dispatch delegate's `load_more` method when the visible range is near the end.
    fn load_more(&mut self, visible_range: Range<usize>, cx: &mut ViewContext<Self>) {
        if !self.delegate.can_load_more() {
            return;
        }

        let row_count = self.delegate.rows_count();
        let load_more_count = self.delegate.load_more_threshold();

        // Securely handle subtract logic to prevent attempt to subtract with overflow
        if row_count >= load_more_count {
            if visible_range.end >= row_count - load_more_count {
                cx.spawn(|view, mut cx| async move {
                    cx.update(|cx| {
                        view.update(cx, |view, cx| {
                            view.delegate.load_more(cx);
                        })
                    })
                })
                .detach()
            }
        }
    }

    fn move_col(&mut self, col_ix: usize, to_ix: usize, cx: &mut ViewContext<Self>) {
        if col_ix == to_ix {
            return;
        }

        self.delegate.move_col(col_ix, to_ix);
        let col_group = self.col_groups.remove(col_ix);
        self.col_groups.insert(to_ix, col_group);

        cx.notify();
    }
}

impl<D> FocusableView for Table<D>
where
    D: TableDelegate,
{
    fn focus_handle(&self, _cx: &gpui::AppContext) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<D> EventEmitter<TableEvent> for Table<D> where D: TableDelegate {}

impl<D> Render for Table<D>
where
    D: TableDelegate,
{
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let view = cx.view().clone();
        let vertical_scroll_handle = self.vertical_scroll_handle.clone();
        let horizontal_scroll_handle = self.horizontal_scroll_handle.clone();
        let cols_count: usize = self.delegate.cols_count();
        let rows_count = self.delegate.rows_count();

        fn last_empty_col(_: &mut WindowContext) -> Div {
            h_flex().w(px(100.)).h_full().flex_shrink_0()
        }

        fn tr(_: &mut WindowContext) -> Div {
            h_flex()
        }

        let inner_table = v_flex()
            .key_context("Table")
            .id("table")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::action_cancel))
            .on_action(cx.listener(Self::action_select_next))
            .on_action(cx.listener(Self::action_select_prev))
            .on_action(cx.listener(Self::action_select_next_col))
            .on_action(cx.listener(Self::action_select_prev_col))
            .size_full()
            .overflow_hidden()
            .child(
                v_flex()
                    .flex_grow()
                    .h_10()
                    .w_full()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        uniform_list(view.clone(), "table-uniform-list-head", 1, {
                            let horizontal_scroll_handle = horizontal_scroll_handle.clone();
                            move |table, _, cx| {
                                // Columns
                                tr(cx)
                                    .id("table-head")
                                    .w_full()
                                    .h_10()
                                    .overflow_scroll()
                                    .track_scroll(&horizontal_scroll_handle)
                                    .bg(cx.theme().table_head)
                                    .children(
                                        table
                                            .col_groups
                                            .iter()
                                            .enumerate()
                                            .map(|(col_ix, _)| table.render_th(col_ix, cx)),
                                    )
                                    .child(last_empty_col(cx))
                                    .map(|this| vec![this])
                            }
                        })
                        .size_full(),
                    ),
            )
            .map(|this| {
                if rows_count == 0 {
                    this.child(div().size_full().child(self.delegate.render_empty(cx)))
                } else {
                    this.child(
                        h_flex().id("table-body").flex_grow().size_full().child(
                            uniform_list(view, "table-uniform-list", rows_count, {
                                let horizontal_scroll_handle = horizontal_scroll_handle.clone();
                                move |table, visible_range, cx| {
                                    table.load_more(visible_range.clone(), cx);

                                    visible_range
                                        .map(|row_ix| {
                                            table
                                                .delegate
                                                .render_tr(row_ix, cx)
                                                .id(("table-row", row_ix))
                                                .w_full()
                                                .when(row_ix > 0, |this| {
                                                    this.border_t_1()
                                                        .border_color(cx.theme().border)
                                                })
                                                .when(table.stripe && row_ix % 2 != 0, |this| {
                                                    this.bg(cx.theme().table_even)
                                                })
                                                .hover(|this| {
                                                    if table.selected_row == Some(row_ix) {
                                                        this
                                                    } else {
                                                        this.bg(cx.theme().table_hover)
                                                    }
                                                })
                                                .children((0..cols_count).map(|col_ix| {
                                                    table
                                                        .col_wrap(col_ix, cx) // Make the row scroll sync with the horizontal_scroll_handle to support horizontal scrolling.
                                                        .left(horizontal_scroll_handle.offset().x)
                                                        .child(
                                                            table
                                                                .render_cell(col_ix, cx)
                                                                .flex_shrink_0()
                                                                .child(
                                                                    table.delegate.render_td(
                                                                        row_ix, col_ix, cx,
                                                                    ),
                                                                ),
                                                        )
                                                }))
                                                .child(last_empty_col(cx))
                                                // Row selected style
                                                .when_some(
                                                    table.selected_row,
                                                    |this, selected_row| {
                                                        this.when(
                                                            row_ix == selected_row
                                                                && table.selection_state
                                                                    == SelectionState::Row,
                                                            |this| this.bg(cx.theme().table_active),
                                                        )
                                                    },
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, cx| {
                                                        this.on_row_click(row_ix, cx);
                                                    }),
                                                )
                                        })
                                        .collect::<Vec<_>>()
                                }
                            })
                            .flex_grow()
                            .size_full()
                            .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
                            .track_scroll(vertical_scroll_handle)
                            .into_any_element(),
                        ),
                    )
                }
            });

        let view = cx.view().clone();
        div()
            .size_full()
            .when(self.border, |this| {
                this.rounded_md().border_1().border_color(cx.theme().border)
            })
            .bg(cx.theme().table)
            .child(inner_table)
            .child(ScrollableMask::new(
                cx.view().clone(),
                ScrollableAxis::Horizontal,
                &horizontal_scroll_handle,
            ))
            .child(canvas(
                move |bounds, cx| view.update(cx, |r, _| r.bounds = bounds),
                |_, _, _| {},
            ))
            .when(rows_count > 0, |this| {
                this.children(self.render_scrollbar(cx))
            })
    }
}
