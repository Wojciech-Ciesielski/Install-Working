# Wspólny format tłumaczeń (box i18n-core)

Teksty każdego boxa leżą w jego katalogu `i18n/`:

- `_keys.tsv`: `id⇥klucz⇥tekst angielski` (jedna linia na tekst, `#` = komentarz),
- `<znacznik>.txt`: `id⇥tłumaczenie`; brak linii = angielski (albo język zapasowy `text` z `langs.ts`).

Ten box daje wszystkim jedną listę języków i narzędzia:

| Plik | Do czego |
|---|---|
| `langs.ts` | języki Windows i GNOME: znaczniki, endonimy, pismo, język zapasowy, locale glibc (jedno źródło prawdy, czytają go też `build.rs` boxów Rust) |
| `core.ts` | parser `_keys.tsv` i `<znacznik>.txt`, wybór języka, placeholdery (Setup, Shrimp Install) |
| `check.ts` | sprawdzanie plików tłumaczeń w testach (błędy budowy, pokrycie) |
| `PRZEGLAD.md` | przegląd jakości tłumaczeń |

Boxy z tekstami: `setup`, `shrimp-install`, `shrimp-apps`, `panko`, `cocktail`, `web-apps`, `grub-theme`.
Aplikacje GTK (Rust) wbudowują swoje teksty przez `shrimp-i18n` (`build.rs` → `shrimp_i18n::codegen::write("i18n")`).
