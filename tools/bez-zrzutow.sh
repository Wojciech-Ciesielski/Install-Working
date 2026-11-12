#!/bin/bash
# Pilnuje zasady właściciela: żaden PR nie dodaje zrzutów ekranu ani wyników testów.
# Wyniki i dzienniki CI są artefaktami przebiegów GitHub Actions, nigdy nie trafiają do repo.
#
# Użycie: tools/bez-zrzutow.sh [BAZA]   (domyślnie origin/main; sprawdza pliki dodane i zmienione od BAZA)
# Obrazy wolno trzymać tylko tam, gdzie są częścią produktu (lista DOZWOLONE poniżej).
set -euo pipefail
base="${1:-origin/main}"
DOZWOLONE='^boxes/([^/]+/(root|data)/|site/(assets/|favicon)|setup/web/src/assets/|shrimp-install/ui/src/|shrimp-install/rufus/(res|src/shrimp)/)'

bad=0
while IFS= read -r f; do
  [ -n "$f" ] || continue
  lower="$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')"
  if [[ "$lower" == test-results/* ]]; then
    echo "::error file=$f::wyniki testów nie należą do repo (zapisuj je jako artefakty CI)"; bad=1
  elif [[ "$lower" =~ (^|/)(zrzuty?|screenshots?)(/|$) || "$lower" =~ (zrzut|screenshot)[^/]*\.(png|jpe?g|gif|webp|bmp)$ ]]; then
    echo "::error file=$f::zrzut ekranu w PR (zasada: żadnych zrzutów w repo)"; bad=1
  elif [[ "$lower" =~ \.(png|jpe?g|gif|webp|bmp)$ ]] && ! [[ "$f" =~ $DOZWOLONE ]]; then
    echo "::error file=$f::obraz poza katalogami zasobów produktu (jeśli to zasób, dopisz katalog do DOZWOLONE w tools/bez-zrzutow.sh)"; bad=1
  fi
done < <(git diff --name-only --diff-filter=AMR "$base"...HEAD)

if [ "$bad" = 1 ]; then
  echo "PR zawiera zrzuty ekranu lub wyniki testów. Usuń je z gałęzi; wyniki CI zapisuj jako artefakty przebiegów." >&2
  exit 1
fi
echo "ok: brak zrzutów i wyników testów"
