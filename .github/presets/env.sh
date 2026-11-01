#!/bin/sh
# Load RUSTDESK_PRESET_* environment variables from a preset JSON file.
#
# Usage (from any build shell):
#   . "$(dirname "$0")/env.sh" .github/presets/full-client.json
#
# build.rs listens for these variables and reruns when they change, so the
# preset is baked into every subsequent cargo build of this shell.
#
# Only non-empty sections are exported; an empty hard_settings stays unset so
# runtime defaults apply.

PRESET_FILE="${1:?usage: source env.sh <preset.json>}"
[ -f "$PRESET_FILE" ] || { echo "env.sh: preset file not found: $PRESET_FILE" >&2; return 1 2>/dev/null || exit 1; }

_export_section() {
  python3 - "$PRESET_FILE" "$1" <<'EOF'
import json, sys
section = sys.argv[2]
data = json.load(open(sys.argv[1])).get(section, {})
if data:
    print(json.dumps(data, separators=(",", ":")))
EOF
}

HARD=$(_export_section hard_settings)
OVERRIDE=$(_export_section override_settings)
BUILTIN=$(_export_section builtin_settings)

[ -n "$HARD" ] && export RUSTDESK_PRESET_HARD_SETTINGS="$HARD"
[ -n "$OVERRIDE" ] && export RUSTDESK_PRESET_OVERRIDE_SETTINGS="$OVERRIDE"
[ -n "$BUILTIN" ] && export RUSTDESK_PRESET_BUILTIN_SETTINGS="$BUILTIN"
