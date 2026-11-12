#!/bin/bash
# Klucz publiczny podpisu manifestu (RSA, PEM) dla kompilacji: SHRIMP_MANIFEST_PUBKEY=<plik.pem> cargo build --release
#   openssl genrsa -aes256 -out shrimp-install-key.pem 3072        # raz; klucza prywatnego nie commituj
#   tools/make-key.sh shrimp-install-key.pem > data/manifest-key.pub.pem
# Podpis manifestu przy wydaniu:
#   openssl dgst -sha256 -sign shrimp-install-key.pem -out manifest.json.sig manifest.json
set -euo pipefail
key="${1:?użycie: make-key.sh klucz-prywatny.pem}"
openssl rsa -in "$key" -pubout
