//! Pure host-mode rules, shared by the bridge ([`crate::host`]) and testable without a browser
//! (like [`crate::prefs`]): the name the bridge fills in when a host's open request has none,
//! and whether the page asked for a view-only window. Which pages host the app is
//! `craft-web-host`'s.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mode_view_asks_for_a_view_only_window() {
        assert!(view_mode(Some("view")));
        for other in [None, Some(""), Some("edit"), Some("View"), Some("viewer")] {
            assert!(!view_mode(other), "{other:?} is the ordinary editor");
        }
    }

    #[test]
    fn open_names_fall_back_to_the_default_when_the_host_sent_none() {
        assert_eq!(open_name("Budget.xlsx"), "Budget.xlsx");
        assert_eq!(open_name("Sheet2!Book.xlsx"), "Sheet2!Book.xlsx");
        assert_eq!(open_name("document"), DEFAULT_OPEN_NAME, "what the crate says for a missing or blank name");
    }
}
