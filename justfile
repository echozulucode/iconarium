# SVG Library Browser — task runner (https://just.systems)
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

# Release build + Windows installers (NSIS/MSI) in target/release/bundle/
build: deps
    npm run tauri build

# Debug build of the app without installers
build-debug: deps
    npm run tauri build -- --debug --no-bundle

# Install JavaScript dependencies when missing or when package.json / package-lock.json changed
[windows]
deps:
    $stamp = 'node_modules/.deps-stamp'; if (-not (Test-Path $stamp) -or (Get-Item package.json, package-lock.json | Where-Object { $_.LastWriteTime -gt (Get-Item $stamp).LastWriteTime })) { npm install --no-audit --no-fund; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; New-Item -ItemType File -Force $stamp | Out-Null }

[unix]
deps:
    #!/usr/bin/env sh
    set -e
    stamp=node_modules/.deps-stamp
    if [ ! -f "$stamp" ] || [ package.json -nt "$stamp" ] || [ package-lock.json -nt "$stamp" ]; then
        npm install --no-audit --no-fund
        touch "$stamp"
    fi

# Reinstall JavaScript dependencies unconditionally
install:
    npm install --no-audit --no-fund

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

# Type-check and lint everything
check: deps
    npx tsc -b
    cargo clippy --workspace --all-targets

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
[unix]
e2e: deps
    npm run build
    cargo build --release -p svg-library-browser --features tauri/custom-protocol
    xvfb-run -a -s "-screen 0 1280x900x24" node scripts/e2e/run.mjs

# Remove build outputs (Rust target/ and dist/)
clean:
    cargo clean
    npx --yes rimraf dist
