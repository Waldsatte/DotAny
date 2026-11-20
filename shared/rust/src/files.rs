//! The open files, a row per window (on the Mac a program has one process and a window per file). On
//! Windows every file is a program of its own, so the table has one row, its window's; the same code runs.
//!
//! A row is made with its window (`open_window_at`) and goes with it (`WindowEvent::Destroyed`). It holds what the family
//! knows of the window's file (where it is, what it was when last read or written, its watcher) and, for the Mac, how a
//! file handed over by the system reaches the window: whether its page can take one yet, and whether one is already on
//! its way, so a second file never goes to a window that is about to open the first.
//!
//! Only data and rules here, no Tauri: `lib.rs` does the windows, and the rules are tested below without any.

use crate::Meta;
use crate::watcher::FileWatch;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The label of a program's first window, and of its only one where a program has one window at a time (every tool that
/// hasn't moved to a file per window; every program on Windows).
pub const MAIN: &str = "main";

#[derive(Default)]
pub(crate) struct Row {
    pub path: Option<PathBuf>,
    pub baseline: Option<Meta>,
    pub announced: Option<Meta>,
    pub watch: Option<FileWatch>,
    /// The order windows were made in: a new window is cascaded from the newest when none is in front.
    pub born: u64,
    /// The page has said it can take a file (`window_ready`).
    pub ready: bool,
    /// A file handed to the window before its page could take it: given when it is ready.
    pub waiting: Option<PathBuf>,
    /// A file is on its way to this window (handed over, or the window was made with one) and hasn't arrived: no other
    /// file goes to it. Over when the window opens a file or closes the one it has.
    pub claimed: bool,
    /// The window holds work that has no file (a picture pasted in): it takes no file handed over, which would replace
    /// that work; a new window does.
    pub untitled: bool,
    /// The page has work not saved yet (it says so on the Mac, for the dot in the red button: `set_edited`): Quit asks
    /// these windows first.
    pub edited: bool,
    /// The Mac's Quit asked the window to close before its page could hear it (a window that has only just opened): the
    /// page is asked again once it is ready (`ready_to_close`), or the quit would wait for it for ever.
    pub close_held: bool,
    /// What the Mac's menu bar or the system asked of the window's page before the page could hear it (Settings
    /// chosen with no window open, a new document asked for by DotAny): event and what goes with it, said once it is
    /// ready.
    pub told: Vec<(String, serde_json::Value)>,
}

#[derive(Default)]
pub(crate) struct Table {
    rows: HashMap<String, Row>,
    born: u64,
    /// Windows made by the base for a tool with a file per window, to name the next one.
    made: u64,
}

impl Table {
    /// A window is being made: its row, the newest. A window made again where it was (its WebView went) starts afresh.
    pub fn add(&mut self, label: &str, claimed: bool) {
        self.born += 1;
        self.rows.insert(label.to_string(), Row { born: self.born, claimed, ..Row::default() });
    }

    /// The window has gone: its row, to drop outside the lock (its watcher stops as it drops).
    pub fn remove(&mut self, label: &str) -> Option<Row> {
        self.rows.remove(label)
    }

    pub fn row(&mut self, label: &str) -> Option<&mut Row> {
        self.rows.get_mut(label)
    }

    pub fn path(&self, label: &str) -> Option<PathBuf> {
        self.rows.get(label).and_then(|r| r.path.clone())
    }

    /// The name of the next window a tool with a file per window opens: its first is `main`, as every program's is, and
    /// the rest `main-2`, `main-3`… never used twice in one run.
    pub fn next_label(&mut self) -> String {
        self.made += 1;
        if self.made == 1 { MAIN.to_string() } else { format!("{MAIN}-{}", self.made) }
    }

    /// The window opened `path`: it is the row's file now, watched from here. Gives back the watcher that is no longer
    /// wanted (the old file's, or this one's when the window has gone), to drop outside the lock: a watcher stopping
    /// waits for its callback, which takes the lock.
    #[must_use]
    pub fn opened(
        &mut self,
        label: &str,
        path: &Path,
        baseline: Option<Meta>,
        watch: Option<FileWatch>,
    ) -> Option<FileWatch> {
        let Some(row) = self.rows.get_mut(label) else { return watch };
        row.path = Some(path.to_path_buf());
        row.baseline = baseline;
        row.announced = None;
        row.claimed = false;
        row.untitled = false;
        std::mem::replace(&mut row.watch, watch)
    }

    /// The window closed its file: nothing to watch or step from, and it can take another. With `untitled`, it holds
    /// work that has no file instead (a picture pasted in). The window itself stays as it was. Gives back the watcher, to
    /// drop outside the lock.
    #[must_use]
    pub fn closed(&mut self, label: &str, untitled: bool) -> Option<FileWatch> {
        let row = self.rows.get_mut(label)?;
        row.path = None;
        row.baseline = None;
        row.announced = None;
        row.claimed = false;
        row.untitled = untitled;
        row.watch.take()
    }

    /// Every window's file, to look for one already open (outside the lock: comparing names asks the disk).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn files(&self) -> Vec<(String, PathBuf)> {
        self.rows.iter().filter_map(|(l, r)| r.path.clone().map(|p| (l.clone(), p))).collect()
    }

    /// A window that can take a file: nothing open, nothing on its way. The newest, if there are several.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn free(&self) -> Option<String> {
        self.rows
            .iter()
            .filter(|(_, r)| r.path.is_none() && !r.claimed && !r.untitled)
            .max_by_key(|(_, r)| r.born)
            .map(|(l, _)| l.clone())
    }

    /// Whether this window can take a file handed over: nothing open, nothing on its way, no work without a file.
    pub fn can_take(&self, label: &str) -> bool {
        self.rows.get(label).is_some_and(|r| r.path.is_none() && !r.claimed && !r.untitled)
    }

    /// `path` goes to this window: back for the page now if it can take it, else kept until it can (`ready`).
    pub fn hand(&mut self, label: &str, path: PathBuf) -> Option<PathBuf> {
        let row = self.rows.get_mut(label)?;
        row.claimed = true;
        if row.ready {
            Some(path)
        } else {
            row.waiting = Some(path);
            None
        }
    }

    /// The window's page can take a file now: the one handed to it meanwhile, if any.
    pub fn ready(&mut self, label: &str) -> Option<PathBuf> {
        let row = self.rows.get_mut(label)?;
        row.ready = true;
        row.waiting.take()
    }

    /// The window is asked to close: true when its page can't hear that yet, which is kept until it can.
    pub fn hold_close(&mut self, label: &str) -> bool {
        let Some(row) = self.rows.get_mut(label) else { return false };
        if !row.ready {
            row.close_held = true;
        }
        !row.ready
    }

    /// Whether the window was asked to close before its page was ready (asked once, then forgotten).
    pub fn ready_to_close(&mut self, label: &str) -> bool {
        self.rows.get_mut(label).is_some_and(|r| std::mem::take(&mut r.close_held))
    }

    /// The quit was given up: no window still waiting for its page is asked to close once it is ready.
    pub fn forget_held(&mut self) {
        self.rows.values_mut().for_each(|r| r.close_held = false);
    }

    /// `event` for the window's page: back to say now if the page can hear it, else kept until it can (`told`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn tell(
        &mut self,
        label: &str,
        event: &str,
        payload: serde_json::Value,
    ) -> Option<(String, serde_json::Value)> {
        let row = self.rows.get_mut(label)?;
        if row.ready {
            return Some((event.to_string(), payload));
        }
        row.told.push((event.to_string(), payload));
        None
    }

    /// What was told the window's page before it was ready, in order, once (the page has just said it is).
    pub fn told(&mut self, label: &str) -> Vec<(String, serde_json::Value)> {
        self.rows.get_mut(label).map(|r| std::mem::take(&mut r.told)).unwrap_or_default()
    }

    /// The newest window, for a new one to cascade from when none is in front.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn newest(&self) -> Option<String> {
        self.rows.iter().max_by_key(|(_, r)| r.born).map(|(l, _)| l.clone())
    }

    /// Whether the window's page has said it has work not saved yet.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn edited(&self, label: &str) -> bool {
        self.rows.get(label).is_some_and(|r| r.edited)
    }

    /// When each window was made, for listing them in that order (the Mac's test driver).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn born(&self, label: &str) -> u64 {
        self.rows.get(label).map_or(u64::MAX, |r| r.born)
    }
}

/// A tool's own state, a value per window, as the base keeps its rows: what a tool with a file per window holds of its
/// file (Image's picture and what is drawn on it, Text's document). `of` makes a window's value the first time it is
/// asked for; `remove` takes it away when the window's file closes or the window goes (the tool's `FileWindows::forget`).
/// Each value is shared (`Arc`), so a command can hold it without holding the whole table.
pub struct PerWindow<T>(std::sync::Mutex<HashMap<String, std::sync::Arc<T>>>);

impl<T> Default for PerWindow<T> {
    fn default() -> Self {
        PerWindow(std::sync::Mutex::default())
    }
}

impl<T: Default> PerWindow<T> {
    /// The window's value, made now if it has none.
    pub fn of(&self, label: &str) -> std::sync::Arc<T> {
        crate::lock(&self.0).entry(label.to_string()).or_default().clone()
    }
}

impl<T> PerWindow<T> {
    /// The window's value, if it has one.
    pub fn get(&self, label: &str) -> Option<std::sync::Arc<T>> {
        crate::lock(&self.0).get(label).cloned()
    }

    /// Takes the window's value away, to stop what it runs (a picture still being read) as it goes.
    pub fn remove(&self, label: &str) -> Option<std::sync::Arc<T>> {
        crate::lock(&self.0).remove(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_asked_before_the_page_is_ready_is_asked_again_once_it_is() {
        let mut t = Table::default();
        t.add("main", false);
        t.add("main-2", true);
        t.ready("main");
        assert!(!t.hold_close("main"), "a ready page hears it now");
        assert!(!t.ready_to_close("main"));
        assert!(t.hold_close("main-2"), "this page can't hear it yet");
        assert_eq!(t.ready("main-2"), None);
        assert!(t.ready_to_close("main-2"), "asked again once ready");
        assert!(!t.ready_to_close("main-2"), "and only once");
        assert!(!t.hold_close("gone"));
    }

    #[test]
    fn a_quit_given_up_asks_no_window_to_close_later() {
        let mut t = Table::default();
        t.add("main", false);
        assert!(t.hold_close("main"));
        t.forget_held();
        let _ = t.ready("main");
        assert!(!t.ready_to_close("main"), "the quit was given up before the page was ready");
    }

    #[test]
    fn what_is_told_a_page_before_it_is_ready_waits_for_it_in_order() {
        let mut t = Table::default();
        t.add("main", false);
        assert_eq!(t.tell("main", "app-menu", "settings".into()), None);
        assert_eq!(t.tell("main", "new-file", "csv".into()), None);
        assert_eq!(t.tell("gone", "app-menu", "about".into()), None, "no window: nothing kept");
        let _ = t.ready("main");
        assert_eq!(
            t.told("main"),
            vec![("app-menu".to_string(), "settings".into()), ("new-file".to_string(), "csv".into())]
        );
        assert!(t.told("main").is_empty(), "told once");
        // a ready page hears it now
        assert_eq!(t.tell("main", "app-menu", "about".into()), Some(("app-menu".to_string(), "about".into())));
        assert!(t.told("main").is_empty());
    }

    #[test]
    fn a_tools_own_state_is_kept_a_value_per_window() {
        let state: PerWindow<std::sync::Mutex<Vec<u8>>> = PerWindow::default();
        crate::lock(&state.of("main")).push(1);
        crate::lock(&state.of("main-2")).push(2);
        assert_eq!(*crate::lock(&state.of("main")), vec![1]);
        assert_eq!(*crate::lock(&state.get("main-2").unwrap()), vec![2]);
        assert!(state.remove("main").is_some());
        assert!(state.get("main").is_none());
        assert!(crate::lock(&state.of("main")).is_empty(), "made afresh");
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn the_first_window_is_main_and_the_rest_are_never_named_twice() {
        let mut t = Table::default();
        assert_eq!(t.next_label(), "main");
        assert_eq!(t.next_label(), "main-2");
        assert_eq!(t.next_label(), "main-3");
    }

    #[test]
    fn each_window_keeps_its_own_file() {
        let mut t = Table::default();
        t.add("main", false);
        t.add("main-2", false);
        let _ = t.opened("main", &p("a.png"), Some((1, None)), None);
        let _ = t.opened("main-2", &p("b.png"), Some((2, None)), None);
        assert_eq!(t.path("main"), Some(p("a.png")));
        assert_eq!(t.path("main-2"), Some(p("b.png")));
        let _ = t.closed("main-2", false);
        assert_eq!(t.path("main-2"), None);
        assert_eq!(t.path("main"), Some(p("a.png")), "closing one window's file leaves the other's");
        assert!(t.remove("main").is_some());
        assert_eq!(t.path("main"), None);
        // an open that finishes after its window went makes no row
        let _ = t.opened("main", &p("c.png"), None, None);
        assert!(t.row("main").is_none());
    }

    #[test]
    fn a_file_goes_to_an_empty_window_once_its_page_can_take_it() {
        let mut t = Table::default();
        t.add("main", false);
        assert_eq!(t.free().as_deref(), Some("main"));
        assert!(t.can_take("main") && !t.can_take("main-2"));
        // the system hands a file over before the page is up: kept
        assert_eq!(t.hand("main", p("a.txt")), None);
        assert_eq!(t.free(), None, "a window a file is on its way to takes no other");
        assert!(!t.can_take("main"));
        assert_eq!(t.ready("main"), Some(p("a.txt")));
        assert_eq!(t.ready("main"), None, "handed over once");
        let _ = t.opened("main", &p("a.txt"), None, None);
        assert_eq!(t.free(), None);
        // its page closes the file: free again, and a file now goes straight to the page
        let _ = t.closed("main", false);
        assert_eq!(t.free().as_deref(), Some("main"));
        assert_eq!(t.hand("main", p("b.txt")), Some(p("b.txt")));
    }

    #[test]
    fn a_window_made_with_a_file_takes_no_other_and_the_newest_free_one_is_chosen() {
        let mut t = Table::default();
        t.add("main", false);
        t.add("main-2", true);
        t.add("main-3", false);
        assert_eq!(t.free().as_deref(), Some("main-3"));
        assert_eq!(t.newest().as_deref(), Some("main-3"));
        let _ = t.opened("main-3", &p("c.txt"), None, None);
        assert_eq!(t.free().as_deref(), Some("main"));
        let mut files = t.files();
        files.sort();
        assert_eq!(files, vec![("main-3".to_string(), p("c.txt"))]);
        assert!(t.born("main") < t.born("main-2") && t.born("main-2") < t.born("main-3"));
        // work with no file (a picture pasted in) isn't replaced by a file handed over
        let _ = t.closed("main", true);
        assert_eq!(t.free(), None);
        let _ = t.opened("main", &p("a.txt"), None, None);
        let _ = t.closed("main", false);
        assert_eq!(t.free().as_deref(), Some("main"));
    }

    #[test]
    fn a_window_made_again_starts_afresh() {
        let mut t = Table::default();
        t.add("main", false);
        let _ = t.ready("main");
        let _ = t.opened("main", &p("a.txt"), None, None);
        let _ = t.remove("main");
        t.add("main", false);
        assert_eq!(t.path("main"), None);
        assert!(!t.row("main").unwrap().ready);
    }
}
