//! View-only sessions: a host shows workbooks that people look through but cannot edit.
//!
//! A host sets [`Session::view_only`](crate::Session::view_only); no command does, so nothing a
//! person, a script or a control channel runs can turn it off. While it is on, `execute` refuses
//! every top-level command except queries and the commands in [`ALLOWED`]. Those look around (the
//! selection, sheets, Go To, Find, zoom, copying, following a link), arrange what is shown
//! (sorting, filtering, column widths, frozen panes), undo that, and open a workbook. Saving and
//! exporting are refused as well: the host owns the file. A command an allowed one runs inside
//! itself is not checked again.

use crate::cmd::CommandSpec;

/// Why a command is refused in a view-only session.
pub const REFUSED: &str = "This workbook is view-only.";

/// Commands a view-only session runs, besides queries.
pub const ALLOWED: &[&str] = &[
    "selection.set",
    "selection.move",
    "selection.next",
    "selection.nextCorner",
    "selection.row",
    "selection.column",
    "selection.currentRegion",
    "edit.selectAll",
    "edit.goTo",
    "edit.goToSpecial",
    "edit.find",
    "edit.copy",
    "edit.clearClipboard",
    "edit.undo",
    "edit.redo",
    "sheet.activate",
    "sheet.next",
    "sheet.previous",
    "window.activate",
    "link.open",
    "view.zoom",
    "view.zoom100",
    "view.zoomToSelection",
    "view.freezePanes",
    "view.freezeTopRow",
    "view.freezeFirstColumn",
    "view.unfreezePanes",
    "data.sortAscending",
    "data.sortDescending",
    "data.sort",
    "data.filter",
    "data.filterBy",
    "data.clearFilter",
    "data.reapply",
    "home.columnWidth",
    "home.autofitColumnWidth",
    "file.open",
];

/// Why a command is refused in an entry-only session.
pub const ENTRY_REFUSED: &str = "Only cell values can be entered in this workbook.";

/// Commands an entry-only session runs besides [`ALLOWED`] and queries: they fill in cells.
/// Pasting pastes values there. `data.validation` sets a cell's list for a host page: nothing in
/// an entry-only window opens its dialog.
pub const ENTRY: &[&str] = &[
    "cell.set",
    "cell.toggleCheckbox",
    "range.setValues",
    "edit.beginEdit",
    "edit.paste",
    "edit.clearContents",
    "edit.fillDown",
    "edit.fillRight",
    "edit.fillUp",
    "edit.fillLeft",
    "edit.autoFill",
    "data.validation",
];

/// Whether a view-only session runs `spec`: a query, or one of [`ALLOWED`].
pub fn allows_spec(spec: &CommandSpec) -> bool {
    (!spec.journal && !spec.undoable) || ALLOWED.contains(&spec.id)
}

/// Whether an entry-only session runs `spec`: what a view-only one runs, or one of [`ENTRY`].
pub fn allows_entry_spec(spec: &CommandSpec) -> bool {
    allows_spec(spec) || ENTRY.contains(&spec.id)
}

/// Whether an entry-only session runs the command `id`. An unknown id is not allowed.
pub fn allows_entry(id: &str) -> bool {
    crate::cmd::find_command(id).is_some_and(allows_entry_spec)
}

/// Whether a view-only session runs the command `id`. An unknown id is not allowed.
pub fn allows(id: &str) -> bool {
    crate::cmd::find_command(id).is_some_and(allows_spec)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::Session;

    /// A workbook with a header row and three rows of data, then switched to view-only.
    fn viewing() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Row", "Task"], [1, "beta"], [2, "alpha"], [3, "gamma"]]})).unwrap();
        s.view_only = true;
        s
    }

    fn column(s: &mut Session, range: &str) -> serde_json::Value {
        s.execute("sheet.read", json!({"range": range})).unwrap()
    }

    #[test]
    fn a_view_only_session_refuses_edits_and_saving() {
        let mut s = viewing();
        let before = s.doc().unwrap().wb.clone();
        let refused = [
            ("cell.set", json!({"cell": "B2", "input": "changed"})),
            ("range.setValues", json!({"range": "B2", "values": [["changed"]]})),
            ("edit.clearContents", json!({"range": "A1:B4"})),
            ("edit.paste", json!({})),
            ("edit.cut", json!({})),
            ("edit.replace", json!({"what": "beta", "with": "x", "all": true})),
            ("edit.fillDown", json!({})),
            ("home.bold", json!({})),
            ("home.insertRows", json!({})),
            ("home.deleteColumns", json!({})),
            ("home.insertSheet", json!({"name": "Two"})),
            ("home.rowHeight", json!({"rows": "2:2", "height": 40})),
            ("sheet.rename", json!({"name": "Renamed"})),
            ("review.protectSheet", json!({})),
            ("file.save", json!({"path": "book.xlsx"})),
            ("file.saveAs", json!({"path": "book.xlsx"})),
            ("file.exportCsv", json!({"path": "book.csv"})),
            ("file.new", json!({})),
        ];
        for (id, params) in refused {
            let err = s.execute(id, params).expect_err(id).to_string();
            assert!(err.contains(REFUSED), "{id} says why it is refused: {err}");
        }
        assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &before), "nothing changed the workbook");
    }

    #[test]
    fn a_view_only_session_still_sorts_filters_and_sizes_columns() {
        let mut s = viewing();
        s.execute("selection.set", json!({"range": "B2"})).unwrap();
        let unsorted = column(&mut s, "B2:B4");
        s.execute("data.sortAscending", json!({"column": "B"})).unwrap();
        let sorted = column(&mut s, "B2:B4");
        assert_ne!(sorted, unsorted, "sorting reorders the rows");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(column(&mut s, "B2:B4"), unsorted, "and Undo puts them back");

        s.execute("data.filter", json!({"range": "A1:B4"})).unwrap();
        s.execute("data.filterBy", json!({"column": "B", "values": ["alpha"]})).unwrap();
        let hidden = |s: &Session, row: u32| s.doc().unwrap().wb.active().unwrap().is_row_hidden(row);
        assert!(hidden(&s, 1) && !hidden(&s, 2), "the filter hides the rows that do not match");
        s.execute("data.clearFilter", json!({})).unwrap();
        assert!(!hidden(&s, 1), "clearing the filter shows them again");

        let width = |s: &Session| s.doc().unwrap().wb.active().unwrap().col_width(1);
        let narrow = width(&s);
        s.execute("home.columnWidth", json!({"cols": "B:B", "width": narrow + 80.0})).unwrap();
        assert!(width(&s) > narrow, "a column can be made wider");

        s.execute("view.freezeTopRow", json!({})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("edit.find", json!({"what": "gamma"})).unwrap();
        s.execute("selection.row", json!({})).unwrap();
        s.execute("document.inspect", json!({})).unwrap();
    }

    #[test]
    fn a_view_only_session_opens_the_workbook_a_host_hands_over() {
        let mut s = viewing();
        let bytes = s.execute("file.saveBytes", json!({})).unwrap();
        let opened = s.execute("file.open", json!({"name": "Plan.xlsx", "base64": bytes["base64"]}));
        assert!(opened.is_ok(), "{opened:?}");
        assert_eq!(s.documents().len(), 2, "the workbook opens beside the first");
        assert!(s.execute("cell.set", json!({"cell": "A1", "input": "x"})).is_err(), "and is view-only too");
    }

    #[test]
    fn the_command_list_marks_what_a_view_only_session_refuses() {
        let s = viewing();
        let info = |id: &str| s.commands().into_iter().find(|c| c.id == id).unwrap();
        let set = info("cell.set");
        assert!(!set.enabled);
        assert_eq!(set.disabled_reason.as_deref(), Some(REFUSED));
        assert!(info("data.sortAscending").enabled, "an allowed command stays enabled");
        assert!(info("sheet.read").enabled, "and so does a query");
    }

    /// The table of `viewing`, entry-only instead.
    fn entering() -> Session {
        let mut s = viewing();
        s.view_only = false;
        s.entry_only = true;
        s
    }

    #[test]
    fn an_entry_only_session_fills_in_cells_but_keeps_the_structure_and_formats() {
        let mut s = entering();
        s.execute("cell.set", json!({"cell": "B2", "input": "typed"})).unwrap();
        s.execute("range.setValues", json!({"range": "B3", "values": [["from the host"]]})).unwrap();
        s.execute("edit.clearContents", json!({"range": "B4"})).unwrap();
        let read = column(&mut s, "B2:B4");
        assert_eq!(read["values"], json!([["typed"], ["from the host"], [null]]), "{read}");
        s.execute("edit.undo", json!({})).unwrap();
        let before = s.doc().unwrap().wb.clone();
        for (id, params) in [
            ("home.insertRows", json!({})),
            ("home.deleteColumns", json!({})),
            ("home.insertSheet", json!({"name": "Two"})),
            ("sheet.rename", json!({"name": "Renamed"})),
            ("home.bold", json!({"range": "A1"})),
            ("home.rowHeight", json!({"rows": "2:2", "height": 40})),
            ("review.unprotectSheet", json!({})),
            ("file.save", json!({"path": "book.xlsx"})),
            ("file.exportCsv", json!({"path": "book.csv"})),
            ("edit.cut", json!({})),
            ("edit.replace", json!({"what": "beta", "with": "x", "all": true})),
        ] {
            let err = s.execute(id, params).expect_err(id).to_string();
            assert!(err.contains(ENTRY_REFUSED), "{id} says why it is refused: {err}");
        }
        assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &before), "nothing changed the workbook");
    }

    #[test]
    fn an_entry_only_session_pastes_values_without_the_copys_format() {
        let mut s = entering();
        s.entry_only = false;
        s.execute("home.bold", json!({"range": "B2"})).unwrap();
        s.entry_only = true;
        s.execute("selection.set", json!({"range": "B2"})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("selection.set", json!({"range": "C2"})).unwrap();
        s.execute("edit.paste", json!({})).unwrap();
        let cell = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
        assert_eq!(cell(&mut s, "C2")["value"], cell(&mut s, "B2")["value"], "the value comes over");
        assert_ne!(cell(&mut s, "C2")["style"]["font"]["bold"], json!(true), "the bold does not");
    }

    #[test]
    fn every_allowed_command_is_a_registered_command() {
        for id in ENTRY {
            assert!(crate::cmd::find_command(id).is_some(), "{id} is not a command");
        }
        for id in ALLOWED {
            assert!(crate::cmd::find_command(id).is_some(), "{id} is not a command");
        }
        assert!(!allows("no.suchCommand"));
    }
}
