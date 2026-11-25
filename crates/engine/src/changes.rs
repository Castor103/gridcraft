//! The cells a person changed: a host that keeps its own copy of the data hears which cells to
//! read again after each command (`Session::changes`).
//!
//! Only what a cell holds counts: its constant, formula or Python code. A recalculated result, a
//! format and a column width are not changes. Commands a host or an agent sends
//! (`Session::from_host`) are not reported either: the host knows what it wrote.

use gridcraft_core::CellRef;
use gridcraft_model::{Cell, Workbook};

/// One command's changes on one sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The workbook's id ([`crate::DocState::uid`]).
    pub doc: u64,
    /// The sheet's name.
    pub sheet: String,
    /// The sheet's index, for reading the cells.
    pub sheet_index: usize,
    /// The changed cells, in row order.
    pub cells: Vec<CellRef>,
}

/// Whether two cells hold the same thing: the same formula or Python code, or for a constant the
/// same value. A formula's result is left out: recalculation changes it, not the person.
pub fn same_input(a: &Cell, b: &Cell) -> bool {
    a.formula == b.formula && a.py == b.py && (a.formula.is_some() || a.py.is_some() || a.value == b.value)
}

/// The cells whose input differs from `before` to `after`, per sheet of `after`. Sheets are
/// matched by identity, so a renamed sheet is still compared with itself; a sheet that is new or
/// unchanged (still shared with `before`) gives nothing.
pub fn input_changes(doc: u64, before: &Workbook, after: &Workbook) -> Vec<Change> {
    let mut out = Vec::new();
    for (i, sheet) in after.sheets.iter().enumerate() {
        let Some(old) = before.sheets.iter().find(|s| s.uid.get() == sheet.uid.get()) else { continue };
        if std::sync::Arc::ptr_eq(old, sheet) {
            continue;
        }
        let cells = sheet.cells.changed_from(&old.cells, same_input);
        if !cells.is_empty() {
            out.push(Change { doc, sheet: sheet.name.clone(), sheet_index: i, cells });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    /// A session that records changes, on a table of three rows.
    fn recording() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Row", "Task", "Days"], [1, "beta", 3], [2, "alpha", 5]]})).unwrap();
        s.execute("cell.set", json!({"cell": "D2", "input": "=C2*2"})).unwrap();
        s.changes = Some(Vec::new());
        s
    }

    fn taken(s: &mut Session) -> Vec<(String, Vec<String>)> {
        s.changes.replace(Vec::new()).unwrap_or_default().into_iter().map(|c| (c.sheet, c.cells.iter().map(|c| c.a1()).collect())).collect()
    }

    #[test]
    fn a_persons_edit_reports_the_cells_it_changed_not_the_recalculated_ones() {
        let mut s = recording();
        let sheet = s.doc().unwrap().wb.active().unwrap().name.clone();
        s.execute("cell.set", json!({"cell": "C2", "input": "4"})).unwrap();
        assert_eq!(taken(&mut s), vec![(sheet.clone(), vec!["C2".to_string()])], "D2 recalculated, but nobody typed in it");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(taken(&mut s), vec![(sheet.clone(), vec!["C2".to_string()])], "Undo changes the cell back");
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(taken(&mut s).len(), 1, "and so does Redo");
        s.execute("home.bold", json!({"range": "A1:C1"})).unwrap();
        s.execute("selection.set", json!({"range": "B3"})).unwrap();
        assert!(taken(&mut s).is_empty(), "a format and a selection change no cell's content");
    }

    #[test]
    fn a_hosts_own_commands_are_not_reported() {
        let mut s = recording();
        s.from_host = true;
        s.execute("range.setValues", json!({"range": "B2", "values": [["from the host"]]})).unwrap();
        s.from_host = false;
        assert!(taken(&mut s).is_empty(), "the host knows what it wrote");
    }

    #[test]
    fn nothing_is_recorded_unless_a_host_asks() {
        let mut s = recording();
        s.changes = None;
        s.execute("cell.set", json!({"cell": "B2", "input": "x"})).unwrap();
        assert!(s.changes.is_none());
    }
}
