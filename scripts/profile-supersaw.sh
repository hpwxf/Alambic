#!/usr/bin/env bash
# Profiles SUPERSAW on a running module through the probe: one
# `cargo probe stats --fresh`, the loop's and the audio interrupt's main
# stages kept (docs/agents/firmware.md, "Measuring costs").
#
# Usage: scripts/profile-supersaw.sh [seconds per case] [label]
#   SHAPE=0 scripts/profile-supersaw.sh       # SHAPE, 25 % by default
#   INSTANCES=4 scripts/profile-supersaw.sh   # several at once (journal/D-046)
#   DETUNE_INPUT=cv1 scripts/profile-supersaw.sh
#
# The probe assigns CV1 to NOTE and DETUNE_INPUT (CV2 by default) to
# DETUNE, both added (journal/D-045): patch a signal into them, noise will
# do. With INSTANCES above one, the instances run side by side
# (`cargo probe add`), each set and assigned the same; with two, the first
# keeps A, B and SUB on A to C and the second plays on D; with more, each
# plays its A alone, on A, B, C and D in turn. The other instances stop
# first: the profile is of these alone.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
seconds=${1:-10}
label=${2:-}
instances=${INSTANCES:-1}
if [ "$instances" -lt 1 ] || [ "$instances" -gt 4 ]; then
  echo "INSTANCES must be 1 to 4 (MAX_INSTANCES)" >&2
  exit 1
fi
probe() { cargo -q probe "$@" 2>/dev/null; }
# Rows sit two spaces under their section's title (`cargo probe stats`); a
# title is printed only when one of its rows is kept.
keep='tick|update|outputs|render|publish|audio|audio_period|voice|voice_params|frame'
filter() {
  awk -v keep="^  ($keep) " '
    /^\/dev|^us / { print; next }
    /^[^ ]/ { title = $0; next }
    $0 ~ keep { if (title != "") { print title; title = "" } print }
  '
}

# One instance left, at position 0, a fresh SUPERSAW.
for position in 3 2 1; do probe remove "@$position" >/dev/null || true; done
probe focus @0 >/dev/null || true
probe select supersaw >/dev/null
if [ "$instances" -gt 2 ]; then probe route @0 b=off sub=off >/dev/null; fi
for position in $(seq 1 $((instances - 1))); do
  probe add supersaw >/dev/null
  if [ "$instances" -gt 2 ]; then probe route "@$position" b=off sub=off >/dev/null; fi
done
for position in $(seq 0 $((instances - 1))); do
  probe set "@$position" mix 50 >/dev/null
  probe set "@$position" shape "${SHAPE:-25}" >/dev/null
  probe set "@$position" stereo on >/dev/null
  probe assign "@$position" note input=cv1 effect=add >/dev/null
  probe assign "@$position" detune "input=${DETUNE_INPUT:-cv2}" effect=add >/dev/null
done
probe focus @0 >/dev/null

# Long enough for what the sets put on the screen to go: a profile taken
# with it there draws another frame.
sleep 5
echo "=== ${label:+$label, }${instances} x SUPERSAW, SHAPE ${SHAPE:-25} % ($seconds s)"
probe stats --fresh "$seconds" | filter
