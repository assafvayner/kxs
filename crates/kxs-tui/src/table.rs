//! Shared selection viewport for the table views.
//!
//! A plain `render_widget(Table)` always starts at row 0 and clips, so a
//! selection below the fold is invisible and unreachable. `TableState` keeps
//! the offset and scrolls the selected row into view; the cell lets `render`
//! stay `&self`.

use std::cell::RefCell;

use ratatui::layout::Rect;
use ratatui::widgets::{Table, TableState};
use ratatui::Frame;

#[derive(Default)]
pub struct Scroll(RefCell<TableState>);

impl Scroll {
    /// Renders `table` with the offset needed to keep row `selected` visible.
    pub fn render(&self, f: &mut Frame, area: Rect, table: Table<'_>, selected: Option<usize>) {
        let mut state = self.0.borrow_mut();
        state.select(selected);
        f.render_stateful_widget(table, area, &mut state);
    }

    /// First visible row index, for page-sized moves.
    pub fn offset(&self) -> usize {
        self.0.borrow().offset()
    }
}
