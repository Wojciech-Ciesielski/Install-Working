# Kontrakt Shrimp Install ↔ Shrimp Setup

`shrimp-transfer/shrimp.json` na nośniku to jedyny kanał, którym dane z Windows trafiają do Setup.
Ten katalog trzyma jego opis i przykład, które sprawdzają obie strony:

| Plik | Kto go używa |
|---|---|
| `shrimp.schema.json` | CI Shrimp Install: wynik `shrimp-install.exe --collect` musi przejść walidację (PowerShell `Test-Json`) |
| `shrimp.example.json` | testy Setup: Rust (`boxes/setup/shell/src/transfer.rs`, `system.rs`) i TypeScript (`boxes/setup/web/src/logic.test.ts`) |

Zmiana pola = zmiana w `collect.c`/`transfer.c`, w schemacie, w przykładzie i po stronie Setup w jednym PR.
Nowa, niezgodna wersja formatu podbija `version`; Setup czyta tylko wersję 1 i przy innej pomija podpowiedzi
(pliki i tak przywraca, bo lista `files` jest wymagana w każdej wersji).
