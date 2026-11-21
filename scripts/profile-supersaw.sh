#!/usr/bin/env bash
# Profiles SUPERSAW on a running module through the probe: one
# `cargo probe stats --fresh` per arithmetic, the loop's and the audio
# interrupt's main stages kept (docs/agents/firmware.md, "Measuring costs").
#
# Usage: scripts/profile-supersaw.sh [seconds per case] [label]
#   SHAPE=0 scripts/profile-supersaw.sh      # SHAPE, 25 % by default
#   CALCS=f32 scripts/profile-supersaw.sh    # the arithmetics, "int f32" by default
#
# The probe sets the applet's settings, not its CV assignments: patch and
# assign the inputs on the panel first (NOTE takes CV1 at start-up).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
seconds=${1:-10}
label=${2:-}
probe() { cargo -q probe "$@" 2>/dev/null; }
keep='^(/dev|us |tick|update|outputs|render|publish|audio|voice|voice_params|frame) '

probe select supersaw >/dev/null
probe set supersaw mix 50 >/dev/null
probe set supersaw shape "${SHAPE:-25}" >/dev/null
probe set supersaw stereo on >/dev/null

for calc in ${CALCS:-int f32}; do
  probe set supersaw calc "$calc" >/dev/null
  sleep 1
  echo "=== ${label:+$label, }CALC $calc, SHAPE ${SHAPE:-25} % ($seconds s)"
  probe stats --fresh "$seconds" | grep -E "$keep"
done
