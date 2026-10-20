# shellcheck shell=bash
# Shared by scripts/sync-mattpocock.sh and scripts/sync-pstack.sh, which each
# define `EXCLUDED` as `name|reason` lines. Reading it only here keeps the two
# syncs from drifting apart.

excluded_names() {
  printf '%s\n' "$EXCLUDED" | cut -d'|' -f1 | sort
}

# The `excluded` object body for the lock file.
excluded_json() {
  local first=1 name reason
  while IFS='|' read -r name reason; do
    [ -n "$name" ] || continue
    [ "$first" -eq 1 ] || echo ','
    printf '    "%s": "%s"' "$name" "$reason"
    first=0
  done <<< "$EXCLUDED"
  echo
}
