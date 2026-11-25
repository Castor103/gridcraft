//! Host modes: a page that runs GridCraft (as an iframe, or in its own document) opens workbooks
//! in it and stores what it saves instead of the browser downloading. The messages and the two
//! transports are `craft-web-host`'s (storytold/craft-libs), the same contract as the other Craft
//! apps' bridges, with the `gridcraft:` prefix. This module is GridCraft's part of it.
//!
//! - `?host=parent` (an `<iframe>`, such as a Nextcloud app) talks to the same-origin parent over
//!   `postMessage`; `data-host="page"` on the canvas (the page's own document, no iframe) uses
//!   DOM `CustomEvent`s on the canvas instead. `?host=parent` wins when both are present. With no
//!   parent, or a parent on another origin, there is no bridge: the normal app with downloads.
//! - `gridcraft:ready` once the app is listening; send a file now. GridCraft reads bytes, so it
//!   says `file: false` and a host sends `bytes`.
//! - `gridcraft:open` (from the host): `{ name, bytes }`. It opens like a workbook picked with
//!   File › Open or dropped on the window; a file over [`MAX_OPEN_BYTES`] is refused before it is
//!   copied.
//! - `gridcraft:save` (to the host): `{ name, saveAs, bytes }` for every file GridCraft would
//!   otherwise download: Save and Save As (an .xlsx workbook) and Export as PDF. The host stores
//!   it and reports failures itself. GridCraft doesn't tell the host which saves make a file the
//!   document, so `saveAs` is always `false`.
//! - `gridcraft:dirty` whenever unsaved changes appear or go away, `gridcraft:failed` when the
//!   app couldn't start.
//! - `gridcraft:selection` (to the host): `{ sheet, selection, activeCell, rowKey }` whenever the
//!   selection changes ([`crate::hostmode::SelectionNote`]); a drag tells where it ended. This one
//!   is GridCraft's own, not part of the shared contract.
//!
//! With `?mode=view` the window is view-only (`Session::view_only`): nothing is saved, so the
//! host gets no `gridcraft:save` and no `gridcraft:dirty` (sorting and filtering change the
//! workbook in memory, which is not something to save).
//!
//! In both host modes the page starts with an empty workbook instead of a sample, saves and PDF
//! exports are handed to the page rather than downloaded, and `?author=` names the user for new
//! notes and comments.

use std::cell::RefCell;
use std::rc::Rc;

use craft_web_host::web::{FileData, Host, Message};
use craft_web_host::{Mode, message_type};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsValue;

use crate::hostmode::{ChangeNote, SelectionNote, SelectionWatch, change_notes, open_name, selection_note};

/// The app id in the host messages (`gridcraft:ready`).
const APP: &str = "gridcraft";

/// Saved files that count as a workbook: the document is then clean under the name it was saved as.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The largest workbook taken from the host page, refused before it is copied.
const MAX_OPEN_BYTES: u64 = 512 << 20;

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

#[derive(Clone)]
pub struct Bridge {
    host: Rc<Host>,
    /// Names of the files handed to the host since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// Where `gridcraft:selection` goes, and what the host last heard.
    notes: Rc<NoteTarget>,
    selection: Rc<RefCell<SelectionWatch>>,
}

impl Bridge {
    /// The bridge for this page, or `None` when GridCraft isn't hosted: no `?host=parent` and no
    /// `data-host="page"` on the canvas, an unframed `?host=parent` page (it stays the normal app
    /// with downloads), or a parent on another origin.
    pub fn detect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
        let host = Host::connect(APP, canvas)?;
        let notes = match host.mode() {
            Mode::Parent => {
                // `Host::connect` admitted only a same-origin parent. That origin is the
                // `postMessage` target, so a parent that navigated away hears nothing.
                let window = web_sys::window()?;
                NoteTarget::Parent { parent: window.parent().ok().flatten()?, origin: window.location().origin().ok()? }
            }
            Mode::Page => NoteTarget::Page(canvas.clone()),
        };
        Some(Self { host: Rc::new(host), saved: Rc::default(), notes: Rc::new(notes), selection: Rc::default() })
    }

    /// Send saved and exported files to the host instead of downloading them, and hear files
    /// from it.
    pub fn connect(&self, services: &mut Services, inbox: Inbox, ctx: egui::Context) {
        self.listen(inbox, ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes) {
                log::error!("couldn't hand {name} to the page: {e}");
            }
        }));
    }

    /// `?author=`: the host's name for the user signs new notes and comments.
    pub fn set_author(app: &mut SheetApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.prefs.user_name = author;
        }
    }

    pub fn post_ready(&self) {
        let _ = self.host.post_ready(env!("CARGO_PKG_VERSION"), false);
    }

    pub fn post_failed(&self, error: &str) {
        let _ = self.host.post_failed(error, None);
    }

    /// After each frame: a workbook handed to the host this frame counts as saved under the
    /// name it was saved as, and the host hears when unsaved changes appear or go away (the
    /// crate does not repeat a state it already reported).
    pub fn sync(&self, app: &mut SheetApp) {
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        if let Some(name) = saved.iter().rev().find(|name| is_workbook(name))
            && let Some(doc) = app.session.active_mut()
        {
            doc.saved = doc.wb.clone();
            if doc.path.is_none() {
                doc.title = name.clone();
            }
        }
        // A view-only window saves nothing, so it has no unsaved changes to report.
        if !app.view_only() {
            self.host.post_dirty(app.session.documents().iter().any(|d| d.is_dirty()));
        }
    }

    /// After each frame: the host hears what a person changed since the last frame, one
    /// `gridcraft:change` per command and sheet (`Session::changes`, which this turns on).
    pub fn sync_changes(&self, app: &mut SheetApp) {
        let changes = std::mem::take(app.session.changes.get_or_insert_with(Vec::new));
        for note in change_notes(app, changes) {
            if let Err(e) = self.notes.post_change(&note) {
                log::warn!("couldn't tell the page what changed: {e:?}");
            }
        }
    }

    /// After each frame: the host hears the selection when it changed (`gridcraft:selection`).
    /// `pointer_down` holds it back during a drag, which tells where it ended.
    pub fn sync_selection(&self, app: &SheetApp, pointer_down: bool) {
        // The watch is released before the send: a page host's listener runs inside it.
        let note = self.selection.borrow_mut().note(selection_note(app), pointer_down);
        if let Some(note) = note
            && let Err(e) = self.notes.post_selection(&note)
        {
            log::warn!("couldn't tell the page the selection: {e:?}");
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = file_name(path);
        self.host.post_save(&name, bytes, None, false)?;
        self.saved.borrow_mut().push(name);
        Ok(())
    }

    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        self.host.listen(move |message| {
            if let Message::Open { name, data } = message {
                deliver_open(&inbox, &ctx, &name, &data);
            }
        });
    }
}

/// Where GridCraft's own messages go: the same place and transport as `craft-web-host`'s, which
/// sends only the messages of the shared contract.
enum NoteTarget {
    Parent { parent: web_sys::Window, origin: String },
    Page(web_sys::HtmlCanvasElement),
}

impl NoteTarget {
    fn post_selection(&self, note: &SelectionNote) -> Result<(), JsValue> {
        let payload = js_sys::Object::new();
        let set = |key: &str, value: &str| js_sys::Reflect::set(&payload, &JsValue::from_str(key), &JsValue::from_str(value));
        set("sheet", &note.sheet)?;
        set("selection", &note.selection)?;
        set("activeCell", &note.active_cell)?;
        set("rowKey", &note.row_key)?;
        self.post("selection", payload)
    }

    fn post_change(&self, note: &ChangeNote) -> Result<(), JsValue> {
        let payload = js_sys::Object::new();
        let set = |key: &str, value: JsValue| js_sys::Reflect::set(&payload, &JsValue::from_str(key), &value);
        set("sheet", note.sheet.as_str().into())?;
        set("range", note.range.as_str().into())?;
        set("cells", note.cells.iter().map(|c| JsValue::from_str(c)).collect::<js_sys::Array>().into())?;
        set("truncated", note.truncated.into())?;
        let rows = js_sys::Array::new();
        for (row, key) in &note.rows {
            let r = js_sys::Object::new();
            js_sys::Reflect::set(&r, &"row".into(), &JsValue::from(*row))?;
            js_sys::Reflect::set(&r, &"rowKey".into(), &key.as_str().into())?;
            rows.push(&r);
        }
        set("rows", rows.into())?;
        // A change within one row also carries that row's key, as `gridcraft:selection` does.
        if let [(_, key)] = note.rows.as_slice() {
            set("rowKey", key.as_str().into())?;
        }
        self.post("change", payload)
    }

    /// Sends `payload` as the message `gridcraft:<kind>`.
    fn post(&self, kind: &str, payload: js_sys::Object) -> Result<(), JsValue> {
        let kind = message_type(APP, kind);
        let set = |key: &str, value: &str| js_sys::Reflect::set(&payload, &JsValue::from_str(key), &JsValue::from_str(value));
        match self {
            Self::Parent { parent, origin } => {
                set("type", &kind)?;
                parent.post_message(&payload, origin)
            }
            Self::Page(canvas) => {
                let init = web_sys::CustomEventInit::new();
                init.set_detail(&payload.into());
                let event = web_sys::CustomEvent::new_with_event_init_dict(&kind, &init)?;
                canvas.dispatch_event(&event).map(|_| ())
            }
        }
    }
}

/// The file an open message carries, pushed onto the inbox for the next frame, like a dropped
/// file. A host's file is untrusted input: an oversized one is refused before it is copied into
/// the app's memory (the data's own length, not the host's claim).
fn deliver_open(inbox: &Inbox, ctx: &egui::Context, name: &str, data: &FileData) {
    let FileData::Bytes(array) = data else {
        log::warn!("host file {name} came as a File; GridCraft reads bytes only; not opened");
        return;
    };
    if data.size() > MAX_OPEN_BYTES as f64 {
        log::warn!("host file is {} bytes, over the {} MiB host open limit; not opened", data.size(), MAX_OPEN_BYTES >> 20);
        return;
    }
    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((open_name(name), array.to_vec()));
    ctx.request_repaint();
}

fn is_workbook(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| WORKBOOK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}
