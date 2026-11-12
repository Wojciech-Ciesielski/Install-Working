# Shrimp Install

Program dla Windows 10/11, który przygotowuje nośnik z Shrimp, zachowuje pliki użytkownika, zapisuje listę programów i ustawień Windows dla Setup i uruchamia komputer z nośnika bez wchodzenia do UEFI. Wygląd pochodzi z `shrimp-handoff` (`mockups/shrimp-install.html`, `specs/shrimp-install.md`).

To fork Rufusa 4.16 (GPLv3). Kod Rufusa leży w `rufus/` bez zmian (pierwszy commit), a zmiany Shrimp są osobnymi commitami i komentarzami `SHRIMP` w kodzie Rufusa.

## Jak to działa

```
Start → Nośnik → Pliki → Przygotuj nośnik
          │        │            │
          │        │            ├─ 1. Pobieranie Shrimp   manifest → ISO → SHA-256 (+ podpis manifestu)
          │        │            ├─ 2. Zapis na nośniku    Rufus: GPT, UEFI, FAT32, tryb ISO (pliki)
          │        │            └─ 3. Kopiowanie plików   shrimp-transfer/files/…, shrimp.json
          │        └─ foldery profilu albo wybrane pliki i foldery
          └─ pendrive'y, karty SD, dyski zewnętrzne USB (≥ 16 GB); dysków wewnętrznych Rufus nie pokazuje
Gotowe → Uruchom ponownie: wpis UEFI „Shrimp Install” + BootNext (jednorazowo), wyłączone szybkie uruchamianie, restart
```

Rufus robi to, co umie: wykrywa nośniki, analizuje ISO, formatuje i zapisuje. Jego okno istnieje, ale jest ukryte. Shrimp ustawia jego kontrolki i „naciska” START. Wybór ISO/DD dostaje zawsze tryb ISO, bo tylko wtedy na nośniku zostaje partycja FAT32 z miejscem na pliki z Windows.

Pytania Rufusa (`ShrimpNotification` w `shrimp.c`) mają jawną listę odpowiedzi:

| Pytanie Rufusa | Odpowiedź Shrimp |
|---|---|
| „Wszystkie dane na nośniku zostaną usunięte” (MSG_003), kilka partycji (MSG_094), sektor ≠ 512 B (MSG_197) | tak: zawartość nośnika jest już skopiowana i sprawdzona |
| „Inny program używa nośnika” (MSG_132) | nie: błąd `busy` („Nośnik jest używany”), użytkownik zamyka program i ponawia |
| Pobranie plików GRUB/Syslinux z serwerów Rufusa (MSG_115, MSG_103), aktualizacja DBX (MSG_353), podmiana `.c32` (MSG_083) | nie: start z UEFI ich nie potrzebuje |
| każde inne (np. odwołany program rozruchowy UEFI, MSG_338) | nie: błąd `question` z tekstem Rufusa, zamiast zgadywania |

Interfejs to strona w WebView2 (`ui/`, TypeScript), zbudowana do jednego pliku HTML i wkompilowana w `.exe` razem z `WebView2Loader.dll` (licencja BSD z WebView2 SDK).

| Część | Gdzie |
|---|---|
| Interfejs (ekrany z makiety, tryb podglądu) | `ui/src/app.ts`, `ui/src/mockup.css`, `ui/src/screens.html` |
| Most interfejs ↔ program | `ui/src/backend/host.ts`, `rufus/src/shrimp/window.c` |
| Przebieg, punkty zaczepienia w Rufusie | `rufus/src/shrimp/shrimp.c` |
| Pobieranie i weryfikacja ISO | `rufus/src/shrimp/fetch.c` |
| Kopia plików, `shrimp.json` | `rufus/src/shrimp/transfer.c`, `rufus/src/shrimp/collect.c` |
| BootNext, szybkie uruchamianie, restart | `rufus/src/shrimp/boot.c` |

Zmiany w kodzie Rufusa (wszystkie oznaczone `SHRIMP`):

- `rufus.c`: argumenty Shrimp na początku `WinMain`, ukryte okno Rufusa i start okna Shrimp, bez pytań i sprawdzania aktualizacji Rufusa, bez `rufus.ini` obok programu, powiadomienia o końcu analizy ISO i zapisu.
- `stdlg.c`: `NotificationEx`, `SelectionDialog`, `ListDialog` bez okien w trybie Shrimp.
- `ui.c`: postęp zapisu do interfejsu. `dev.c`: rodzaj nośnika i powiadomienie o zmianie listy.
- `pki.c`: sprawdzanie podpisu dowolnym kluczem (manifest Shrimp ma własny).
- `rufus.h`, `rufus.rc`, `Makefile.am`, `configure.ac`, `.mingw/*.def`: pole `shrimp_kind`, zasoby i ikona Shrimp, nowe pliki i funkcje.

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

Zdarzenia: `devices`, `folders` (rozmiary liczone w tle), `bitlocker`, `progress { phase 1–3, done, total, speed, step? }` (`step`: `verify` — suma obrazu pobranego wcześniej, `prepare` — analiza obrazu i formatowanie, `backup` — kopia zawartości nośnika; bez `step` główna praca fazy), `image { size, version }` (prawdziwy rozmiar obrazu po przeczytaniu manifestu), `fail { phase, kind, message? }` (`offline`, `verify`, `space`, `write`, `device`, `busy`, `question`, `copy`, `other`), `done`, `closeRequested`.

Podgląd ekranów błędów bez Windows: `cd ui && npm run dev`, potem `?screen=brak-internetu`, `?screen=nosnik-zajety`, `?screen=pytanie-rufusa`.

### Strony pomocy

Ikony ⓘ (np. przy wyborze nośnika i plików) otwierają stronę pomocy w przeglądarce, gdy znamy jej adres. Adresy i tematy są w [`boxes/help-links/help.json`](../help-links/help.json) (wspólne z Setup): wpisz stronę w `base` (np. `https://…/help`), a ikona otworzy `base/<ścieżka tematu>?lang=<język interfejsu>&from=install`. Przy pustym `base` ikony zostają samą podpowiedzią. Adres z manifestu wydania (`helpUrl` w odpowiedzi `info`) ma pierwszeństwo. Podstrony (`download-iso`, `copy-files`, `usb-drive`, `bitlocker`, `no-internet`, `boot-from-usb`, `nvidia-driver`) leżą w `boxes/site/help/<ścieżka>.html`, po polsku i angielsku.

## shrimp.json

Opis formalny (JSON Schema) i przykład sprawdzany po obu stronach: [`boxes/install-schema`](../install-schema/README.md).

Na nośniku w `shrimp-transfer/shrimp.json` (na żywym systemie `/run/initramfs/live/shrimp-transfer/`). Setup czyta go w obu częściach: przy instalacji podpowiada imię, język, strefę i klawiaturę, a po restarcie przywraca pliki i proponuje odpowiedniki programów.

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
  "split": ["files/videos/wakacje.mkv"],             // pliki > 4 GB: <plik>.shrimp-part000, 001… (po 2 GiB)
  "skipped": [{ "source": "C:\\Users\\jan\\OneDrive\\x.docx", "reason": "cloud" }]  // cloud, locked, read, missing
}
```

Klucze folderów: `documents`, `pictures`, `desktop`, `music`, `videos`, `downloads` (Moje foldery) i `other` (Wybiorę sam, `files/other/<nazwa>`). Dowiązania i pliki tylko w chmurze (OneDrive „na żądanie”) są pomijane, żeby nie pobierać ich przy kopiowaniu.

Uwaga: dysk z BitLockerem (`windows.bitlocker`, zdarzenie i alert na ekranie Start) uniemożliwia instalację obok Windows, bo Setup nie zmniejszy zaszyfrowanej partycji. „Zamiast Windows” działa.

## Manifest pobierania

`https://…/manifest.json` (adres w `SHRIMP_MANIFEST_URL` przy kompilacji albo `--manifest`):

```json
{ "version": "Beta 3.5", "url": "https://…/Shrimp.x86_64-0.1.0.iso", "size": 2087512064, "sha256": "…" }
```

Adres manifestu: zmienna repozytorium `SHRIMP_MANIFEST_URL` (Install-Working), którą workflow przekazuje do `tools/prepare.sh` (`shrimp_config.h`). Manifest i ISO leżą w Cloudflare R2: wysyła je workflow **Release** w CoreOS-Working (`<publiczny adres bucketu>/manifest.json` = ostatnie wydanie, obraz pod `<tag>/`). R2 nie ma limitu 2 GiB na plik; ograniczeniem zostaje FAT32 (plik < 4 GiB). `tools/prepare.sh` nadal umie adres wydania GitHub (`SHRIMP_REPO`, `SHRIMP_PRIVATE`), gdyby był potrzebny.

Wydanie programu: workflow **Release** tutaj (`.exe` + `SHA256SUMS`).

Obok `manifest.json.sig`: `openssl dgst -sha256 -sign klucz.pem -out manifest.json.sig manifest.json`. Klucz publiczny trafia do programu przez `tools/make-key.sh`. Bez klucza (obecny stan) podpis nie jest sprawdzany, a zaufanie do manifestu daje HTTPS. Suma SHA-256 obrazu jest sprawdzana zawsze. Obraz trafia do `%LOCALAPPDATA%\Shrimp Install\` i jest usuwany po zapisie.

Pobieranie przetrwa zerwane połączenie: plik `shrimp-<wersja>.iso.<16 znaków sumy>.part` zostaje na dysku, a kolejna próba
(automatycznie do 4 razy, z przerwą 2, 4, 8 s, albo po ponownym uruchomieniu programu) dociąga resztę nagłówkiem `Range`.
Serwer, który nie wznawia (HTTP 200 zamiast 206), dostaje pobieranie od początku. Suma jest liczona z całego pliku na dysku,
a części innych wydań są usuwane. Błędy serwera (404, brak tokenu) nie są ponawiane.

## Wymagania wobec ISO (wątek budowy ISO)

- Musi uruchamiać się z plików skopiowanych na FAT32 (tryb ISO Rufusa), nie tylko po zapisie `dd`. Etykieta `SHRIMP_LIVE` (≤ 11 znaków) zostaje etykietą nośnika, więc `root=live:CDLABEL=SHRIMP_LIVE` działa bez zmian.
- Żaden plik w ISO nie może przekraczać 4 GB (FAT32), inaczej Rufus wybierze NTFS.
- `hybridpersistent` nie ma na takim nośniku wolnego miejsca na partycję zapisu; kiwi powinno wtedy użyć tmpfs.
- Start z UEFI przez `\EFI\BOOT\BOOTX64.EFI` (shim podpisany przez Microsoft dla Secure Boot).

## Budowa

```sh
tools/prepare.sh                 # WebView2 SDK z NuGet (suma sprawdzana) i ui/ → rufus/src/shrimp/ui.html
cd rufus && ./bootstrap.sh && ./configure --disable-debug && make -j4   # MSYS2 MinGW64
cp src/rufus.exe shrimp-install.exe
```

CI (`.github/workflows/shrimp-install.yml`) buduje interfejs na Linuksie, a `.exe` na Windows, potem uruchamia `--collect` (sprawdza zebrane dane), `--backup-test`, pełny zapis ISO z ostatniego wydania na pusty wirtualny dysk 32 GB (`--auto-disk`, potem sprawdza pliki na nośniku) i okno w trybie `--demo` (sprawdza, że program z WebView2 uruchamia się na każdym ekranie).

Dziennik każdego uruchomienia: `%LOCALAPPDATA%\Shrimp Install\shrimp-install.log` (poprzedni: `shrimp-install.old.log`). Przy zgłoszeniu błędu poproś o ten plik.

Argumenty: `--demo` (dane z makiety, nic nie zapisuje), `--screen <ekran> --theme light|dark` (podgląd), `--source plik.iso` (bez pobierania), `--manifest URL`, `--collect plik.json` (tylko dane z Windows, bez okna), `--rufus` (zwykłe okno Rufusa), `--auto-disk NUMER` (test: od razu cały przebieg na dysku o tym numerze z Zarządzania dyskami, bez folderów użytkownika; kod wyjścia 0 po zakończeniu, 3 po błędzie; tylko dla dysków testowych, bo kasuje wskazany dysk). `SHRIMP_DEVTOOLS=1` włącza narzędzia deweloperskie WebView2.

## Otwarte

- Adres manifestu i serwer z ISO.
- Klucz podpisu manifestu (kto go trzyma).
- Limit rozmiaru kopii (teraz: tyle, ile mieści nośnik).
- Podpis Authenticode `.exe` (SmartScreen ostrzega przed niepodpisanymi programami).
- Nie testowane na prawdziwym sprzęcie: zapis nośnika i BootNext sprawdzi dopiero komputer z UEFI.


## Zawartość nośnika (kopia i przywracanie)

Formatowanie nośnika nie kasuje plików użytkownika. Przebieg (implementacja: `rufus/src/shrimp/usbbackup.c`,
`boxes/setup/system/libexec/shrimp-usb-backup`, krok „pendrive” w Setup):

1. **Przed formatowaniem** Shrimp Install kopiuje całą zawartość nośnika do `%LOCALAPPDATA%\Shrimp\usb-backup\<id>\files`,
   licząc SHA-256, a potem czyta kopię z dysku jeszcze raz i porównuje sumy. Błąd, brak miejsca na dysku
   albo na nośniku (kopia + obraz Shrimp + pliki z Windows) kończy przygotowanie, zanim cokolwiek zostanie skasowane.
2. **Po zapisie Shrimp** pliki idą do ukrytego `shrimp-backup/` na nośniku (`manifest.tsv` + `files/`; pliki ≥ 4 GB dzielone na
   `.shrimp-part000…`), są czytane z nośnika i porównywane. Dopiero wtedy kopia w Windows znika. Ponowne przygotowanie
   tego samego nośnika (etykieta `SHRIMP_LIVE`) bierze oryginały z jego `shrimp-backup/`, z weryfikacją sum.
3. **Instalacja**: `post-install.sh` woła `shrimp-usb-backup stash`: sumy z manifestu, kopia na dysk komputera
   (`/var/lib/shrimp-setup/usb`), drugie sprawdzenie, zapis numeru seryjnego i rozmiaru pendrive'a.
4. **Pierwsza konfiguracja**: krok „pendrive” pokazuje się tylko wtedy, gdy jest co przywracać. „Przywróć” woła
   (pkexec) `shrimp-usb-backup restore`: sprawdza kopię, formatuje wyłącznie dysk USB o zapisanym numerze seryjnym i
   rozmiarze (nigdy dysku systemowego), wgrywa pliki, odmontowuje, czyta z urządzenia i porównuje sumy; dopiero potem
   usuwa kopię z dysku komputera. Gdy cokolwiek zawiedzie albo użytkownik rezygnuje, pliki trafiają do `~/Pendrive`.

`manifest.tsv`: `#shrimp-backup 1`, wiersze `label`, `fs`, `scheme`, `d<TAB>ścieżka`, `f<TAB>sha256<TAB>bajty<TAB>mtime<TAB>części<TAB>ścieżka`.
Test: `boxes/setup/tests/test-usb-backup.sh` (bez prawdziwego pendrive'a) i `shrimp-install.exe --backup-test ŹRÓDŁO CEL KOPIA` w CI.
