#!/usr/bin/env bash
set -euo pipefail

# The repo keeps no node_modules, but "no undeclared global" still needs a real
# name resolver, which the hand-rolled regex checkers did not have (#737,
# #1584). So oxlint: one standalone binary, fetched as a PyPI wheel whose
# sha256 below is the registry's own published digest, checkable at
# https://pypi.org/pypi/oxlint/<version>/json.

version=1.84.0
exe=oxlint

# Bump: re-read that JSON for the new version and refresh every row. An
# unsupported platform fails loudly rather than lint an unpinned binary.
case "$(uname -s):$(uname -m)" in
  Darwin:arm64)
    wheel=oxlint-${version}-py3-none-macosx_11_0_arm64.whl
    url=https://files.pythonhosted.org/packages/67/59/bb43a329a6d088acb94c38a367346d26b88098f926fc5047c8f0b388ead4/${wheel}
    hash=a98bd5f6bf6cd1fb56ef224aaba614fef9830877d2864bfad70dce9cf2301e49
    ;;
  Darwin:x86_64)
    wheel=oxlint-${version}-py3-none-macosx_10_12_x86_64.whl
    url=https://files.pythonhosted.org/packages/ad/66/1f22f882fbf1395460d68bda25de6eb915a79aea45fc0250b9953e7927a7/${wheel}
    hash=de8a5ebaa157ce58a6ca3343fe839d28d08497e6512bbfddd020f175a40f5690
    ;;
  Linux:x86_64)
    wheel=oxlint-${version}-py3-none-manylinux_2_18_x86_64.whl
    url=https://files.pythonhosted.org/packages/1c/37/c97eaf0b996dba188ba38df3e304465eda15df3caef3525506b466915cd4/${wheel}
    hash=0814a26c001d007b0e81819b832c2e1801b64428fc349d94999781aaab6e6203
    ;;
  Linux:aarch64)
    wheel=oxlint-${version}-py3-none-manylinux_2_18_aarch64.whl
    url=https://files.pythonhosted.org/packages/8d/41/86f2de4fd03067bb63423d3f59d5c76a3bfd6e5575c2a99990942ec141fb/${wheel}
    hash=05ed98b1f2413df2cb14e7e5363bd4af69f5ff4e797901e18a939171da513a6e
    ;;
  MSYS*:x86_64 | MINGW*:x86_64)
    wheel=oxlint-${version}-py3-none-win_amd64.whl
    url=https://files.pythonhosted.org/packages/65/b3/b62f59dd14cf428b3c5ffbd18ebef684df32bbb7ea7c576777941e4ac014/${wheel}
    hash=ab44ce2fb76f714b35c945a05236899219f0bf58997fc99871d6d8446e795bc8
    exe=oxlint.exe
    ;;
  *)
    echo "no pinned oxlint wheel for $(uname -s):$(uname -m)" >&2
    exit 1
    ;;
esac

cache="${XDG_CACHE_HOME:-$HOME/.cache}/oxlint/${version}"
wheel_path="${cache}/${wheel}"
installed="${cache}/${wheel%.whl}"
binary="${installed}/oxlint-${version}.data/scripts/${exe}"

digest() {
  if command -v shasum > /dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    sha256sum "$1" | cut -d' ' -f1
  fi
}

if [ -f "$wheel_path" ] && [ "$(digest "$wheel_path")" != "$hash" ]; then
  # A wheel whose bytes changed under the same name is not the pinned one;
  # the binary extracted from it cannot be trusted either.
  rm -f "$wheel_path"
  rm -rf "$installed"
fi

if [ ! -f "$wheel_path" ]; then
  if ! command -v curl > /dev/null 2>&1; then
    echo "the pinned oxlint wheel is not cached and there is no curl to fetch it" >&2
    exit 1
  fi
  mkdir -p "$cache"
  if ! curl -fsSL -o "${wheel_path}.part" "$url" || [ "$(digest "${wheel_path}.part")" != "$hash" ]; then
    rm -f "${wheel_path}.part"
    echo "the oxlint wheel did not fetch, or its sha256 differs from the pinned $hash" >&2
    exit 1
  fi
  mv "${wheel_path}.part" "$wheel_path"
fi

if [ ! -f "$binary" ]; then
  # unzip-or-python3: every context that runs pre-commit has Python, and
  # git-bash and the runner images ship unzip too.
  if command -v unzip > /dev/null 2>&1; then
    unzip -q -o "$wheel_path" "oxlint-${version}.data/scripts/$exe" -d "$installed"
  else
    python3 -m zipfile -e "$wheel_path" "$installed"
  fi
  chmod +x "$binary"
fi

exec "$binary" -A all -D no-undef -D no-global-assign "$@"
