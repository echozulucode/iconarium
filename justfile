# Iconarium — task runner (https://just.systems)
#
#   just            list recipes
#   just dev        run the desktop app in development mode (hot reload)
#
# Install just:  winget install Casey.Just   (or: cargo install just)

# -ExecutionPolicy Bypass: npm/npx ship as .ps1 shims that the default policy would block.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

# List available recipes
default:
    @just --list --unsorted

# Run the desktop app in dev mode: Vite hot reload + Rust backend (rebuilds on change)
dev: deps
    npm run tauri dev

# Run only the UI in a browser against the built-in mock backend (no Rust needed)
dev-ui: deps
    npm run dev

# The bundler signs the installer for the auto-updater, so `build` needs the private key. Checking
# first fails in a second instead of after a multi-minute compile that leaves an .exe without a
# .sig (which can never be offered as an update).

# Signed release installer (NSIS) in target/release/bundle/nsis/ — needs TAURI_SIGNING_PRIVATE_KEY
build: deps
    @node tools/updater-key.mjs --check
    npm run tauri build

# Installer WITHOUT updater signing — local testing only, never for a release
build-unsigned: deps
    npm run tauri build -- --config src-tauri/tauri.unsigned.conf.json

# Debug build of the app without installers
build-debug: deps
    npm run tauri build -- --debug --no-bundle

# Install JavaScript dependencies when missing or when package.json / package-lock.json changed
deps:
    @node tools/ensure-deps.mjs

# Reinstall JavaScript dependencies unconditionally
install:
    npm install --no-audit --no-fund

# Bump the version in all three files that carry it, e.g. `just bump patch --tag`
bump *ARGS:
    node tools/bump-version.mjs {{ARGS}}

# Verify the three version files agree with a version or tag (what the release workflow checks)
check-version VERSION:
    node tools/bump-version.mjs --check {{VERSION}}

# One-time: generate the updater signing key in ~/.tauri and write its PUBLIC key into tauri.conf.json
updater-key:
    node tools/updater-key.mjs

# Everything CI runs
ci: check test

# Type-check and lint everything (format check, tsc, clippy with warnings as errors)
check: deps
    cargo fmt --all -- --check
    npx tsc -b
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests (Rust core + app shell + frontend)
test: deps
    cargo test --workspace
    npm test

# Rust core engine tests only
test-core:
    cargo test -p svg-core

# Frontend unit tests only
test-ui: deps
    npm test

# Format Rust code
fmt:
    cargo fmt --all

# Generate deterministic test libraries into ./datasets (A=1k, B=10k, C=50k, D=10k diagrams, E=pathological, all)
datasets set="all":
    cargo run -p svg-core --release --example gen_dataset -- {{set}} datasets

# Benchmark the core pipeline against a dataset folder (e.g. just bench datasets/C)
bench dir="datasets/C":
    cargo run -p svg-core --release --example bench_pipeline -- {{dir}}

# End-to-end tests of the real app (Linux only: Xvfb + tauri-driver, see scripts/e2e/README.md)
e2e: deps
    npm run build
    cargo build --release -p iconarium --features tauri/custom-protocol
    xvfb-run -a -s "-screen 0 1280x900x24" node scripts/e2e/run.mjs

# Remove build outputs (Rust target/ and dist/)
clean:
    cargo clean
    npx --yes rimraf dist
