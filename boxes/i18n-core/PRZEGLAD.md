# Tłumaczenia do przeglądu

Wszystkie tłumaczenia w `i18n/` (setup, install, webapps, grub) są maszynowe i nie przeszły
przeglądu native speakerów. Nie tłumaczymy ich ponownie: czekają na ludzi, którzy znają język.

## Najpierw (najsłabsze, z przeglądu repo 2026-11-08)

| Znacznik | Język | Uwagi |
|---|---|---|
| `kw` | kornijski | mało źródeł, dużo zgadywania |
| `mai` | maithili | tekstowy język zapasowy: hindi (`text` w `langs.ts`) |
| `os` | osetyjski | tekstowy język zapasowy: rosyjski |
| `dz` | dzongkha | brak czcionki tybetańskiej w Leap (workflow Fonts probe), więc tekst może się nie wyświetlać |

## Potem

`nso`, `tn`, `rw`, `ti`, `ig`, `yo`, `ha`, `wa`, `crh`, `an`, `br`, `fur`, `kab`.

Transliteracje (nie osobne tłumaczenia): `be-latn`, `sr-latn`, `uz-cyrl`.
Bez tłumaczenia (angielski albo język zapasowy): Setup `ab`, `mjw`, `sat`; Install `wo`, `chr`, `iu`, `quz`, `quc`.

## Jak poprawić

Plik `i18n/<przestrzeń>/<znacznik>.txt`, wiersz `id⇥tekst`. Id i placeholdery (`{name}`) bez zmian.
Po zmianach w `grub` uruchom `tools/render-grub-i18n.js`.
