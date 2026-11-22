#!/usr/bin/env bash
# Profiles SUPERSAW on a running module through the probe: one
# `cargo probe stats --fresh`, the loop's and the audio interrupt's main
# stages kept (docs/agents/firmware.md, "Measuring costs").
#
# Usage: scripts/profile-supersaw.sh [seconds per case] [label]
#   SHAPE=0 scripts/profile-supersaw.sh      # SHAPE, 25 % by default
#
# The probe assigns CV1 to NOTE and CV2 to DETUNE, both added
# (journal/D-045): patch a signal into them, noise will do.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
seconds=${1:-10}
label=${2:-}
probe() { cargo -q probe "$@" 2>/dev/null; }
# Rows sit two spaces under their section's title (`cargo probe stats`); a
# title is printed only when one of its rows is kept.
keep='tick|update|outputs|render|publish|audio|voice|voice_params|frame'
filter() {
  awk -v keep="^  ($keep) " '
    /^\/dev|^us / { print; next }
    /^[^ ]/ { title = $0; next }
    $0 ~ keep { if (title != "") { print title; title = "" } print }
  '
}

probe select supersaw >/dev/null
probe set supersaw mix 50 >/dev/null
probe set supersaw shape "${SHAPE:-25}" >/dev/null
probe set supersaw stereo on >/dev/null
probe assign supersaw note input=cv1 effect=add >/dev/null
probe assign supersaw detune input=cv2 effect=add >/dev/null

# Long enough for what the sets put on the screen to go: a profile taken
# with it there draws another frame.
sleep 5
echo "=== ${label:+$label, }SHAPE ${SHAPE:-25} % ($seconds s)"
probe stats --fresh "$seconds" | filter
