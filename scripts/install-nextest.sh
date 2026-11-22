#!/usr/bin/env bash
# Install a pinned cargo-nextest release into ~/.cargo/bin for CI. A download,
# not an install action: the repository allows only listed third-party actions.
# A bump changes VERSION and the three digests, from the release's asset list.

set -euo pipefail

VERSION=0.9.151

case "$(uname -s)" in
  Linux)
    target=x86_64-unknown-linux-gnu
    digest=1d4ce302b820c41f4b89c5fb9bce45f0f0c94882c1cf08fc360caf08be6e7df4
    ;;
  Darwin)
    target=universal-apple-darwin
    digest=d1d7d025d4bfc9fb154cd76aea3949bd78534829a0589abefa6e5dd4edb135dd
    ;;
  MINGW* | MSYS*)
    target=x86_64-pc-windows-msvc
    digest=e51bdb4b5883df48f335381b287378edb244318fac65ae0adaf6b9e69298246b
    ;;
  *)
    echo "install-nextest: no pinned build for $(uname -s)" >&2
    exit 1
    ;;
esac

# mktemp, not RUNNER_TEMP: GNU tar reads the `D:` in a Windows path as a host.
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
archive="$dir/cargo-nextest.tar.gz"
curl -fsSL --retry 3 -o "$archive" \
  "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$VERSION/cargo-nextest-$VERSION-$target.tar.gz"
# macOS ships shasum, not sha256sum.
if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$archive" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$archive" | cut -d' ' -f1)
fi
if [ "$actual" != "$digest" ]; then
  echo "install-nextest: $target digest $actual, expected $digest" >&2
  exit 1
fi

bin="$HOME/.cargo/bin"
mkdir -p "$bin"
tar -xzf "$archive" -C "$bin"
cargo nextest --version
