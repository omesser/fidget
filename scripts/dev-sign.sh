#!/usr/bin/env bash
# Run after each build. A stable self-signed identity stops Keychain prompts; an
# ad-hoc cdhash changes every build. `-A` is for development only. A key saved
# before the first signed run must be re-saved.

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != Darwin ]]; then
  echo "dev-sign: macOS only; nothing to sign on $(uname -s)" >&2
  exit 0
fi

IDENTITY="fidget dev signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
TARGET="${1:-target/debug/fidget}"

if [[ ! -e $TARGET ]]; then
  echo "dev-sign: no $TARGET — build it first" >&2
  exit 1
fi

# Identities, not certificates: one whose private key never arrived cannot sign.
# `-v` filters to trusted identities, and this one is trusted by nothing, which
# codesign does not mind.
if ! security find-identity "$KEYCHAIN" | grep -q "\"$IDENTITY\""; then
  echo "dev-sign: creating the '$IDENTITY' certificate in the login keychain"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" \
    -subj "/CN=$IDENTITY/O=fidget" \
    -addext "basicConstraints=critical,CA:false" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning" 2> /dev/null
  # 3DES rather than the OpenSSL 3 default: the macOS importer cannot read a
  # PKCS#12 encrypted with AES.
  openssl pkcs12 -export -name "$IDENTITY" \
    -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -out "$tmp/id.p12" \
    -passout pass:fidget \
    -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
  security import "$tmp/id.p12" -k "$KEYCHAIN" -P fidget -T /usr/bin/codesign -A
fi

# A bundle carries the frameworks Tauri links; a raw binary carries nothing.
deep=()
[[ $TARGET == *.app ]] && deep=(--deep)

codesign --force "${deep[@]}" --sign "$IDENTITY" "$TARGET"
# Reporting only: a successful signature must not fail on codesign's phrasing.
codesign -dvvv "$TARGET" 2>&1 | grep -E '^(Authority|CDHash)=' || true
