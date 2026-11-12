# Install-Working

Repozytorium robocze **Shrimp Install**: programu dla Windows (własny kod w Rust + WebView2), który pobiera Shrimp, sprawdza sumę SHA-256, zapisuje nośnik (GPT/UEFI: FAT32 `SHRIMP_BOOT` + exFAT `SHRIMP_LIVE`) i przenosi dane z Windows (`shrimp-transfer/shrimp.json`, pliki użytkownika, kopia pendrive'a).

- Kod i opis: [`boxes/shrimp-install/README.md`](boxes/shrimp-install/README.md). Budowa `.exe`: workflow **Shrimp Install** (artefakt `shrimp-install`).
- Kontrakt z Setup w ISO: box `install-schema` (`shrimp.schema.json`). To repozytorium jest jego źródłem; kopia w CoreOS-Working musi być taka sama.
- ISO: Cloudflare R2, adres manifestu w zmiennej repozytorium `SHRIMP_MANIFEST_URL` (Settings → Secrets and variables → Actions → Variables), np. `https://<publiczny adres bucketu>/manifest.json`. Wydania ISO robi CoreOS-Working.
- Wydanie programu: workflow **Release** („Shrimp Install Beta”, `.exe` + `SHA256SUMS`).

Kopie wspólnych boxów (zmieniaj najpierw w źródle): `shrimp-box`, `i18n-core` → CoreOS-Working; `help-links` → Site-Working; `install-schema` → tutaj.

Zasady boxów: [`boxes/README.md`](boxes/README.md), sprawdzenie `cd boxes/shrimp-box && cargo run -- check` (workflow **Boxy**). Żaden PR nie dodaje zrzutów ekranu ani `test-results/` (workflow **Bez zrzutów**).
