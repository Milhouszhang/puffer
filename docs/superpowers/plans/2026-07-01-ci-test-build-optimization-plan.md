# CI TEST + BUILD Optimization — Execution Plan

Design: `ci-test-build-optimization-design.md`
Branch: `chore/add-ci-and-git-hooks`

Ordered phases; each ends green and is independently revertible. Commit after
each phase. Do NOT drop `--test-threads=1` (Phase 3) until Phase 1 verification
is green.

---

## Phase 0 — Baseline probes (decide two open choices)

No code changes; gather facts that pick between design options.

1. `cargo build --workspace --all-targets --locked` — does the whole workspace
   (incl. examples/benches/integration-test code) compile today?
   - **Green** → Phase 2 uses `cargo build --workspace --all-targets`.
   - **Fails only on examples/benches (unrelated rot)** → Phase 2 uses
     `cargo test --workspace --tests --no-run --locked` instead (do not fix the
     rot; that is an explicit non-goal).
2. `cd apps/puffer-desktop/src-tauri && cargo check --locked` (if the host has
   webkit deps) — confirms src-tauri compiles and its `Cargo.lock` is in sync.
   If the host lacks webkit, skip locally; Phase 4 verifies on CI.

**Exit:** the two commands above run; choices for Phase 2/3 recorded.

---

## Phase 1 — `discover` temp-dir isolation (the enabler)

File: `crates/puffer-config/src/lib.rs` (`ConfigPaths::discover`).

**Change:** after the `puffer_home_override()` and `$PUFFER_HOME` checks and
before the `$HOME`/`dirs::home_dir()` fallback, add: if `workspace_root`
canonicalizes to a path under the canonicalized `std::env::temp_dir()`, set
`user_config_dir = workspace_root.join(".puffer-user")`. On any canonicalize
error, fall through to the existing behavior.

Sketch:
```rust
fn under_temp_dir(workspace_root: &Path) -> bool {
    let (Ok(root), Ok(tmp)) = (workspace_root.canonicalize(),
                               std::env::temp_dir().canonicalize()) else {
        return false;
    };
    root.starts_with(tmp)
}
// in discover(), for user_config_dir, after override/$PUFFER_HOME, before $HOME:
//   if under_temp_dir(&workspace_root) { workspace_root.join(".puffer-user") }
```

**Tests** (same file's test module):
- tempdir root → `user_config_dir` is inside the tempdir.
- non-temp root (e.g. a fixed `/some/project`) → `user_config_dir` ==
  `$HOME/.puffer` (production unchanged); guard with the existing home lock if
  it mutates env.
- explicit `puffer_home_override` still wins over the temp-dir rule.

**Verification (the critical gate for going parallel):**
```bash
CLEAN=$(mktemp -d)
env HOME="$CLEAN" RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" \
  cargo test --workspace --lib --bins --locked   # parallel, no --test-threads
```
Run 3x. All green → §1 works, parallel is safe. Any failure → inspect:
- HOME-race regression → the isolation missed a path; fix in `discover`.
- Non-home race (socket/global) → fix that specific test.

**Commit:** `fix(puffer-config): isolate user_config_dir under tempdir in tests`.
**Rollback:** revert this commit; CI still green on serial.

---

## Phase 2 — Widen + parallelize the Rust gate

File: `.github/workflows/ci.yml` (rust job) + `README.md`.

- Build step → `cargo build --workspace --all-targets --locked`
  (or `cargo test --workspace --tests --no-run --locked` per Phase 0).
- Test step → `cargo test --workspace --lib --bins --locked`
  (remove `-- --test-threads=1`).
- Update the step comments and the README CI paragraph (drop the
  single-threaded rationale; note `--bins` and all-targets compile).

**Verification:** local parallel run already done in Phase 1; YAML validates;
`--locked` satisfiable. (Real proof is Phase 4 on CI.)

**Commit:** `ci: parallel gate over lib+bins with all-targets compile`.
**Rollback:** restore `--lib` + `--test-threads=1` (the current mature gate).

---

## Phase 3 — Desktop backend compile job

File: `.github/workflows/ci.yml` (new `desktop-backend` job).

```yaml
  desktop-backend:
    name: Desktop backend (cargo check)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - name: Install system dependencies
        run: sudo apt-get update && sudo apt-get install -y
             pkg-config libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev
             libayatana-appindicator3-dev librsvg2-dev
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: apps/puffer-desktop/src-tauri
      - name: cargo check (src-tauri)
        run: cargo check --manifest-path apps/puffer-desktop/src-tauri/Cargo.toml --locked
```

**Verification:** Phase 4 (CI). If webkit deps or `--locked` fail on CI, either
fix the src-tauri lockfile or drop this job (it is the designated trim point).

**Commit:** `ci: compile the desktop (Tauri) Rust backend`.
**Rollback:** delete the job.

---

## Phase 4 — Push, watch, confirm

1. Push; watch PR #1 CI.
2. Expect: Rust green (~1.5 min, parallel), Desktop green, Desktop-backend
   green.
3. If Rust flakes on a residual non-home race → fix that test, repeat.
4. If Desktop-backend fails on deps/lock → fix or drop per Phase 3.

**Done when:** all three jobs green on a PR run, Rust back to parallel speed.

---

## Sequencing note

Phase 1 must be verified green **before** Phase 2 removes `--test-threads=1`.
Phases 2 and 3 are independent and could land together, but keep separate
commits for clean rollback. The `discover` change (Phase 1) has standalone value
(correctness + no test pollution) even if parallelization were later reverted.
