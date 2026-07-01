# CI TEST + BUILD Maturity & Performance Optimization — Design

Date: 2026-07-01
Branch: `chore/add-ci-and-git-hooks`
Status: Approved (design, re-reviewed)

## Problem

The current CI (`.github/workflows/ci.yml`) gates on `cargo build --workspace`
and `cargo test --workspace --lib -- --test-threads=1`. A review found real
coverage gaps and one performance cost:

1. **`--lib` misses `puffer-cli`.** `puffer-cli` is a bin-only crate (no
   `lib.rs`). `cargo test --workspace --lib` runs only library targets, so the
   daemon, RPC handlers, and workflow tests — a large product surface — never
   run.
2. **Build does not cover all targets.** `cargo build --workspace` compiles only
   lib + bin production code. `#[cfg(test)]` modules, integration tests
   (`crates/*/tests/`), examples, and benches are not compiled by build; a
   compile break there is caught only by the informational (non-blocking)
   clippy step, which swallows it.
3. **The Tauri Rust backend is never compiled.** `apps/puffer-desktop/src-tauri`
   is a *separate* Cargo workspace, so root `cargo build --workspace` skips it.
   The desktop job only builds the frontend. A backend compile break ships
   undetected. (`backend.rs` alone is ~2400 lines — a real surface.)
4. **Tests run single-threaded** (`--test-threads=1`) as a stability stopgap.
   ~150 call sites resolve config via `ConfigPaths::discover`, which without a
   per-test home override reads the real `~/.puffer`; run in parallel, tests
   race on that shared session/project state and fail intermittently.
   Serializing is deterministic but ~4x slower (≈6.5 min vs ≈1.5 min).

Constraints: no backward-compatibility obligation; optimize for long-term value,
stability, and performance; avoid over-engineering.

## Design

### 1. Test isolation at the source (enables parallel)

Change `ConfigPaths::discover(workspace_root)` so that when `workspace_root` is
under the OS temporary directory, `user_config_dir` resolves to
`workspace_root/.puffer-user` instead of the real `$HOME/.puffer`. Outside the
temp dir (production), behavior is unchanged.

Precedence (unchanged order, new rule added as a fallback): explicit
`puffer_home_override()` thread-local → `$PUFFER_HOME` → **[new] temp-dir rule**
→ `$HOME` / `dirs::home_dir()`.

Detection must be robust:
- Canonicalize both `workspace_root` and `std::env::temp_dir()` before the
  `starts_with` check (macOS `/var` → `/private/var` symlink; trailing slashes).
- If canonicalization fails (e.g. the path does not exist yet), fall back to the
  existing production behavior. Never panic; never change production resolution.

What this covers and does not cover:
- **Covers the write-race source.** The intermittent failures are concurrent
  writes to the shared `user_config_dir` (session/project registry,
  `project_metadata.json`). Isolating `user_config_dir` per tempdir removes
  them. It also fixes `puffer-resources`'s loader tests, which read `~/.claude`
  via `user_config_dir.parent()` — so local verification is no longer perturbed
  by a developer's real `~/.claude`.
- **Does not cover direct `$HOME` reads.** A few tests read `$HOME/.claude` /
  `$HOME/.puffer` directly (e.g. `system_prompt::load_context_blocks`). These
  are **read-only** (no write race) and the individual assertion-sensitive cases
  are already fixed with `ScopedHome`. Not a parallelism blocker.

Add unit tests for the new `discover` behavior:
- Under a tempdir workspace root, `user_config_dir` is inside that tempdir.
- Under a non-temp (production) root, `user_config_dir` is `$HOME/.puffer`
  (existing behavior preserved).
- An explicit `PUFFER_HOME` / `puffer_home_override` still wins over the
  temp-dir rule.

### 2. Widen and parallelize the Rust gate

- **Test:** `cargo test --workspace --lib --bins --locked` — runs lib **and**
  bin unit tests (adds `puffer-cli`). Run **in parallel** (drop
  `--test-threads=1`), safe once §1 makes tests hermetic.
- **Build / compile coverage:** close the "integration-test compile break is
  invisible" gap. Preferred: `cargo build --workspace --all-targets --locked`
  (compiles lib, bin, integration tests, examples, benches). **Verify it
  compiles cleanly first.** If pre-existing examples/benches are bit-rotted, do
  **not** fix unrelated rot — narrow to `cargo test --workspace --tests --no-run
  --locked` (compiles all test targets, skips examples/benches), which still
  closes the real gap. Decide by the verification result.

### 3. Compile the desktop backend (new job)

Add a job `Desktop backend`:
- `apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev
  libayatana-appindicator3-dev librsvg2-dev` (plus the pkg-config/openssl deps
  already used by the Rust job).
- `cargo check --manifest-path apps/puffer-desktop/src-tauri/Cargo.toml
  --locked` (verify src-tauri's `Cargo.lock` is in sync so `--locked` passes).
- Runs in parallel with the other jobs → no critical-path wall-clock cost.
- `check` only (no build/link, no bundle) — minimal signal for a real gap.
- Heaviest addition (webkit deps); first thing to drop if the cost is judged not
  worth it.

### Resulting CI shape (three parallel jobs)

| Job | Hard-gate steps | Informational |
|---|---|---|
| Rust | build `--all-targets` (or `test --tests --no-run`); `test --workspace --lib --bins` (parallel) | rustfmt, clippy |
| Desktop (frontend) | `vite build`, `svelte-check`, node tests | — |
| Desktop backend | `cargo check` src-tauri | — |

Unchanged and already mature: least-privilege token, `push: [master]` +
`pull_request`, `concurrency` cancel, `timeout-minutes`, `--locked`, rust-cache,
npm cache.

## Verification

1. `cargo build --workspace --all-targets --locked` compiles cleanly (decides
   `--all-targets` vs `--tests --no-run` for §2).
2. `cargo check --manifest-path apps/puffer-desktop/src-tauri/Cargo.toml
   --locked` compiles (locally if the host has the apt deps, otherwise accept
   CI-only verification).
3. With a clean HOME (matching CI), run `cargo test --workspace --lib --bins
   --locked` **in parallel**, several times, to confirm §1 removes the
   contention and the suite is deterministically green.
4. If a small number of residual non-`~/.puffer` races remain (the known
   non-home races — an image-response socket test and an `op` process-group
   kill — are already fixed), fix each directly.
5. Push; confirm PR CI green; Rust job drops back to ≈1.5 min.

## Explicit non-goals (avoid over-engineering)

- No per-test hermeticity edits across the ~150 call sites — §1 replaces that.
- No integration/e2e infrastructure (tmux, workflow-runtime image, Playwright).
- No full Tauri app bundle/e2e — backend `cargo check` only.
- No doctest gating (doctests remain uncompiled/unrun).
- No fixing unrelated example/bench rot — narrow the compile step instead.

## Fallback

If parallel proves unexpectedly flaky beyond a handful of fixable tests, revert
the Test step to `--test-threads=1` (still a mature, deterministic, all-crate
gate) while keeping the coverage widenings (`--bins`, all-targets compile) and
the `discover` isolation (which stands alone as a correctness/pollution fix).
