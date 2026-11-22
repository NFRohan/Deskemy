//! Back through the pages, as a browser's back does: the mouse's back
//! button on the library and its pages (the player and the PDF viewer have
//! their own). Each page landed on is recorded, a course or a track by which
//! one; back returns to the one before. UI thread only.

use std::cell::RefCell;

/// A place in the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// A page of its own: "library", "search", "settings", ...
    Page(String),
    Course(String),
    Track(String),
}

/// How far back is remembered.
const KEEP: usize = 50;

#[derive(Default)]
pub struct History {
    /// Earlier places, the latest last.
    earlier: Vec<Place>,
    here: Option<Place>,
    /// Going back: the next place landed on is the one gone back to, not a
    /// new visit.
    returning: bool,
}

impl History {
    /// A place was landed on.
    pub fn visit(&mut self, place: Place) {
        if std::mem::take(&mut self.returning) {
            self.here = Some(place);
            return;
        }
        if self.here.as_ref() == Some(&place) {
            return;
        }
        if let Some(here) = self.here.replace(place) {
            self.earlier.push(here);
            if self.earlier.len() > KEEP {
                self.earlier.remove(0);
            }
        }
    }

    /// Where back goes: the latest earlier place that's still there
    /// (`exists`), dropping those that aren't. None: nowhere to go back to.
    pub fn back(&mut self, exists: impl Fn(&Place) -> bool) -> Option<Place> {
        while let Some(place) = self.earlier.pop() {
            if exists(&place) {
                self.returning = true;
                return Some(place);
            }
        }
        None
    }
}

thread_local! {
    static HISTORY: RefCell<History> = RefCell::default();
}

/// A place was landed on (see `History::visit`).
pub fn visit(place: Place) {
    HISTORY.with(|h| h.borrow_mut().visit(place));
}

/// Where back goes (see `History::back`).
pub fn back(exists: impl Fn(&Place) -> bool) -> Option<Place> {
    HISTORY.with(|h| h.borrow_mut().back(exists))
}

/// A page's title, as the sidebar names it (its breadcrumb).
pub fn page_title(page: &str) -> &'static str {
    match page {
        "favorites" => "Favorites",
        "history" => "History",
        "bookmarks" => "Bookmarks",
        "search" => "Search",
        "tracks" => "Career Tracks",
        "stats" => "Stats",
        "settings" => "Settings",
        _ => "Library",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(name: &str) -> Place {
        Place::Page(name.into())
    }

    #[test]
    fn back_returns_where_you_were() {
        let mut h = History::default();
        h.visit(page("library"));
        h.visit(page("search"));
        h.visit(Place::Course("a".into()));
        h.visit(Place::Course("a".into())); // shown again: not a new visit

        assert_eq!(h.back(|_| true), Some(page("search")));
        h.visit(page("search")); // landing there isn't a visit
        assert_eq!(h.back(|_| true), Some(page("library")));
        h.visit(page("library"));
        assert_eq!(h.back(|_| true), None, "nowhere further");
    }

    #[test]
    fn a_course_since_removed_is_skipped() {
        let mut h = History::default();
        h.visit(page("library"));
        h.visit(Place::Course("gone".into()));
        h.visit(page("stats"));
        let back = h.back(|p| *p != Place::Course("gone".into()));
        assert_eq!(back, Some(page("library")));
    }

    #[test]
    fn only_the_latest_places_are_kept() {
        let mut h = History::default();
        for i in 0..KEEP + 10 {
            h.visit(Place::Course(i.to_string()));
        }
        let mut n = 0;
        while h.back(|_| true).is_some() {
            h.visit(page("x"));
            n += 1;
        }
        assert_eq!(n, KEEP);
    }
}
