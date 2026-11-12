# Shrimp Install

Program dla Windows 10/11, który przygotowuje nośnik z Shrimp, zachowuje pliki użytkownika, zapisuje listę programów i ustawień Windows dla Setup i uruchamia komputer z nośnika bez wchodzenia do UEFI. Wygląd pochodzi z `shrimp-handoff` (`mockups/shrimp-install.html`, `specs/shrimp-install.md`).

Od Bety 5 to **własny kod w Rust** (ok. 2,8 tys. wierszy razem z testami zamiast forka Rufusa 4.16 w C). Fork Rufusa jest w historii gita (ostatni commit z nim: `2435584`), gdyby trzeba było do niego wrócić. Interfejs (`ui/`, TypeScript w WebView2) i most do programu się nie zmieniły.

## Jak to działa

```
Start → Nośnik → Pliki → Przygotuj nośnik
          │        │            │
          │        │            ├─ 1. Pobieranie Shrimp   manifest → ISO → SHA-256 (+ podpis manifestu)
          │        │            ├─ 2. Zapis na nośniku    kopia zawartości nośnika (SHA-256) → GPT: FAT32 + exFAT → pliki obrazu
          │        │            └─ 3. Kopiowanie plików   shrimp-backup/, shrimp-transfer/files/…, shrimp.json
          │        └─ foldery profilu albo wybrane pliki i foldery
          └─ pendrive'y, karty SD, dyski USB (Get-Disk: magistrala USB/SD/MMC, bez dysku z Windows; ≥ 16 GB)
Gotowe → Uruchom ponownie: wpis UEFI „Shrimp Install” + BootNext (jednorazowo), wyłączone szybkie uruchamianie, restart
```

### Układ nośnika: FAT32 + exFAT

| Partycja (GPT, typ: dane podstawowe) | System plików | Zawartość |
|---|---|---|
| 1. `SHRIMP_BOOT`, ≥ 1 GiB | FAT32 | wszystko z obrazu poza `LiveOS/`: `EFI/BOOT/bootx64.efi` (shim), `boot/` (GRUB, jądro, initrd), `boot/grub2/shrimp-lang.cfg` |
| 2. `SHRIMP_LIVE`, reszta nośnika | exFAT | `LiveOS/squashfs.img`, `shrimp-transfer/`, `shrimp-backup/` |

Firmware UEFI startuje z FAT32 (`\EFI\BOOT\BOOTX64.EFI`), GRUB znajduje swoje pliki na tej samej partycji, a system live szuka nośnika po etykiecie (`root=live:CDLABEL=SHRIMP_LIVE`) i montuje partycję exFAT (moduł `exfat` jest w initrd obrazu, CoreOS-Working). Setup widzi dane w `/run/overlay/live` jak dotąd. Dzięki exFAT ani obraz, ani pliki z Windows nie mają limitu 4 GiB i nic nie jest dzielone. Własny formater FAT32 nie jest potrzebny: partycja startowa jest mała, a Windows formatuje FAT32 do 32 GB.

Przygotowanie robi Windows (moduł Storage, `src/win/disk.rs`): `Clear-Disk`, `Initialize-Disk -PartitionStyle GPT`, `New-Partition`, `Format-Volume`. Obraz ISO jest montowany tylko do odczytu (`Mount-DiskImage`) i kopiowany plik po pliku. Dysk z Windows (`IsBoot`/`IsSystem`) jest odrzucany w skrypcie formatowania niezależnie od interfejsu.

| Część | Gdzie |
|---|---|
| Interfejs (ekrany z makiety, tryb podglądu) | `ui/src/app.ts`, `ui/src/mockup.css`, `ui/src/screens.html` |
| Most interfejs ↔ program, przebieg | `ui/src/backend/host.ts`, `src/win/app.rs`, okno: `src/win/window.rs` (wry + tao) |
| Nośniki, formatowanie, obraz | `src/win/disk.rs` (PowerShell: `src/win/ps.rs`) |
| Pobieranie i weryfikacja ISO | `src/win/fetch.rs`, `src/manifest.rs` |
| Kopia zawartości nośnika | `src/backup.rs` |
| Kopia plików, `shrimp.json` | `src/transfer.rs`, `src/win/collect.rs` |
| BootNext, szybkie uruchamianie, restart | `src/win/boot.rs` |

## Most interfejs ↔ program

Interfejs wysyła tekst `id⇥wywołanie⇥argumenty…` (ścieżki Windows nie mogą zawierać tabulatora), program odpowiada JSON `{ "id", "ok", "result" | "error" }`.

| Wywołanie | Argumenty | Wynik |
|---|---|---|
| `info` | | `{ bitlocker, shrimpSize, minSize, folders: [{ key, path, size }], helpUrl? }` |
| `devices` | | `[{ id, name, kind, size }]` |
| `pick` | `folder` \| `file` | `[{ path, name, kind, size }]` (okno wyboru Windows) |
| `start` | nośnik, klucze folderów po przecinku, ścieżki… | od razu `null`, dalej zdarzenia |
| `cancel` | | `null` po zatrzymaniu |
| `reboot` | | `null` albo błąd z opisem dla użytkownika |
| `open` | adres `https://…` | bez odpowiedzi; program otwiera stronę w przeglądarce (odrzuca wszystko poza https i zwykłymi znakami adresu) |
| `later`, `close`, `minimize`, `drag` | | bez odpowiedzi |

Zdarzenia: `devices`, `folders` (rozmiary liczone w tle), `bitlocker`, `progress { phase 1–3, done, total, speed, step? }` (`step`: `verify` — suma obrazu pobranego wcześniej, `prepare` — analiza obrazu i formatowanie, `backup` — kopia zawartości nośnika; bez `step` główna praca fazy), `image { size, version }` (prawdziwy rozmiar obrazu po przeczytaniu manifestu), `fail { phase, kind, message? }` (`offline`, `verify`, `space`, `write`, `device`, `busy`, `copy`, `other`), `done`, `closeRequested`.

Podgląd ekranów błędów bez Windows: `cd ui && npm run dev`, potem `?screen=brak-internetu`, `?screen=nosnik-zajety`.

### Strony pomocy

Ikony ⓘ (np. przy wyborze nośnika i plików) otwierają stronę pomocy w przeglądarce, gdy znamy jej adres. Adresy i tematy są w [`boxes/help-links/help.json`](../help-links/help.json) (wspólne z Setup): wpisz stronę w `base` (np. `https://…/help`), a ikona otworzy `base/<ścieżka tematu>?lang=<język interfejsu>&from=install`. Przy pustym `base` ikony zostają samą podpowiedzią. Adres z manifestu wydania (`helpUrl` w odpowiedzi `info`) ma pierwszeństwo. Podstrony (`download-iso`, `copy-files`, `usb-drive`, `bitlocker`, `no-internet`, `boot-from-usb`, `nvidia-driver`) leżą w `boxes/site/help/<ścieżka>.html`, po polsku i angielsku.

## shrimp.json

Opis formalny (JSON Schema) i przykład sprawdzany po obu stronach: [`boxes/install-schema`](../install-schema/README.md).

Na nośniku w `shrimp-transfer/shrimp.json` na partycji exFAT `SHRIMP_LIVE` (na żywym systemie `/run/overlay/live/shrimp-transfer/`). Setup czyta go w obu częściach: przy instalacji podpowiada imię, język, strefę i klawiaturę, a po restarcie przywraca pliki i proponuje odpowiedniki programów.

```jsonc
{
  "version": 1,
  "created": "2026-11-05T19:00:00Z",
  "installer": "Shrimp Install 0.1.0",
  "image": { "version": "Beta 3.5", "sha256": "…" },
  "windows": { "version": "10.0.26100", "firmware": "uefi", "secureBoot": true, "bitlocker": false },
  "user": { "name": "jan", "fullName": "Jan Kowalski" },
  "locale": {
    "format": "pl-PL",            // format liczb i dat
    "language": "pl-PL",          // język interfejsu Windows
    "region": "PL",
    "timezone": "Europe/Warsaw",  // IANA (ICU w Windows 10 1903+), może być null
    "windowsTimezone": "Central European Standard Time",
    "keyboards": ["00000415"]     // KLID w kolejności z Windows, pierwszy domyślny
  },
  "apps": [{ "name": "Mozilla Firefox (x64 pl)", "publisher": "Mozilla", "version": "132.0" }],
  "storeApps": ["SpotifyAB.SpotifyMusic"],          // pakiety Microsoft Store (bez składników systemu)
  "files": [{ "key": "documents", "path": "files/documents", "source": "C:\\Users\\jan\\Documents", "bytes": 3435973836 }],
  "split": [],                                      // zawsze puste: exFAT mieści pliki > 4 GB (Setup nadal skleja części ze starszych nośników)
  "skipped": [{ "source": "C:\\Users\\jan\\OneDrive\\x.docx", "reason": "cloud" }]  // cloud, locked, read, missing
}
```

Klucze folderów: `documents`, `pictures`, `desktop`, `music`, `videos`, `downloads` (Moje foldery) i `other` (Wybiorę sam, `files/other/<nazwa>`). Dowiązania i pliki tylko w chmurze (OneDrive „na żądanie”) są pomijane, żeby nie pobierać ich przy kopiowaniu.

Uwaga: dysk z BitLockerem (`windows.bitlocker`, zdarzenie i alert na ekranie Start) uniemożliwia instalację obok Windows, bo Setup nie zmniejszy zaszyfrowanej partycji. „Zamiast Windows” działa.

## Manifest pobierania

`https://…/manifest.json` (adres w `SHRIMP_MANIFEST_URL` przy kompilacji albo `--manifest`; opcjonalne `helpUrl` = strona pomocy):

```json
{ "version": "Beta 3.5", "url": "https://…/Shrimp.x86_64-0.1.0.iso", "size": 2087512064, "sha256": "…" }
```

Adres manifestu: zmienna repozytorium `SHRIMP_MANIFEST_URL` (Install-Working), wkompilowana przy budowie (`build.rs`). Manifest i ISO leżą w Cloudflare R2: wysyła je workflow **Release** w CoreOS-Working (`<publiczny adres bucketu>/manifest.json` = ostatnie wydanie, obraz pod `<tag>/`). R2 nie ma limitu 2 GiB na plik, a `LiveOS/squashfs.img` leży na exFAT, więc obraz może przekroczyć 4 GiB.

Wydanie programu: workflow **Release** tutaj (`.exe` + `SHA256SUMS`).

Obok `manifest.json.sig`: `openssl dgst -sha256 -sign klucz.pem -out manifest.json.sig manifest.json`. Klucz publiczny (PEM z `tools/make-key.sh`) trafia do programu przez `SHRIMP_MANIFEST_PUBKEY=<plik>` przy kompilacji. Bez klucza (obecny stan) podpis nie jest sprawdzany, a zaufanie do manifestu daje HTTPS. Suma SHA-256 obrazu jest sprawdzana zawsze. Obraz trafia do `%LOCALAPPDATA%\Shrimp Install\` i jest usuwany po zapisie.

Pobieranie przetrwa zerwane połączenie: plik `shrimp-<wersja>.iso.<16 znaków sumy>.part` zostaje na dysku, a kolejna próba
(automatycznie do 4 razy, z przerwą 2, 4, 8 s, albo po ponownym uruchomieniu programu) dociąga resztę nagłówkiem `Range`.
Serwer, który nie wznawia (HTTP 200 zamiast 206), dostaje pobieranie od początku. Suma jest liczona z całego pliku na dysku,
a części innych wydań są usuwane. Błędy serwera (404) nie są ponawiane.

## Budowa

```sh
cd ui && npm ci && npm run build && cd ..        # interfejs → ui/dist/index.html (wkompilowany w program)
cargo build --release                            # Windows (MSVC); ikona i manifest administratora z data/
cargo test                                       # na każdym systemie: kopia nośnika, opis plików, manifest, języki
```

`SHRIMP_MANIFEST_URL` (adres manifestu) i opcjonalnie `SHRIMP_MANIFEST_PUBKEY` (plik PEM) są czytane przy kompilacji. Komunikaty sprzed startu interfejsu (brak WebView2) pochodzą z kluczy `native.*` w `i18n/` (tabela generowana w `build.rs`).

CI (`.github/workflows/shrimp-install.yml`): testy interfejsu i Rust na Linuksie (z `clippy` dla celu Windows), potem na Windows: `--collect` (dane i zgodność ze schematem), `--backup-test`, pełny zapis ISO z ostatniego wydania na wirtualny dysk 32 GB z plikiem użytkownika (`--auto-disk`, potem sprawdza GPT, FAT32 `SHRIMP_BOOT`, exFAT `SHRIMP_LIVE`, wielkość liter nazw, `shrimp-backup` i `shrimp.json`) i okno w trybie `--demo`.

Dziennik każdego uruchomienia: `%LOCALAPPDATA%\Shrimp Install\shrimp-install.log` (poprzedni: `shrimp-install.old.log`). Przy zgłoszeniu błędu poproś o ten plik.

Argumenty: `--demo` (dane z makiety, nic nie zapisuje), `--screen <ekran> --theme light|dark` (podgląd), `--lang <język>`, `--source plik.iso` (bez pobierania), `--manifest URL`, `--collect plik.json` (tylko dane z Windows, bez okna), `--backup-test ŹRÓDŁO CEL KOPIA` (działa też na Linuksie), `--auto-disk NUMER` (test: od razu cały przebieg na dysku o tym numerze z Zarządzania dyskami, bez folderów użytkownika; kod wyjścia 0 po zakończeniu, 3 po błędzie; tylko dla dysków testowych, bo kasuje wskazany dysk). `SHRIMP_DEVTOOLS=1` włącza narzędzia deweloperskie WebView2.

## Otwarte

- Klucz podpisu manifestu (kto go trzyma).
- Limit rozmiaru kopii (teraz: tyle, ile mieści nośnik).
- Podpis Authenticode `.exe` (SmartScreen ostrzega przed niepodpisanymi programami).
- Lokalne repozytorium pakietów na exFAT (Setup bez internetu).
- Nie testowane na prawdziwym sprzęcie: zapis nośnika FAT32 + exFAT i BootNext sprawdzi dopiero komputer z UEFI.


## Zawartość nośnika (kopia i przywracanie)

Formatowanie nośnika nie kasuje plików użytkownika. Przebieg (implementacja: `src/backup.rs`,
w CoreOS-Working `boxes/setup/system/libexec/shrimp-usb-backup`, krok „pendrive” w Setup):

1. **Przed formatowaniem** Shrimp Install kopiuje całą zawartość nośnika do `%LOCALAPPDATA%\Shrimp\usb-backup\<id>\files`,
   licząc SHA-256, a potem czyta kopię z dysku jeszcze raz i porównuje sumy. Błąd, brak miejsca na dysku
   albo na nośniku (kopia + obraz Shrimp + pliki z Windows) kończy przygotowanie, zanim cokolwiek zostanie skasowane.
2. **Po zapisie Shrimp** pliki idą do ukrytego `shrimp-backup/` na partycji exFAT (`manifest.tsv` + `files/`, bez dzielenia plików;
   części `.shrimp-part000…` zostały tylko na starszych nośnikach FAT32 i nadal są czytane), są czytane z nośnika i porównywane. Dopiero wtedy kopia w Windows znika. Ponowne przygotowanie
   tego samego nośnika (etykieta `SHRIMP_LIVE`; partycja `SHRIMP_BOOT` zawiera tylko pliki Shrimp i jest pomijana) bierze oryginały z jego `shrimp-backup/`, z weryfikacją sum.
3. **Instalacja**: `post-install.sh` woła `shrimp-usb-backup stash`: sumy z manifestu, kopia na dysk komputera
   (`/var/lib/shrimp-setup/usb`), drugie sprawdzenie, zapis numeru seryjnego i rozmiaru pendrive'a.
4. **Pierwsza konfiguracja**: krok „pendrive” pokazuje się tylko wtedy, gdy jest co przywracać. „Przywróć” woła
   (pkexec) `shrimp-usb-backup restore`: sprawdza kopię, formatuje wyłącznie dysk USB o zapisanym numerze seryjnym i
   rozmiarze (nigdy dysku systemowego), wgrywa pliki, odmontowuje, czyta z urządzenia i porównuje sumy; dopiero potem
   usuwa kopię z dysku komputera. Gdy cokolwiek zawiedzie albo użytkownik rezygnuje, pliki trafiają do `~/Pendrive`.

`manifest.tsv`: `#shrimp-backup 1`, wiersze `label`, `fs`, `scheme`, `d<TAB>ścieżka`, `f<TAB>sha256<TAB>bajty<TAB>mtime<TAB>części<TAB>ścieżka`.
Test: `boxes/setup/tests/test-usb-backup.sh` (bez prawdziwego pendrive'a) i `shrimp-install.exe --backup-test ŹRÓDŁO CEL KOPIA` w CI.
