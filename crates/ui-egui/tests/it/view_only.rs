//! A host can show workbooks for looking through only (`Session::view_only`): the window draws no
//! title bar, ribbon or File view, starts no edit, and lists in its menus only what still runs.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::core::Value;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A window on a small table; `view_only` is what a host sets before the first frame.
fn harness(view_only: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Row", "Task"], [1, "beta"], [2, "alpha"]]})).unwrap();
    session.view_only = view_only;
    crate::support::harness(session, 4)
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn value(h: &Harness, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap()))).unwrap()
}

fn right_click(h: &mut Harness, cell: &str) {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let pos = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}

#[test]
fn a_view_only_window_draws_no_title_bar_ribbon_or_new_sheet_button() {
    let full = harness(false);
    let h = harness(true);
    for label in ["AutoSave", "File", "Paste", "New sheet (⇧F11)"] {
        assert!(full.query_by_label(label).is_some(), "an ordinary window draws {label}");
        assert!(h.query_by_label(label).is_none(), "a view-only window does not draw {label}");
    }
    let (ordinary, viewing) = (full.state().grid.rect.unwrap(), h.state().grid.rect.unwrap());
    assert!(viewing.min.y < ordinary.min.y, "the sheet moves up into the room the title bar and ribbon took");
}

#[test]
fn typing_deleting_and_formatting_change_nothing_in_a_view_only_window() {
    let mut h = harness(true);
    let before = h.state().session.active().unwrap().wb.clone();
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "typing starts no edit");
    key(&mut h, Key::F2, Modifiers::NONE);
    assert!(h.state().editor.is_none(), "neither does F2");
    key(&mut h, Key::Delete, Modifiers::NONE);
    key(&mut h, Key::B, Modifiers::COMMAND);
    assert_eq!(value(&h, "A1"), Value::Text("Row".into()));
    assert!(std::sync::Arc::ptr_eq(&h.state().session.active().unwrap().wb, &before), "the workbook is as it was opened");
    assert!(h.state().message.is_none(), "a refusal is a passing notice, not a box to dismiss");
}

#[test]
fn a_view_only_window_opens_find_without_replace_and_no_editing_dialog() {
    let mut h = harness(true);
    h.state_mut().open_dialog("formatCells", json!({}));
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "Format Cells does not open");
    h.state_mut().open_dialog("saveAs", json!({}));
    h.state_mut().open_backstage(gridcraft_ui_egui::backstage::Page::Home);
    h.run_steps(2);
    assert!(h.state().dialog.is_none() && h.state().backstage.is_none(), "nor Save As or the File view");
    // Ctrl+H asks for the Replace tab; a view-only window shows Find.
    h.state_mut().open_dialog("find", json!({"replace": true}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| (d.name.as_str(), d.tab.as_str())), Some(("find", "Find")));
    assert!(h.query_by_label("Replace All").is_none(), "there is nothing to replace with");
}

#[test]
fn the_cell_menu_of_a_view_only_window_lists_only_what_still_runs() {
    let mut full = harness(false);
    right_click(&mut full, "B2");
    assert!(full.query_by_label("Format Cells…").is_some(), "an ordinary window's menu offers Format Cells");

    let mut h = harness(true);
    right_click(&mut h, "B2");
    for kept in ["Copy", "Sort A to Z", "Filter by Selected Cell's Value"] {
        assert!(h.query_by_label(kept).is_some(), "the menu keeps {kept}");
    }
    for gone in ["Cut", "Paste", "Insert…", "Delete…", "Clear Contents", "Format Cells…", "New Comment"] {
        assert!(h.query_by_label(gone).is_none(), "the menu leaves out {gone}");
    }
}

#[test]
fn sorting_from_the_cell_menu_still_works_in_a_view_only_window() {
    let mut h = harness(true);
    right_click(&mut h, "B2");
    h.get_by_label("Sort A to Z").click();
    h.run_steps(3);
    assert_eq!(value(&h, "B2"), Value::Text("alpha".into()), "the rows are sorted by the clicked column");
    assert_eq!(value(&h, "A2"), Value::Number(2.0), "and each row keeps its cells together");
}

/// The same table in an entry-only window (`Session::entry_only`).
fn entry_harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Row", "Task"], [1, "beta"], [2, "alpha"]]})).unwrap();
    session.entry_only = true;
    crate::support::harness(session, 4)
}

#[test]
fn an_entry_only_window_hides_the_chrome_but_takes_typing() {
    let mut h = entry_harness();
    for label in ["AutoSave", "File", "Paste", "New sheet (⇧F11)"] {
        assert!(h.query_by_label(label).is_none(), "an entry-only window does not draw {label}");
    }
    h.state_mut().session.execute("selection.set", json!({"range": "B2"})).unwrap();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("gamma".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some(), "typing starts an edit");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::Text("gamma".into()), "and Enter enters it");
    key(&mut h, Key::B, Modifiers::COMMAND);
    let bold = h.state_mut().session.run("cell.get", json!({"cell": "B3"})).unwrap()["style"]["font"]["bold"].clone();
    assert_ne!(bold, json!(true), "a format shortcut is refused");
}

#[test]
fn the_cell_menu_of_an_entry_only_window_lists_entering_but_not_structure() {
    let mut h = entry_harness();
    right_click(&mut h, "B2");
    for kept in ["Copy", "Paste", "Clear Contents", "Sort A to Z", "Pick From Drop-down List…"] {
        assert!(h.query_by_label(kept).is_some(), "the menu keeps {kept}");
    }
    for gone in ["Cut", "Insert…", "Delete…", "Format Cells…", "New Comment", "Define Name…"] {
        assert!(h.query_by_label(gone).is_none(), "the menu leaves out {gone}");
    }
}
