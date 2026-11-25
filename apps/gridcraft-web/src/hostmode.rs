//! Pure host-mode rules, shared by the bridge ([`crate::host`]) and testable without a browser
//! (like [`crate::prefs`]): the name the bridge fills in when a host's open request has none,
//! whether the page asked for a view-only window, and what the host hears about the selection
//! and when. Which pages host the app is `craft-web-host`'s.

use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;

/// The file name used when an open request carries no usable one.
pub const DEFAULT_OPEN_NAME: &str = "Book.xlsx";

/// The name `craft-web-host` gives an open request that carries no usable one (it cleans a host's
/// name and falls back to this).
const NAMELESS: &str = "document";

/// A usable file name for an open request: the cleaned name the crate hands over, or
/// [`DEFAULT_OPEN_NAME`] when the host sent none.
pub fn open_name(name: &str) -> String {
    if name == NAMELESS { DEFAULT_OPEN_NAME.to_string() } else { name.to_string() }
}

/// `?mode=view`: the page shows workbooks for looking through only (`Session::view_only`). Any
/// other value, or none, is the ordinary editor.
pub fn view_mode(mode: Option<&str>) -> bool {
    mode == Some("view")
}

/// `?mode=entry`: people fill in cells but change no structure or format (`Session::entry_only`).
pub fn entry_mode(mode: Option<&str>) -> bool {
    mode == Some("entry")
}

/// The most cells (and rows) one `gridcraft:change` lists; `range` still covers them all.
pub const MAX_LISTED: usize = 500;

/// What `gridcraft:change` tells the host: the cells one command of a person's changed on one
/// sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNote {
    /// The sheet's name.
    pub sheet: String,
    /// The smallest range holding every changed cell, in A1 notation (`C7`, `B3:D9`).
    pub range: String,
    /// The changed cells in A1 notation, in row order, at most [`MAX_LISTED`].
    pub cells: Vec<String>,
    /// More cells changed than `cells` lists.
    pub truncated: bool,
    /// The changed rows (1-based, as the sheet numbers them) with the text column A shows in each
    /// (the row's key, as in `gridcraft:selection`), at most [`MAX_LISTED`].
    pub rows: Vec<(u32, String)>,
}

/// The notes for a person's changes (`Session::changes`), read against the workbooks as they are
/// now. A change to a workbook that has closed since is dropped.
pub fn change_notes(app: &SheetApp, changes: Vec<gridcraft_engine::changes::Change>) -> Vec<ChangeNote> {
    let mut out = Vec::new();
    for c in changes {
        let Some(doc) = app.session.documents().iter().find(|d| d.uid == c.doc) else { continue };
        let Some(sheet) = doc.wb.sheet(c.sheet_index) else { continue };
        let (Some(first), Some(last)) = (c.cells.first(), c.cells.last()) else { continue };
        let (mut top, mut left, mut bottom, mut right) = (first.row, first.col, last.row, first.col);
        for cell in &c.cells {
            left = left.min(cell.col);
            right = right.max(cell.col);
            top = top.min(cell.row);
            bottom = bottom.max(cell.row);
        }
        let range = gridcraft_engine::core::RangeRef::new(CellRef::new(top, left), CellRef::new(bottom, right)).a1();
        let mut rows: Vec<u32> = c.cells.iter().map(|cell| cell.row).collect();
        rows.dedup();
        let rows = rows
            .into_iter()
            .take(MAX_LISTED)
            .map(|row| (row.saturating_add(1), gridcraft_engine::display::cell_text(&doc.wb, sheet, CellRef::new(row, 0))))
            .collect();
        let truncated = c.cells.len() > MAX_LISTED;
        let cells = c.cells.iter().take(MAX_LISTED).map(|cell| cell.a1()).collect();
        out.push(ChangeNote { sheet: c.sheet, range, cells, truncated, rows });
    }
    out
}

/// What `gridcraft:selection` tells the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionNote {
    /// The active sheet's name.
    pub sheet: String,
    /// The selected areas in A1 notation (`B2:D5`, or `A1,C3` for several).
    pub selection: String,
    /// The active cell in A1 notation.
    pub active_cell: String,
    /// The text shown in column A of the active cell's row. A host that keeps a key there (the
    /// row's number in its own data) knows the row from it after a sort or a filter, when the
    /// sheet's row number no longer says.
    pub row_key: String,
}

/// The selection of the active workbook, with that workbook's id (so a newly opened workbook
/// counts as a change), or `None` when no workbook is open.
pub fn selection_note(app: &SheetApp) -> Option<(u64, SelectionNote)> {
    let doc = app.session.active()?;
    let sheet = doc.wb.active()?;
    let active = doc.selection.active;
    let row_key = gridcraft_engine::display::cell_text(&doc.wb, sheet, CellRef::new(active.row, 0));
    Some((doc.uid, SelectionNote { sheet: sheet.name.clone(), selection: doc.selection.a1(), active_cell: active.a1(), row_key }))
}

/// Decides when the host hears about the selection: once whenever it differs from what was last
/// told, and not while a pointer button is down (a drag tells its final selection, not every
/// cell it passed).
#[derive(Default)]
pub struct SelectionWatch {
    told: Option<(u64, SelectionNote)>,
}

impl SelectionWatch {
    /// The note to send for the selection `now`, if the host has not heard it yet.
    pub fn note(&mut self, now: Option<(u64, SelectionNote)>, pointer_down: bool) -> Option<SelectionNote> {
        if pointer_down {
            return None;
        }
        let Some(now) = now else {
            self.told = None;
            return None;
        };
        if self.told.as_ref() == Some(&now) {
            return None;
        }
        let note = now.1.clone();
        self.told = Some(now);
        Some(note)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::Services;
    use serde_json::json;

    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range": "A1", "values": [["Row", "Task"], [7, "beta"], [3, "alpha"]]})).unwrap();
        SheetApp::new(session, Services::default())
    }

    fn select(app: &mut SheetApp, range: &str) -> SelectionNote {
        app.session.execute("selection.set", json!({"range": range})).unwrap();
        selection_note(app).unwrap().1
    }

    #[test]
    fn only_mode_entry_asks_for_an_entry_only_window() {
        assert!(entry_mode(Some("entry")));
        for other in [None, Some("view"), Some("Entry"), Some("")] {
            assert!(!entry_mode(other), "{other:?} is not entry-only");
        }
    }

    /// The notes for what a person did to `app` with `run`.
    fn notes_after(app: &mut SheetApp, run: impl FnOnce(&mut SheetApp)) -> Vec<ChangeNote> {
        app.session.changes = Some(Vec::new());
        run(app);
        let changes = app.session.changes.take().unwrap_or_default();
        change_notes(app, changes)
    }

    #[test]
    fn a_change_note_names_the_cells_their_range_and_their_rows_keys() {
        let mut app = app();
        let notes = notes_after(&mut app, |app| {
            app.session.execute("range.setValues", json!({"range": "B2", "values": [["gamma"], ["delta"]]})).unwrap();
        });
        let [note] = notes.as_slice() else { panic!("one sheet changed: {notes:?}") };
        assert_eq!(note.range, "B2:B3");
        assert_eq!(note.cells, ["B2", "B3"]);
        assert_eq!(note.rows, [(2, "7".to_string()), (3, "3".to_string())], "each row with its column A");
        assert!(!note.truncated);
    }

    #[test]
    fn a_large_change_is_one_range_with_its_list_cut_short() {
        let mut app = app();
        let notes = notes_after(&mut app, |app| {
            let values: Vec<Vec<i32>> = (0..300).map(|r| vec![r, r]).collect();
            app.session.execute("range.setValues", json!({"range": "E1", "values": values})).unwrap();
        });
        let [note] = notes.as_slice() else { panic!("one sheet changed: {notes:?}") };
        assert_eq!(note.range, "E1:F300");
        assert!(note.truncated && note.cells.len() == MAX_LISTED, "600 cells, the first {MAX_LISTED} listed");
        assert_eq!(note.rows.len(), 300, "every row is listed");
    }

    #[test]
    fn only_mode_view_asks_for_a_view_only_window() {
        assert!(view_mode(Some("view")));
        for other in [None, Some(""), Some("edit"), Some("View"), Some("viewer")] {
            assert!(!view_mode(other), "{other:?} is the ordinary editor");
        }
    }

    #[test]
    fn the_selection_note_names_the_cells_and_the_rows_key() {
        let mut app = app();
        let note = select(&mut app, "B2:B3");
        assert_eq!((note.selection.as_str(), note.active_cell.as_str()), ("B2:B3", "B2"));
        assert_eq!(note.row_key, "7", "column A of the active cell's row");
        assert_eq!(note.sheet, app.session.active().unwrap().wb.active().unwrap().name);
    }

    #[test]
    fn the_rows_key_follows_the_row_through_a_sort() {
        let mut app = app();
        let before = select(&mut app, "B2");
        app.session.execute("data.sortAscending", json!({"column": "B"})).unwrap();
        let after = selection_note(&app).unwrap().1;
        assert_eq!(before.active_cell, after.active_cell, "the same cell is active");
        assert_ne!(before.row_key, after.row_key, "but another row is in it now");
    }

    #[test]
    fn the_host_hears_a_selection_once_and_a_drag_only_when_it_ends() {
        let mut app = app();
        let mut watch = SelectionWatch::default();
        let first = selection_note(&app);
        assert_eq!(watch.note(first.clone(), false), first.clone().map(|(_, n)| n), "the first selection is told");
        assert_eq!(watch.note(first, false), None, "and not told again while it stays");

        // A drag passes B2:B3 on its way to B2:C3.
        let passed = select(&mut app, "B2:B3");
        assert_eq!(watch.note(selection_note(&app), true), None, "nothing while the button is down");
        let ended = select(&mut app, "B2:C3");
        assert_eq!(watch.note(selection_note(&app), true), None);
        let told = watch.note(selection_note(&app), false);
        assert_eq!(told.as_ref(), Some(&ended), "the release tells where the drag ended");
        assert_ne!(told, Some(passed));
    }

    #[test]
    fn the_same_cells_in_another_workbook_are_told_again() {
        let app = app();
        let (doc, note) = selection_note(&app).unwrap();
        let mut watch = SelectionWatch::default();
        assert!(watch.note(Some((doc, note.clone())), false).is_some());
        assert!(watch.note(Some((doc + 1, note.clone())), false).is_some(), "a newly opened workbook starts over");
        assert!(watch.note(None, false).is_none(), "no workbook, nothing to tell");
        assert!(watch.note(Some((doc + 1, note)), false).is_some(), "and the next one is told from the start");
    }

    #[test]
    fn open_names_fall_back_to_the_default_when_the_host_sent_none() {
        assert_eq!(open_name("Budget.xlsx"), "Budget.xlsx");
        assert_eq!(open_name("Sheet2!Book.xlsx"), "Sheet2!Book.xlsx");
        assert_eq!(open_name("document"), DEFAULT_OPEN_NAME, "what the crate says for a missing or blank name");
    }
}
