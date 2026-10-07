#!/usr/bin/env bash
# Generate the deterministic benchmark/stress datasets (plan §33) into ./datasets
# (gitignored). Usage:
#
#   scripts/gen-datasets.sh [A|B|C|D|E|all] [--seed N] [--verify]
#
# --verify generates a second copy into a temp dir and compares BLAKE3 tree hashes.
# Total size for `all` is ~650 MB (E alone is ~250 MB: 24.9/26/100 MB files and an
# 80 MB data-URI PNG).
set -euo pipefail

cd "$(dirname "$0")/.."
which="${1:-all}"
shift || true
seed_args=()
verify=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --seed) seed_args=(--seed "$2"); shift 2 ;;
    --verify) verify=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

cargo build -p svg-core --release --example gen_dataset
bin="${CARGO_TARGET_DIR:-target}/release/examples/gen_dataset"

"$bin" "$which" datasets "${seed_args[@]}"

if [[ $verify -eq 1 ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  "$bin" "$which" "$tmp" "${seed_args[@]}" > /dev/null
  status=0
  for d in datasets/*/; do
    set_name="$(basename "$d")"
    [[ "$which" != all && "${which^^}" != "$set_name" ]] && continue
    a="$("$bin" hash "datasets/$set_name" | cut -d' ' -f1)"
    b="$("$bin" hash "$tmp/$set_name" | cut -d' ' -f1)"
    if [[ "$a" == "$b" ]]; then
      echo "verify $set_name: OK ($a)"
    else
      echo "verify $set_name: MISMATCH ($a vs $b)" >&2
      status=1
    fi
  done
  exit $status
fi
