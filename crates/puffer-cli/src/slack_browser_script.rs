//! JS snippets + pure parsers for the Slack browser connector.
//! Selectors are confirmed by the Phase 0 spike; keep ALL of them here.

// Placeholder JS — real selectors land in Task 7 after the spike.
pub(crate) const SLACK_SIDEBAR_SCRIPT: &str = r#"(() => JSON.stringify({ loaded: false, rows: [] }))()"#;

pub(crate) fn sidebar_loaded(result: &serde_json::Value) -> bool {
    result.get("loaded").and_then(|v| v.as_bool()).unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SidebarRow {
    pub channel_id: String,
    pub name: String,
    pub unread: bool,
    pub mention: bool,
}

pub(crate) fn parse_sidebar_rows(result: &serde_json::Value) -> Vec<SidebarRow> {
    result.get("rows").and_then(|v| v.as_array()).map(|rows| {
        rows.iter().filter_map(|r| {
            let channel_id = r.get("channel_id").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if channel_id.is_empty() { return None; }
            Some(SidebarRow {
                channel_id,
                name: r.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                unread: r.get("unread").and_then(|v| v.as_bool()).unwrap_or(false),
                mention: r.get("mention").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        }).collect()
    }).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveMsg {
    pub ts: String,
    pub sender_id: String,
    pub text: String,
}

/// A real Slack message ts is `<digits>.<digits>`. Optimistic/client ids are not.
pub(crate) fn is_message_ts(ts: &str) -> bool {
    match ts.split_once('.') {
        Some((a, b)) => !a.is_empty() && !b.is_empty()
            && a.bytes().all(|c| c.is_ascii_digit())
            && b.bytes().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

pub(crate) fn parse_active_drain(result: &serde_json::Value) -> (String, Vec<ActiveMsg>) {
    let channel_id = result.get("channel_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let msgs = result.get("items").and_then(|v| v.as_array()).map(|items| {
        items.iter().filter_map(|m| {
            let ts = m.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            if !is_message_ts(ts) { return None; }
            Some(ActiveMsg {
                ts: ts.to_string(),
                sender_id: m.get("sender_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                text: m.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            })
        }).collect()
    }).unwrap_or_default();
    (channel_id, msgs)
}

pub(crate) fn conversation_type_for(channel_id: &str) -> &'static str {
    match channel_id.chars().next() {
        Some('D') => "dm",
        Some('G') => "group_dm",
        _ => "channel",
    }
}

#[cfg(test)]
mod parser_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ts_detection() {
        assert!(is_message_ts("1718000000.123456"));
        assert!(!is_message_ts("pending-abc")); // optimistic/client id
        assert!(!is_message_ts(""));
    }

    #[test]
    fn conversation_type_from_prefix() {
        assert_eq!(conversation_type_for("C0001"), "channel");
        assert_eq!(conversation_type_for("D0001"), "dm");
        assert_eq!(conversation_type_for("G0001"), "group_dm");
        assert_eq!(conversation_type_for("X0001"), "channel"); // default
    }

    #[test]
    fn sidebar_loaded_gate() {
        assert!(sidebar_loaded(&json!({"loaded": true, "rows": []})));
        assert!(!sidebar_loaded(&json!({"loaded": false, "rows": []})));
        assert!(!sidebar_loaded(&json!({"rows": []})));
        assert!(!sidebar_loaded(&serde_json::Value::Null));
    }

    #[test]
    fn parses_sidebar_rows() {
        let result = json!({"loaded": true, "rows": [
            {"channel_id": "C111", "name": "general", "unread": true, "mention": false},
            {"channel_id": "D222", "name": "Bob", "unread": false, "mention": true}
        ]});
        let rows = parse_sidebar_rows(&result);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].channel_id, "C111");
        assert!(rows[0].unread);
        assert!(rows[1].mention);
    }

    #[test]
    fn sidebar_skips_rows_without_channel_id() {
        assert!(parse_sidebar_rows(&json!({"rows": [{"name": "x"}]})).is_empty());
    }

    #[test]
    fn parse_active_drain_drops_pending_ids() {
        let result = json!({"channel_id": "C999", "items": [
            {"ts": "pending-xyz", "sender_id": "U1", "text": "sending"},
            {"ts": "1718000000.000200", "sender_id": "U1", "text": "sent"},
            {"ts": "1718000000.000300", "sender_id": "U2", "text": "reply"}
        ]});
        let (chan, msgs) = parse_active_drain(&result);
        assert_eq!(chan, "C999");
        assert_eq!(msgs.len(), 2); // pending dropped
        assert_eq!(msgs[0].ts, "1718000000.000200");
        assert_eq!(msgs[0].sender_id, "U1");
        assert_eq!(msgs[1].sender_id, "U2");
    }
}
