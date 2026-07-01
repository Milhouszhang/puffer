# CI TEST + BUILD Maturity & Performance Optimization — Design

Date: 2026-07-01
Branch: `chore/add-ci-and-git-hooks`
Status: Approved (design)

## Problem

The current CI (`.github/workflows/ci.yml`) gates on `cargo build --workspace`
and `cargo test --workspace --lib -- --test-threads=1`. A review found real
coverage gaps and one performance cost:

1. **`--lib` misses `puffer-cli`.** `puffer-cli` is a bin-only crate (no
   `lib.rs`). `cargo test --workspace --lib` runs only library targets, so the
   daemon, RPC handlers, and workflow tests — a large product surface — are
   never run.
2. **Build does not cover all targets.** `cargo build --workspace` compiles only
   lib + bin production code. `#[cfg(test)]` modules, integration tests
   (`crates/*/tests/`), examples, and benches are not compiled by build; a
   compile break there is caught only by the informational (non-blocking)
   clippy step, which swallows it.
3. **The Tauri Rust backend is never compiled.** `apps/puffer-desktop/src-tauri`
   is a *separate* Cargo workspace, so root `cargo build --workspace` skips it.
   The desktop job only builds the frontend (vite/svelte/node). A backend
   compile break ships undetected.
4. **Tests run single-threaded** (`--test-threads=1`) as a stability stopgap.
   ~150 call sites resolve config via `ConfigPaths::discover`, which without a
   per-test home override reads the real `~/.puffer`; run in parallel, tests
   race on that shared session/project state and fail intermittently.
   Serializing is deterministic but ~4x slower (≈6.5 min vs ≈1.5 min).

Constraints for this work: no backward-compatibility obligation; optimize for
long-term value, stability, and performance; avoid over-engineering.

## Design

### 1. Test isolation at the source (enables parallel)

Change `ConfigPaths::discover(workspace_root)` so that when `workspace_root` is
located under the OS temporary directory (`std::env::temp_dir()` — where
`tempfile::tempdir()` creates its dirs), `user_config_dir` resolves to
`workspace_root/.puffer-user` instead of the real `$HOME/.puffer`. Outside the
temp dir (production), behavior is unchanged (real `~/.puffer`).

Rationale:
- **Single point of change.** No edits to the ~150 test call sites.
- **Automatic everywhere.** Works for both CI and local `cargo test`, zero
  configuration, no env var to remember.
- **Correct semantics.** A tempdir workspace *is* an ephemeral/test context;
  isolating its "user" config to that same tempdir is the right behavior and
  each test's dir is unique, so parallel tests never share state.
- **Bonus long-term value.** Tests stop polluting the developer's real
  `~/.puffer`.

Edge case: a real workspace physically located under the OS temp dir would get
an isolated user config. This is negligible and harmless.

Implementation notes:
- Compare canonicalized paths (macOS `/var/folders` vs `/private/var/folders`
  symlink) so the temp-dir check is reliable.
- The existing `puffer_home_override` thread-local and `$PUFFER_HOME` env var
  continue to take precedence (checked first); the temp-dir rule is the new
  fallback ahead of `$HOME`.

### 2. Widen and parallelize the Rust gate

- **Build:** `cargo build --workspace --all-targets --locked` — compiles every
  target (lib, bin, integration tests, examples, benches) across the workspace,
  closing the "test/example compile break is invisible" gap. (Integration tests
  are compiled here but still not *run* — running them needs infra: tmux, the
  workflow-runtime image.)
- **Test:** `cargo test --workspace --lib --bins --locked` — runs lib **and**
  bin unit tests (adds `puffer-cli`'s daemon/workflow suites). Run **in
  parallel** (drop `--test-threads=1`), now safe because §1 makes the tests
  hermetic.

### 3. Compile the desktop backend (new job)

Add a dedicated job `Desktop backend` that runs
`cargo check --manifest-path apps/puffer-desktop/src-tauri/Cargo.toml --locked`
after installing the Linux system dependencies Tauri needs (webkit2gtk / gtk).
`cargo check` (not `build`) is the minimal signal — it verifies the backend
compiles without codegen/link cost. As its own job it runs in parallel with the
Rust and Desktop-frontend jobs, so it adds no critical-path wall-clock.

### Resulting CI shape (three parallel jobs)

| Job | Hard-gate steps | Informational |
|---|---|---|
| Rust | `build --workspace --all-targets`; `test --workspace --lib --bins` (parallel) | rustfmt, clippy |
| Desktop (frontend) | `vite build`, `svelte-check`, node tests | — |
| Desktop backend | `cargo check` src-tauri | — |

Unchanged and already mature: least-privilege token, `push: [master]` +
`pull_request` triggers, `concurrency` cancel, `timeout-minutes`, `--locked`,
rust-cache, npm cache.

## Verification

1. Locally, with a clean HOME (matching CI), run `cargo test --workspace --lib
   --bins --locked` **in parallel**, several times, to confirm §1 removes the
   contention and the suite is deterministically green.
2. If a small number of residual non-`~/.puffer` races remain (the known
   non-home races — an image-response socket test and an `op` process-group
   kill — are already fixed), fix each directly; they should be few.
3. Push and confirm the PR CI is green; the Rust job should drop back to ≈1.5 min.

## Explicit non-goals (avoid over-engineering)

- No per-test hermeticity edits across the ~150 call sites — §1 replaces that.
- No integration/e2e infrastructure (tmux, workflow-runtime image, Playwright).
- No full Tauri app bundle/e2e — backend `cargo check` only.
- No doctest gating.

## Fallback

If parallel proves unexpectedly flaky beyond a handful of fixable tests, revert
the Test step to `--test-threads=1` (still a mature, deterministic, all-crate
gate) while keeping the coverage widenings (`--bins`, `--all-targets`) and the
`discover` isolation (which stands on its own as a correctness/pollution fix).
