//! Quit (⌘Q) on the Mac, as a document program does it: the windows close one at a time, front first, each the way its
//! red button closes it, so each asks about its own unsaved work with the window in front; the program ends once the last
//! one has gone. Cancel in any window's question stops the quit: that window and the ones not yet asked stay, and the
//! program stays open after them, as if Quit had never been chosen.
//!
//! The page doesn't say when its question was answered Cancel; the quit learns it the next time any window is asked to
//! close by anything but the quit itself (its red button, ⌘W). Only the rules here; `platform/macos.rs` does the
//! windows. (On Windows nothing uses this: a program there ends with its window.)
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[derive(Default)]
pub(crate) struct Quit {
    /// The window the quit has asked to close, and whether that request has reached it yet. None: no quit under way.
    asked: Option<(String, bool)>,
}

impl Quit {
    pub const fn new() -> Self {
        Quit { asked: None }
    }

    /// The quit asks `label` to close next.
    pub fn ask(&mut self, label: &str) {
        self.asked = Some((label.to_string(), false));
    }

    pub fn on(&self) -> bool {
        self.asked.is_some()
    }

    /// A window is asked to close. The quit's own request, the first time it arrives; anything else is the person closing
    /// a window themselves, so the quit was given up (Cancel in the window it had asked).
    pub fn close_requested(&mut self, label: &str) {
        match &mut self.asked {
            Some((asked, heard)) if asked == label && !*heard => *heard = true,
            Some(_) => self.asked = None,
            None => {}
        }
    }

    /// A window has gone: true when it was the one the quit asked, so the next one is asked now.
    pub fn gone(&self, label: &str) -> bool {
        self.asked.as_ref().is_some_and(|(asked, _)| asked == label)
    }

    /// The page of `label` says the question it asked as it was closing was answered Cancel. The quit stops there if
    /// that was the window it asked (true); a question in another window was nothing to do with the quit.
    pub fn given_up(&mut self, label: &str) -> bool {
        if self.asked.as_ref().is_some_and(|(asked, _)| asked == label) {
            self.asked = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_are_asked_one_after_another() {
        let mut q = Quit::default();
        assert!(!q.on());
        q.ask("main");
        q.close_requested("main");
        assert!(q.on());
        assert!(q.gone("main"), "the one asked has gone: ask the next");
        q.ask("main-2");
        q.close_requested("main-2");
        assert!(!q.gone("main"));
        assert!(q.gone("main-2"));
        // the last one gone: still quitting, so the program ends
        assert!(q.on());
    }

    #[test]
    fn closing_a_window_by_hand_gives_the_quit_up() {
        let mut q = Quit::default();
        q.ask("main");
        q.close_requested("main");
        // Cancel in main's question; later the person closes main with its red button
        q.close_requested("main");
        assert!(!q.on());
        assert!(!q.gone("main"), "its going asks no other window");
        // or another window
        q.ask("main");
        q.close_requested("main");
        q.close_requested("main-2");
        assert!(!q.on());
    }

    #[test]
    fn cancel_in_the_window_asked_stops_the_quit_at_once() {
        let mut q = Quit::default();
        q.ask("main");
        q.close_requested("main");
        // Cancel in another window's question (one it asked as the person closed it) is not the quit's
        assert!(!q.given_up("main-2"));
        assert!(q.on());
        assert!(q.given_up("main"), "the window the quit asked said 取消");
        assert!(!q.on());
        assert!(!q.gone("main"), "its going later asks no other window");
        assert!(!q.given_up("main"), "and only once");
    }

    #[test]
    fn a_close_with_no_quit_under_way_changes_nothing() {
        let mut q = Quit::default();
        q.close_requested("main");
        assert!(!q.on());
        assert!(!q.gone("main"));
    }
}
