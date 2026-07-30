#!/usr/bin/env bash
set -euo pipefail

binary="${1:-}"
if [[ -z "$binary" ]]; then
  echo "tauri-dev-sign-runner: missing binary path" >&2
  exit 64
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/.." && pwd)"
env_file="$project_root/.env.local"

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  exec "$@"
fi

identifier="${TFROBOT_DEV_CODESIGN_IDENTIFIER:-com.tfrobot.client.dev}"
identity="${TFROBOT_DEV_CODESIGN_IDENTITY:-}"

if [[ -z "$identity" ]]; then
  identity="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' \
      | head -n 1
  )"
fi

if [[ -z "$identity" ]]; then
  cat >&2 <<'EOF'
tauri-dev-sign-runner: no valid Apple Development signing identity found.
Continuing without re-signing. Keychain access may fail on macOS dev builds.

Create one in Xcode:
  Xcode > Settings > Accounts > Manage Certificates... > + > Apple Development

Or set TFROBOT_DEV_CODESIGN_IDENTITY to a valid codesigning identity.
EOF
  exec "$@"
fi

codesign --force --sign "$identity" --identifier "$identifier" "$binary" >/dev/null
exec "$@"
