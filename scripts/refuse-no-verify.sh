#!/usr/bin/env bash
# PreToolUse hook: refuses a `git commit` or `git push` that skips the git hooks.
# Claude Code sends the command in `tool_input`, Grok Build in `toolInput`; exit 2
# denies in both. Bash 3.2 and no jq, so stock macOS and Git Bash run it.

json_string='"command"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)"'
[[ $(cat) =~ $json_string ]] || exit 0
command=${BASH_REMATCH[1]}
command=$(printf '%b' "${command//'\"'/\"}")

# Quoted text becomes one placeholder word so a message that names the flag
# passes. Newlines turn into `;` first so a quote spanning lines is caught whole.
strip_quotes=$'s/"([^"\\\\]|\\\\.)*"|\'[^\']*\'/Q/g'
segments=$(printf '%s' "$command" | tr '\n' ';' | sed -E "$strip_quotes" | tr ';&|()`' '[\n*]')

hook_skip() {
  local i=0 sub arg flags
  while [[ ${words[i]} == [A-Za-z_]*=* ]]; do i=$((i + 1)); done
  [[ ${words[i]##*/} == git ]] || return
  for ((i++; ; i++)); do
    case ${words[i]} in
      -C | -c) i=$((i + 1)) ;;
      -*) ;;
      *) break ;;
    esac
  done
  sub=${words[i]}
  [[ $sub == commit || $sub == push ]] || return
  for arg in "${words[@]:i+1}"; do
    [[ $arg == -- ]] && return
    # git takes any unambiguous prefix; `--no-ver` would also match `--no-verbose`.
    if [[ ${#arg} -ge 9 && --no-verify == "$arg"* ]]; then
      echo "git $sub --no-verify"
      return
    fi
    [[ $sub == commit && $arg == -[!-]* ]] || continue
    # Letters after one that takes a value, as in `-mnope`, are that value.
    flags=${arg#-}
    flags=${flags%%[mFcCtSu]*}
    if [[ $flags == *n* ]]; then
      echo "git commit -n"
      return
    fi
  done
}

while read -r -a words; do
  hit=$(hook_skip)
  if [[ -n $hit ]]; then
    echo "\`$hit\` skips the git hooks, and docs/agents/writing.md forbids it. Run \`pre-commit run --files <touched>\`, fix what it reports, and commit without the flag." >&2
    exit 2
  fi
done <<< "$segments"
