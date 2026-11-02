#!/usr/bin/env bash
#
# Tessera — the authoritative verification gate.
#
# This is the one that counts. It runs on the pinned toolchain and runs Verus
# over verus/. The gates run in the cloud during the build are a weaker
# pre-check on a different machine with no Verus at all; a green cloud build
# says nothing about the proofs.
#
# There are no services: the Commons lives in memory (docs/adr/0001). So there
# is no `docker compose` step — the end-to-end gate boots the real binary,
# paints through the CSRF-protected endpoint the browser uses, and checks the
# world moved. Safe to run twice.
#
#   ./verify.sh          run everything
#   ./verify.sh --quick  skip the boot / end-to-end gate
#
set -euo pipefail
cd "$(dirname "$0")"

GATES=()
STATUSES=()
FAILED=0
QUICK=0
APP_PID=""
SCRATCH=""
PORT="${TESSERA_VERIFY_PORT:-3017}"

if [ "${1:-}" = "--quick" ]; then QUICK=1; fi

if [ -t 1 ]; then
    BOLD=$(printf '\033[1m'); RED=$(printf '\033[31m'); GREEN=$(printf '\033[32m')
    YELLOW=$(printf '\033[33m'); OFF=$(printf '\033[0m')
else
    BOLD=""; RED=""; GREEN=""; YELLOW=""; OFF=""
fi

record() { GATES+=("$1"); STATUSES+=("$2"); if [ "$2" = "FAIL" ]; then FAILED=1; fi; }
step() { printf '\n%s── %s %s\n' "$BOLD" "$1" "$OFF"; }
run_gate() {
    local name="$1"; shift
    step "$name"
    if "$@"; then record "$name" "PASS"; else record "$name" "FAIL"; fi
}
cleanup() {
    if [ -n "${APP_PID:-}" ]; then kill "$APP_PID" 2>/dev/null || true; wait "$APP_PID" 2>/dev/null || true; fi
    if [ -n "${SCRATCH:-}" ] && [ -d "$SCRATCH" ]; then rm -rf "$SCRATCH"; fi
}
trap cleanup EXIT INT TERM

# ── Gate 0: the toolchain must match rust-toolchain.toml ────────────────────
step "toolchain"
EXPECTED=$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml | head -1)
if [ -z "$EXPECTED" ]; then printf '%sFAIL%s cannot read channel from rust-toolchain.toml\n' "$RED" "$OFF"; exit 1; fi
if ! command -v cargo >/dev/null 2>&1; then printf '%sFAIL%s cargo is not on PATH\n' "$RED" "$OFF"; exit 1; fi
ACTUAL=$(cargo --version | awk '{print $2}')
printf 'expected %s, found cargo %s\n' "$EXPECTED" "$ACTUAL"
if [ "$ACTUAL" != "$EXPECTED" ]; then
    printf '%sFAIL%s toolchain mismatch: rust-toolchain.toml pins %s, cargo reports %s.\n' "$RED" "$OFF" "$EXPECTED" "$ACTUAL"
    printf '  Install it with:  rustup toolchain install %s\n' "$EXPECTED"
    printf '  Refusing to run: a green result on the wrong compiler is worse than none.\n'
    exit 1
fi
record "toolchain" "PASS"

# ── Gates 1-3: format, lint, tests ──────────────────────────────────────────
run_gate "cargo fmt"    cargo fmt --all -- --check
run_gate "cargo clippy" cargo clippy --all-targets -- -D warnings
run_gate "cargo test"   cargo test --workspace

# ── Gate 4: services ────────────────────────────────────────────────────────
step "services"
printf 'none: the Commons lives in memory (docs/adr/0001-the-commons-lives-in-memory.md)\n'
record "services (none)" "PASS"

# ── Gate 5: the browser engine equals the Rust engine ───────────────────────
parity() {
    if ! command -v node >/dev/null 2>&1; then
        printf '%sno node on PATH%s — the port was checked in the cloud build; install node to check it here\n' "$YELLOW" "$OFF"
        return 2
    fi
    local out
    out=$(mktemp "${TMPDIR:-/tmp}/tessera-parity.XXXXXX")
    cargo run -q --release -p tessera --example dump -- avalon lyonesse hy-brasil thule ys antillia mu kitezh > "$out"
    node tools/parity.js "$out"
    local rc=$?
    rm -f "$out"
    return $rc
}
step "js parity"
set +e; parity; rc=$?; set -e
case $rc in 0) record "js parity" "PASS" ;; 2) record "js parity" "SKIP" ;; *) record "js parity" "FAIL" ;; esac

# ── Gate 6: boot and end to end ─────────────────────────────────────────────
# `curl | grep -q` would trip pipefail when grep exits early, so fetch to a
# file first and grep the file.
serves() {
    curl -fsS "$@" -o "$SCRATCH/body" && grep -q -- "$PATTERN" "$SCRATCH/body"
}

e2e() {
    SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/tessera-verify.XXXXXX")
    local base="http://127.0.0.1:$PORT" jar="$SCRATCH/jar"
    if curl -fsS --max-time 2 "$base/health" >/dev/null 2>&1; then
        printf '%sFAIL%s something already serves on port %s; set TESSERA_VERIFY_PORT\n' "$RED" "$OFF" "$PORT"
        return 1
    fi
    cargo build --release --bin tessera-app
    TESSERA_DRIFT=off AUTUMN_PROFILE=dev AUTUMN_SERVER__PORT="$PORT" \
    AUTUMN_SECURITY__SIGNING_SECRET="${AUTUMN_SECURITY__SIGNING_SECRET:-verify-sh-secret-not-for-production-0123456789}" \
        ./target/release/tessera-app > "$SCRATCH/boot.log" 2>&1 &
    APP_PID=$!
    local up=0
    for _ in $(seq 1 60); do
        if ! kill -0 "$APP_PID" 2>/dev/null; then break; fi
        if curl -fsS "$base/health" >/dev/null 2>&1; then up=1; break; fi
        sleep 1
    done
    if [ "$up" -ne 1 ]; then printf 'did not boot:\n'; tail -40 "$SCRATCH/boot.log"; return 1; fi

    curl -fsS "$base/" -c "$jar" -b "$jar" -o "$SCRATCH/index.html"
    grep -q 'canvas id="map"' "$SCRATCH/index.html" || { echo 'no map on /'; return 1; }
    PATTERN='Every word is an island' serves "$base/atlas" || { echo '/atlas'; return 1; }
    PATTERN='444 tiles' serves "$base/tiles" || { echo '/tiles'; return 1; }
    PATTERN='What is proved' serves "$base/about" || { echo '/about'; return 1; }
    PATTERN='TesseraEngine' serves "$base/engine.js" || { echo '/engine.js'; return 1; }
    PATTERN='TesseraRender' serves "$base/render.js" || { echo '/render.js'; return 1; }
    PATTERN='"seq":0' serves "$base/_world.json" || { echo 'world did not start at seq 0'; return 1; }

    local token
    token=$(sed -n 's/.*name="csrf-token" content="\([^"]*\)".*/\1/p' "$SCRATCH/index.html" | head -1)
    [ -n "$token" ] || { echo 'no csrf token in the page'; return 1; }

    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' -b "$jar" -X POST "$base/paint" -d 'cells=20,12&brush=mountain')
    [ "$code" = "403" ] || { echo "a paint without a CSRF token got $code, wanted 403"; return 1; }
    PATTERN='"seq":1' serves -b "$jar" -c "$jar" -H "x-csrf-token: $token" -X POST "$base/paint" \
        -d 'cells=6,31;7,31&brush=mountain&name=verify&hue=30' || { echo 'the paint did not land'; return 1; }
    PATTERN='"mountain"' serves "$base/_world.json" || { echo 'the pin is not on the world'; return 1; }
    code=$(curl -s -o /dev/null -w '%{http_code}' -b "$jar" -H "x-csrf-token: $token" -X POST "$base/paint" -d 'cells=0,0&brush=town')
    [ "$code" = "422" ] || { echo "a town on the shore got $code, wanted 422"; return 1; }
    PATTERN='"word":"Avalon"' serves -b "$jar" -H "x-csrf-token: $token" -X POST "$base/grow" -d 'word=Avalon' \
        || { echo 'growing a word failed'; return 1; }
    printf '%sbooted; every page served; a paint without a token was refused, one with it landed and\n' "$GREEN"
    printf 'pinned; a town on the shore was refused; a word grew a new world%s\n' "$OFF"
}
if [ "$QUICK" -eq 1 ]; then
    step "boot + end to end"; printf '%sskipped%s (--quick)\n' "$YELLOW" "$OFF"; record "boot + end to end" "SKIP"
else
    run_gate "boot + end to end" e2e
    if [ -n "$APP_PID" ]; then kill "$APP_PID" 2>/dev/null || true; wait "$APP_PID" 2>/dev/null || true; APP_PID=""; fi
fi

# ── Gate 7: Verus ───────────────────────────────────────────────────────────
# The proofs lean on vstd's API (`Set` changed shape between releases), so the
# version is pinned in verus/verus-version, as rust-toolchain.toml pins rustc.
# The first candidate at the pinned version wins.
step "verus"
VERUS_EXPECTED=$(tr -d '[:space:]' < verus/verus-version)
VERUS=""
VERUS_OTHERS=()
for candidate in "$(command -v verus 2>/dev/null || true)" "${VERUS_BIN:-}" \
                 "$HOME/verus/verus" "$HOME/verus/source/target-verus/release/verus"; do
    [ -z "$candidate" ] && continue
    [ -x "$candidate" ] || continue
    found=$("$candidate" --version 2>/dev/null | sed -n 's/^[[:space:]]*Version:[[:space:]]*//p' || true)
    if [ "$found" = "$VERUS_EXPECTED" ]; then VERUS="$candidate"; break; fi
    VERUS_OTHERS+=("$candidate (${found:-version unreadable})")
done
if [ -z "$VERUS" ]; then
    printf '%sFAIL%s no Verus %s found (verus/verus-version). Tried, in order:\n' "$RED" "$OFF" "$VERUS_EXPECTED"
    printf '    verus on PATH\n    $VERUS_BIN (%s)\n    %s/verus/verus\n    %s/verus/source/target-verus/release/verus\n' \
        "${VERUS_BIN:-unset}" "$HOME" "$HOME"
    if [ "${#VERUS_OTHERS[@]}" -gt 0 ]; then
        printf '  Found, at the wrong version:\n'
        printf '    %s\n' "${VERUS_OTHERS[@]}"
    fi
    printf '  Refusing to run: a green result on the wrong checker is worse than none.\n'
    record "verus" "FAIL"
else
    printf 'using %s (%s)\n' "$VERUS" "$VERUS_EXPECTED"
    for source in verus/src/*.rs; do
        name="verus $(basename "$source" .rs)"
        if "$VERUS" "$source"; then record "$name" "PASS"
        else
            printf '  %snot verified%s — docs/SPEC.md §5 ranks the obligations by confidence.\n' "$RED" "$OFF"
            record "$name" "FAIL"
        fi
    done
fi

# ── Summary ─────────────────────────────────────────────────────────────────
printf '\n%s─────────── tessera verify ───────────%s\n' "$BOLD" "$OFF"
i=0
while [ "$i" -lt "${#GATES[@]}" ]; do
    case "${STATUSES[$i]}" in PASS) c="$GREEN" ;; FAIL) c="$RED" ;; *) c="$YELLOW" ;; esac
    printf '  %-22s %s%s%s\n' "${GATES[$i]}" "$c" "${STATUSES[$i]}" "$OFF"
    i=$((i + 1))
done
printf '%s──────────────────────────────────────%s\n' "$BOLD" "$OFF"
if [ "$FAILED" -ne 0 ]; then printf '%sverification failed%s\n' "$RED" "$OFF"; exit 1; fi
printf '%sall gates passed%s\n' "$GREEN" "$OFF"
