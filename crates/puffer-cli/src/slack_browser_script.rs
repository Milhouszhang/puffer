//! JS snippets + pure parsers for the Slack browser connector.
//! Selectors are confirmed by the Phase 0 spike; keep ALL of them here.

// SELECTORS BELOW ARE BEST-EFFORT FROM SLACK data-qa HOOKS — NOT YET LIVE-VALIDATED (see spec §3, live-test task).

/// Returns `{ loggedIn: bool, href: string }`.
/// `loggedIn` is true when the URL path is under `/client/<TeamId>/…` (matches
/// `/\/client\/T[A-Z0-9]+/`), a client-shell hook is present in the DOM, and the
/// page is NOT on a sign-in path (`/signin`, `/get-started`, `/workspace-signin`).
pub(crate) const SLACK_LOGIN_MARKER_JS: &str = r#"(() => {
  try {
    const p = location.pathname;
    const onClientPath = /\/client\/T[A-Z0-9]+/.test(p);
    const onSigninPath = /\/(signin|get-started|workspace-signin)(\/|$)/i.test(p);
    const shellPresent = !!(
      document.querySelector('[data-qa="message_input"]') ||
      document.querySelector('[data-qa="slack_kit_list"]') ||
      document.querySelector('[data-qa="workspace-drawer"]')
    );
    const loggedIn = onClientPath && shellPresent && !onSigninPath;
    return JSON.stringify({ loggedIn, href: location.href });
  } catch (e) { return JSON.stringify({ loggedIn: false, href: '' }); }
})()"#;

/// Returns `{ self_id: string }`.
/// Best-effort: tries Slack's boot-data globals first, then the sidebar profile
/// hook, then falls back to `""`. Every access is wrapped in try/catch.
pub(crate) const SLACK_SELF_ID_JS: &str = r#"(() => {
  let self_id = '';
  try { if (window.TS && TS.model && TS.model.user && TS.model.user.id) self_id = TS.model.user.id; } catch(e) {}
  if (!self_id) { try { if (window.boot_data && boot_data.user_id) self_id = boot_data.user_id; } catch(e) {} }
  if (!self_id) {
    try {
      const el = document.querySelector('[data-qa="current-user-customstatus-section"]') ||
                 document.querySelector('[data-qa="user-button"]');
      if (el) {
        const m = (el.getAttribute('href') || el.getAttribute('data-user-id') || '').match(/U[A-Z0-9]+/);
        if (m) self_id = m[0];
      }
    } catch(e) {}
  }
  return JSON.stringify({ self_id });
})()"#;

/// Returns `{ loaded: bool, rows: [{ channel_id, name, unread, mention }] }`.
/// `loaded` = true when the sidebar shell hook (`[data-qa="slack_kit_list"]` or
/// `[data-qa="channel_sidebar"]`) is present in the DOM.
/// Rows are read from `[data-qa^="channel_sidebar_name_"]` elements; `channel_id`
/// is the suffix after the prefix (e.g. `C…`/`D…`/`G…`); `unread`/`mention` are
/// best-effort from unread-class and badge descendant selectors on the enclosing row.
pub(crate) const SLACK_SIDEBAR_SCRIPT: &str = r#"(() => {
  const loaded = !!(
    document.querySelector('[data-qa="slack_kit_list"]') ||
    document.querySelector('[data-qa="channel_sidebar"]')
  );
  const PREFIX = 'channel_sidebar_name_';
  const nameEls = Array.from(document.querySelectorAll('[data-qa^="' + PREFIX + '"]'));
  const rows = nameEls.map(el => {
    const qa = el.getAttribute('data-qa') || '';
    const channel_id = qa.startsWith(PREFIX) ? qa.slice(PREFIX.length) : '';
    if (!channel_id) return null;
    const name = (el.textContent || '').trim();
    // Walk up to find the enclosing channel row container.
    let row = el;
    for (let i = 0; i < 6; i++) {
      if (!row.parentElement) break;
      row = row.parentElement;
      if (row.getAttribute('data-qa') && row.getAttribute('data-qa').includes('channel_sidebar_item')) break;
    }
    const unread = !!(
      row.querySelector('[data-qa*="unread"]') ||
      row.querySelector('[class*="unread" i]')
    );
    const mentionEl = row.querySelector('[data-qa*="badge"]') || row.querySelector('[data-qa*="mention"]');
    const mention = mentionEl ? parseInt((mentionEl.textContent || '').trim(), 10) > 0 : false;
    return { channel_id, name, unread, mention };
  }).filter(r => r !== null && r.channel_id);
  return JSON.stringify({ loaded, rows });
})()"#;

/// Installs an idempotent MutationObserver that records Slack message containers
/// into `window.__cap`. Guard: re-running is a no-op when `window.__capObs` is set.
/// Each captured item: `{ ts, sender_id, text }`.
/// - `ts`: prefers raw dotted ts from `data-ts`/`id`; falls back to permalink
///   `/archives/<chan>/p<digits>` normalised to `<10digits>.<6digits>`.
/// - `sender_id`: from sender `<a>` href matching `/team/(U[A-Z0-9]+)`.
/// - `text`: `[data-qa="message-text"]` textContent trimmed, capped at 2000 chars.
pub(crate) const SLACK_OBSERVER_INSTALL_JS: &str = r#"(() => {
  window.__cap = window.__cap || [];
  if (window.__capObs) return JSON.stringify({ status: 'already', seeded: window.__cap.length });
  const seen = new Set();

  function normTs(raw) {
    // raw is either "1234567890.123456" (good) or a permalink p-form raw string
    if (/^\d+\.\d+$/.test(raw)) return raw;
    // permalink form: p1234567890123456 — 16 digits; split at 10
    const m = raw.match(/^p?(\d{10})(\d{6})$/);
    if (m) return m[1] + '.' + m[2];
    return raw;
  }

  function extractTs(el) {
    // 1. data-ts attribute (most reliable)
    const dt = el.getAttribute('data-ts');
    if (dt && /\d/.test(dt)) return normTs(dt);
    // 2. id attribute (e.g. "1718000000.000200")
    const id = el.id || '';
    if (/^\d+\.\d+$/.test(id)) return id;
    // 3. permalink anchor href
    const a = el.querySelector('a[href*="/archives/"]');
    if (a) {
      const m = (a.getAttribute('href') || '').match(/\/p(\d{16})/);
      if (m) return normTs('p' + m[1]);
    }
    return '';
  }

  function record(el) {
    if (!el || !el.matches) return;
    if (!el.matches('[data-qa="message_container"]') &&
        !el.querySelector('[data-qa="message_container"]')) return;
    const container = el.matches('[data-qa="message_container"]') ? el
      : el.querySelector('[data-qa="message_container"]');
    if (!container) return;
    const ts = extractTs(container);
    if (!ts || seen.has(ts)) return;
    seen.add(ts);
    const senderLink = container.querySelector('a[href*="/team/U"]');
    const senderMatch = senderLink ? (senderLink.getAttribute('href') || '').match(/\/team\/(U[A-Z0-9]+)/) : null;
    const sender_id = senderMatch ? senderMatch[1] : '';
    const textEl = container.querySelector('[data-qa="message-text"]');
    const text = textEl ? (textEl.textContent || '').trim().slice(0, 2000) : '';
    window.__cap.push({ ts, sender_id, text });
  }

  // Seed existing nodes
  document.querySelectorAll('[data-qa="message_container"]').forEach(record);

  window.__capObs = new MutationObserver(muts => {
    for (const m of muts) {
      for (const n of m.addedNodes) {
        if (n.nodeType !== 1) continue;
        record(n);
        if (n.querySelectorAll) n.querySelectorAll('[data-qa="message_container"]').forEach(record);
      }
    }
  });
  window.__capObs.observe(document.body, { childList: true, subtree: true });
  return JSON.stringify({ status: 'installed', seeded: window.__cap.length });
})()"#;

/// Returns and CLEARS `window.__cap`.
/// `channel_id` is read from the URL pathname: `/client/<TeamId>/(<ChannelId>)`.
pub(crate) const SLACK_OBSERVER_DRAIN_JS: &str = r#"(() => {
  const cap = window.__cap || [];
  window.__cap = [];
  const m = location.pathname.match(/\/client\/T[A-Z0-9]+\/([A-Z0-9]+)/i);
  const channel_id = m ? m[1] : '';
  return JSON.stringify({ channel_id, items: cap });
})()"#;

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
