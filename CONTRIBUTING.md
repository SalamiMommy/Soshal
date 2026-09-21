# Contributing to Soshal

## Development Setup

See [README.md](./README.md) for prerequisites and install steps.

## Code Conventions

### Rust Workspace (Desktop & Core Crates)

- **Pure Rust** — All application logic in Rust across 32 core crates; zero TypeScript.
- **Type check** — `cargo check --workspace` must pass cleanly.
- **FFI Wiring** — Every new bridge fn lives in `flutter-bridge/src/ffi/<module>.rs` with `#[frb(sync, serialize)]`; regenerate bindings before release (see AGENTS.md ritual).
- **Command Wiring** — every Flutter screen/service call must resolve to a live Rust fn (`rg "pub fn <module>_" flutter-bridge/src/ffi/`); stale codegen artifacts in `frb_generated.rs` are traps — never call them.

### Annotation Standards

| Element | Annotation |
|---------|-----------|
| Rust `pub` function / struct / field | `///` doc comment |
| Rust module | `//!` doc comment on every module |
| `flutter-bridge` FFI fn | `///` describing purpose, params, and return values |

### Core Crate Boundaries

- Each `*-core` crate is a pure Rust library with `//!` module docs and `///` item docs.
- Cores must NOT depend on platform crates (tauri/flutter_rust_bridge/wasm/ndk). Desktop-specific dependencies live in cores; `flutter-bridge` thin fns delegate to core crates.
- `soshal_flutter` is the only UI; Dart lint is gated at 0 errors / 0 warnings in CI.

### Testing

- Unit tests live inside each core crate (inline `#[cfg(test)]` modules).
- Run `cargo test --workspace` before pushing.
- Formatter: `cargo fmt --check` enforced via pre-commit hook.

## Git Workflow

1. **Branch from `main`** — `git checkout -b feat/my-feature`
2. **Commit messages** — Conventional Commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `perf:`, `chore:`
3. **Pre-push hook** runs: `cargo check --workspace && cargo test --workspace` (+ `cargo audit` when installed)
4. **Pre-commit hook** runs: `cargo fmt --check` + `cargo clippy --workspace -- -D warnings` + `guard-lib-platform.sh`
5. **PR** — squash-merge to main with descriptive message

## Build Commands

```bash
cargo check --workspace   # Type-check all crates
cargo test --workspace    # Run native test suite (~2500 tests)
cargo fmt --check         # Check formatting
cargo clippy --workspace -- -D warnings
cargo audit               # Dependency audit (pre-push + CI)
./builds/android/build.sh            # 3-ABI bridge + debug APK (add --release)
./builds/linux/build.sh              # host bridge + Linux bundle (add --release)
cd soshal_flutter && flutter analyze # Dart lint gate: 0 errors / 0 warnings
```

## Security

Report vulnerabilities to `dev@soshal.app`. See [SECURITY.md](./SECURITY.md) for scope and response SLA.
