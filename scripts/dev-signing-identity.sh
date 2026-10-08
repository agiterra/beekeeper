#!/usr/bin/env bash
# dev-signing-identity.sh — create the local code-signing identity that
# `scripts/app-from.sh` signs Beekeeper Dev.app with.
#
# Why (ledger 368): an ad-hoc signature changes with every build, and the
# Keychain's "Always Allow" is recorded against the signature. So each install
# asked for the login password again, and the app sat with no window behind
# the prompt until someone answered it. A certificate that stays the same gives
# every build the same designated requirement
# (`identifier … and certificate leaf = H"…"`), so one "Always Allow" lasts
# across rebuilds.
#
# The certificate is self-signed, valid for code signing only, and exists only
# in this user's login keychain. It is not trusted for anything else and does
# not need to be: codesign signs with it, and the Keychain matches against it.
# Nothing here is a Developer ID, and Gatekeeper still treats the bundle as
# locally built.
#
# Idempotent: if an identity of that name already exists, it is reported and
# left alone. To remove it: Keychain Access → login → My Certificates → delete
# "Beekeeper Dev Local Signing" (the certificate and its private key).
#
# Usage: scripts/dev-signing-identity.sh   (or `just dev-signing-identity`)
set -euo pipefail

NAME="${BEEKEEPER_DEV_SIGNING_IDENTITY:-Beekeeper Dev Local Signing}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
OPENSSL=/usr/bin/openssl

if security find-identity -p codesigning "$KEYCHAIN" | grep -qF "\"$NAME\""; then
  echo "==> signing identity \"$NAME\" already exists in the login keychain; nothing to do"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cat >"$WORK/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -config "$WORK/cert.cnf" -keyout "$WORK/key.pem" -out "$WORK/cert.pem" 2>/dev/null
# A throwaway passphrase for the transport file only: `security import` refuses
# an empty one on some macOS releases. The file is deleted on exit.
PASS="$("$OPENSSL" rand -hex 16)"
"$OPENSSL" pkcs12 -export -inkey "$WORK/key.pem" -in "$WORK/cert.pem" \
  -name "$NAME" -passout "pass:$PASS" -out "$WORK/identity.p12"
# -T: codesign may use the private key without asking. macOS can still ask
# once ("codesign wants to sign using key …"); answer Always Allow.
security import "$WORK/identity.p12" -k "$KEYCHAIN" -P "$PASS" \
  -T /usr/bin/codesign >/dev/null

if ! security find-identity -p codesigning "$KEYCHAIN" | grep -qF "\"$NAME\""; then
  echo "imported, but codesign does not list \"$NAME\" as a signing identity" >&2
  exit 1
fi
echo "==> created signing identity \"$NAME\" in the login keychain"
echo "    the next \`just app-from\` signs with it; the Keychain asks once more, then not again"
