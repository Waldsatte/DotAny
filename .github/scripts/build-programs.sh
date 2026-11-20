#!/usr/bin/env bash
# Builds every DotAny program once, in release, for CI. Used by the release workflow; the same commands build them on
# a computer of your own (the top README shows how).
#
#   bash .github/scripts/build-programs.sh <windows|macos> [names...]
#
# Needs two folders from the environment:
#   CARGO_TARGET_DIR  one Cargo target folder shared by all the programs, so the big shared crates (tauri, wry, the
#                     image/audio/archive libraries) are compiled once and every program only compiles its own part.
#   DOTANY_BUILD      where the finished programs are put, in the layout installer/payload.json reads:
#                     $DOTANY_BUILD/cargo-target-<name>/release/<program>   (dotany-pack's DOTANY_BUILD)
#
# A program that fails to build doesn't stop the others, so one run shows every problem. The exit code is 1 if any
# failed. The result is written to $DOTANY_BUILD/build-report.txt and, in CI, to the job summary.
set -uo pipefail

PLATFORM="${1:?say windows or macos}"
shift || true
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR}"
: "${DOTANY_BUILD:?set DOTANY_BUILD}"
export CARGO_INCREMENTAL=0

NAMES="${*:-hub text image audio video recorder model archive books sheets slides pdf}"
EXT=""; [ "$PLATFORM" = windows ] && EXT=".exe"

mkdir -p "$DOTANY_BUILD"
REPORT="$DOTANY_BUILD/build-report.txt"
: > "$REPORT"
failed=0
all_start=$(date +%s)

for name in $NAMES; do
  dir="$REPO/$name"; [ "$name" = starter ] && dir="$REPO/shared/starter"
  exe="dotany-$name$EXT"; [ "$name" = hub ] && exe="dotany$EXT"
  echo "::group::build $name"
  start=$(date +%s)
  # npm ci: exactly what package-lock.json says. `tauri build --no-bundle` runs the page build (scripts/build.mjs,
  # which also writes the licence notices) and then cargo build --release; no installer/bundle is made per program.
  # What follows `--` goes to cargo: --locked builds exactly the crates Cargo.lock names, or stops.
  ( cd "$dir" && npm ci --no-audit --no-fund && npx tauri build --no-bundle -- --locked )
  code=$?
  secs=$(( $(date +%s) - start ))
  echo "::endgroup::"
  if [ $code -eq 0 ] && [ -f "$CARGO_TARGET_DIR/release/$exe" ]; then
    dest="$DOTANY_BUILD/cargo-target-$name/release"
    mkdir -p "$dest"
    cp "$CARGO_TARGET_DIR/release/$exe" "$dest/"
    # Audio (and the hub and Recorder through it) write MP3 with libmp3lame.dll, which the build puts beside the program.
    # The installer's payload takes the one in cargo-target-audio.
    if [ "$PLATFORM" = windows ] && [ "$name" = audio ]; then
      cp "$CARGO_TARGET_DIR/release/libmp3lame.dll" "$dest/"
    fi
    size=$(stat -c %s "$dest/$exe" 2>/dev/null || stat -f %z "$dest/$exe")
    echo "$name ok ${secs}s $size bytes" | tee -a "$REPORT"
  else
    echo "$name FAILED ${secs}s (exit $code)" | tee -a "$REPORT"
    echo "::error title=$name failed to build::see the 'build $name' group in this step's log"
    failed=1
  fi
done

echo "total $(( ($(date +%s) - all_start) / 60 )) minutes" | tee -a "$REPORT"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  { echo "### Programs built ($PLATFORM)"; echo '```'; cat "$REPORT"; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
fi
exit $failed
