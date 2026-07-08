# Contributing to k8pk

## Build and test

```bash
cd rust/k8pk
cargo fmt
cargo clippy --locked -- -D warnings
cargo test --locked
```

CI runs the same checks (see `.github/workflows/checks.yml`).

## Man pages (optional)

Man pages are generated at build time when **`K8PK_MAN_DIR`** points to an output directory:

```bash
cd rust/k8pk
mkdir -p ../../man-out
K8PK_MAN_DIR="$(pwd)/../../man-out" cargo build
ls ../../man-out
```

Release archives include `share/man/man1/*.1` when built with `K8PK_MAN_DIR` set (see `.github/workflows/ci.yml`).

## Project layout

- `rust/k8pk/` — CLI implementation (Rust)
- `shell/` — `k8pk.sh`, `k8pk.fish`
- `wezterm/` — WezTerm plugin (Lua)
- `tests/fixtures/` — Test helpers (e.g. `fake-oc.sh` for OpenShift CLI overrides)

Further packaging options (Nix, AUR, deb) are described in [docs/packaging.md](docs/packaging.md).
