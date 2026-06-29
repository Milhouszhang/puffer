use std::sync::Mutex;
use tauri::{AppHandle, Manager, Window, WindowEvent, State};

#[derive(Default)]
pub struct BadgeState {
    unread: i64,
    focused: bool,
}

impl BadgeState {
    /// 一次对话完成。聚焦时不累加(返回 None,不碰 dock);
    /// 未聚焦时 unread += 1 并返回 Some(新值)。
    pub fn record_completion(&mut self) -> Option<i64> {
        if self.focused {
            return None;
        }
        self.unread += 1;
        Some(self.unread)
    }

    /// 焦点变化。仅"新获得焦点且有未读"时归零并返回 Some(0)(需清除徽章);
    /// 其余情况返回 None(失焦、重复聚焦、无未读)。
    pub fn focus_changed(&mut self, focused: bool) -> Option<i64> {
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

/// 唯一触碰 Tauri/OS 的函数。n > 0 显示数字;n == 0 清除徽章。best-effort。
pub fn apply(app: &AppHandle, n: i64) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_badge_count(if n > 0 { Some(n) } else { None });
    }
}

/// 前端在每个非 replay 的对话完成上调用一次。焦点门控在此(Rust)单一判定。
#[tauri::command]
pub fn badge_bump(app: AppHandle, state: State<'_, Mutex<BadgeState>>) {
    let next = state.lock().unwrap().record_completion();
    if let Some(n) = next {
        apply(&app, n);
    }
}

/// 接到 `.on_window_event`。仅处理 main 窗口的焦点变化。
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::Focused(focused) = event {
        let app = window.app_handle();
        let state = app.state::<Mutex<BadgeState>>();
        let next = state.lock().unwrap().focus_changed(*focused);
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
