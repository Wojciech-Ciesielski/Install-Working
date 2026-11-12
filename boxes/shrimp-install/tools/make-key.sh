#!/bin/bash
# Nagłówek z kluczem publicznym podpisu manifestu (RSA-2048) dla rufus/src/shrimp/shrimp_key.h.
#   openssl genrsa -aes256 -out shrimp-install-key.pem 2048     # raz; klucza prywatnego nie commituj
#   tools/make-key.sh shrimp-install-key.pem > rufus/src/shrimp/shrimp_key.h
# Podpis manifestu przy wydaniu:
#   openssl dgst -sha256 -sign shrimp-install-key.pem -out manifest.json.sig manifest.json
set -euo pipefail
key="${1:?użycie: make-key.sh klucz-prywatny.pem}"
mod="$(openssl rsa -in "$key" -noout -modulus | cut -d= -f2)"
[ ${#mod} -eq 512 ] || { echo "potrzebny klucz RSA-2048" >&2; exit 1; }
echo "/* Wygenerowane przez tools/make-key.sh. Klucz publiczny podpisu manifestu Shrimp Install. */"
echo "#pragma once"
echo "#define SHRIMP_PUBKEY_SET 1"
echo "static const uint8_t shrimp_pubkey_modulus[257] = {"
printf "\t0x00"
echo "$mod" | fold -w2 | awk '{ printf ", 0x%s", tolower($0) } NR % 12 == 0 { printf "\n\t" }'
echo
echo "};"
