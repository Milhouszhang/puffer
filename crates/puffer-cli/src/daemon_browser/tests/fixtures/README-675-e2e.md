# #675 e2e — main-document keystroke-guarded fill fallback

This documents the live-browser verification for the #675 fix (per-character
trusted-keystroke fallback for keystroke-guarded MAIN-DOCUMENT inputs). The
puffer-cli unit suite covers the pure helpers and expression builders
(`cargo test -p puffer-cli --bin puffer -- daemon_browser::`); the steps below
cover the parts that require a real Chromium/CEF browser driven over CDP and so
cannot run as a hermetic unit test in CI.

Fixture: `keystroke-guarded-main-input.html` (this directory). It has three
main-document inputs: a revertible-but-keystroke-fillable `#cardNo`, an
always-revert `#expiry`, and an unguarded `#name`. See the file header for how
the guard tells trusted keystrokes from programmatic value-setter writes.

## Steps (reuse the #633/#656 harness pattern)

1. Serve the fixture locally: `python3 -m http.server 8675` from this directory,
   so it is a real main-document page at `http://localhost:8675/keystroke-guarded-main-input.html`.
2. Start a daemon with an isolated `PUFFER_HOME` so it doesn't touch `~/.puffer`,
   then drive `browser_agent` RPCs over its websocket (the node `ws` harness used
   for #633/#656): `open` the tab on the fixture URL, `snapshot` to get refs,
   then `fill` each field.

## Expected results (assert all three)

- **(a) guarded-but-fillable** — `fill` on the `#cardNo` ref returns
  `{ ok: true, mode: "keystroke", ... }` and a follow-up `snapshot`/readback
  shows `#cardNo` holding the typed digits. The native value-setter fill is
  rejected (revert), the #675 fallback clicks → focuses → types per-char → reads
  back non-empty.
- **(b) honest bail** — `fill` on the `#expiry` ref returns an ERROR whose
  message contains `value did not stick` (it reverts even genuine keystrokes).
  No false success: the field is empty in the next snapshot. (Invariant #580.)
- **(c) unguarded untouched** — `fill` on the `#name` ref returns
  `{ ok: true }` with NO `mode: "keystroke"` (the fast native-setter path ran;
  the fallback never fired). Confirms gating: normal inputs are unaffected.

## #636 invariant check (no OS key-repeat storm)

While `#cardNo` is being filled, the dispatched `Input.dispatchKeyEvent` events
must carry `windowsVirtualKeyCode` but NOT `nativeVirtualKeyCode`. This is
enforced structurally (`BrowserInputEvent::Key` has no native-key-code field;
`input.rs::key_event_params` only emits `windowsVirtualKeyCode`) and asserted by
`type_char_events_carry_text_on_keydown_only_and_no_native_key_code` and the
`input.rs` regression test `native_virtual_key_code_is_omitted_to_avoid_macos_autorepeat_storm`.
