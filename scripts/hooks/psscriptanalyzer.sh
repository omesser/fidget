#!/usr/bin/env bash
set -euo pipefail

if ! command -v pwsh > /dev/null 2>&1; then
  echo "skipped - pwsh not installed" >&2
  exit 0
fi

# Skip rather than install: this runs on `git commit`, and installing a module
# into the committer's PowerShell profile is more than a lint should do behind
# their back. ubuntu-latest ships PSScriptAnalyzer, so CI is unaffected.
if ! pwsh -NoProfile -Command 'if (-not (Get-Module -ListAvailable PSScriptAnalyzer)) { exit 1 }' > /dev/null 2>&1; then
  echo "skipped - PSScriptAnalyzer not installed" >&2
  exit 0
fi

# macos-latest pwsh aborts Invoke-ScriptAnalyzer with FileLoadException
# (exit 134 / Abort trap: 6) — a runtime crash, not a finding. ubuntu-latest
# still runs the real lint.
pwsh_runtime_crash() {
  local status="$1"
  local output="$2"
  case "$output" in
    *FileLoadException* | *Microsoft.Management.Infrastructure* | *"Abort trap"* | *"not properly handled"*) return 0 ;;
  esac
  # SIGABRT. EnableExit uses the finding count, so 134 findings would
  # collide; those prints carry Severity/ParseError and are not skipped.
  if [ "$status" -eq 134 ]; then
    case "$output" in
      *ParseError* | *Severity*) return 1 ;;
    esac
    return 0
  fi
  return 1
}

for f in "$@"; do
  set +e
  output=$(pwsh -NoProfile -Command "Invoke-ScriptAnalyzer -Path \"$f\" -Severity Error,ParseError -EnableExit" 2>&1)
  status=$?
  set -e

  if [ "$status" -eq 0 ]; then
    [ -n "$output" ] && printf '%s\n' "$output"
    continue
  fi

  if pwsh_runtime_crash "$status" "$output"; then
    echo "skipped - pwsh crashed running PSScriptAnalyzer (exit ${status}); Ubuntu CI still runs the real lint" >&2
    [ -n "$output" ] && printf '%s\n' "$output" >&2
    exit 0
  fi

  [ -n "$output" ] && printf '%s\n' "$output"
  exit "$status"
done
