use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, Window, WindowEvent};

#[derive(Default)]
pub struct BadgeState {
    unread: i64,
    focused: bool,
}

impl BadgeState {
    /// One conversation turn completed. While focused, do not accumulate
    /// (return None, leave the dock untouched); while unfocused, increment
    /// `unread` and return `Some(new count)`.
    fn record_completion(&mut self) -> Option<i64> {
        if self.focused {
            return None;
        }
        self.unread += 1;
        Some(self.unread)
    }

    /// Focus changed. Only when *newly* gaining focus with unread > 0 do we
    /// reset to zero and return `Some(0)` (the badge must be cleared); every
    /// other case returns None (blur, repeated focus, nothing unread).
    fn focus_changed(&mut self, focused: bool) -> Option<i64> {
        let gained = focused && !self.focused;
        self.focused = focused;
        if gained && self.unread > 0 {
            self.unread = 0;
            Some(0)
        } else {
            None
        }
    }
}

/// The only function that touches Tauri/OS. `n > 0` shows the number;
/// `n == 0` clears the badge. Best-effort: errors are ignored.
fn apply(app: &AppHandle, n: i64) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_badge_count(if n > 0 { Some(n) } else { None });
    }
}

/// Invoked once per non-replay conversation completion from the frontend.
/// The focus gate lives here (Rust) as the single source of truth.
#[tauri::command]
pub fn badge_bump(app: AppHandle, state: State<'_, Mutex<BadgeState>>) {
    // `.lock().ok()` keeps the best-effort policy (no panic on a poisoned
    // mutex); the guard is dropped when the closure returns, so the OS call
    // below never runs while the lock is held.
    let next = state.lock().ok().and_then(|mut s| s.record_completion());
    if let Some(n) = next {
        apply(&app, n);
    }
}

/// Hooked to `.on_window_event`. Only handles focus changes for the main window.
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::Focused(focused) = event {
        let app = window.app_handle();
        let state = app.state::<Mutex<BadgeState>>();
        let next = state
            .lock()
            .ok()
            .and_then(|mut s| s.focus_changed(*focused));
        if let Some(n) = next {
            apply(app, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BadgeState;

    #[test]
    fn completion_while_unfocused_accumulates() {
        let mut s = BadgeState::default(); // focused = false
        assert_eq!(s.record_completion(), Some(1));
        assert_eq!(s.record_completion(), Some(2));
    }

    #[test]
    fn completion_while_focused_is_ignored() {
        let mut s = BadgeState::default();
        s.focus_changed(true);
        assert_eq!(s.record_completion(), None);
        // 聚焦时不累加:再失焦后从 1 开始
        s.focus_changed(false);
        assert_eq!(s.record_completion(), Some(1));
    }

    #[test]
    fn gaining_focus_clears_accumulated() {
        let mut s = BadgeState::default();
        s.record_completion();
        s.record_completion();
        assert_eq!(s.focus_changed(true), Some(0));
        // 已清零,再次进入聚焦无副作用
        assert_eq!(s.focus_changed(true), None);
    }

    #[test]
    fn gaining_focus_with_nothing_unread_is_noop() {
        let mut s = BadgeState::default();
        assert_eq!(s.focus_changed(true), None);
    }

    #[test]
    fn losing_focus_never_touches_badge() {
        let mut s = BadgeState::default();
        s.focus_changed(true);
        assert_eq!(s.focus_changed(false), None);
    }
}
