# Boxy i kontenery

Cały kod Shrimp leży w **boxach**. Box to katalog `boxes/<id>/` z kodem i plikiem `box.toml`
(identyfikator, wersja, rodzaj, zależności, pakiety, sposób budowy). **Kontener** to worek boxów
(`containers/<id>.toml`): mówi, co razem trafia do jednego produktu (ISO, program dla Windows, strona).
Narzędzie `shrimp-box` (sam też jest boxem) czyta oba rodzaje plików, pilnuje zasad i składa z nich obraz.

```
boxes/<id>/box.toml      ← metadane boxa
boxes/<id>/...           ← kod, testy, README boxa
boxes/<id>/i18n/         ← teksty boxa (format boxes/i18n-core)
boxes/<id>/root/         ← pliki kopiowane do obrazu 1:1 (nakładka KIWI)
boxes/<id>/configure.sh  ← fragment config.sh obrazu (chroot)
boxes/<id>/install.sh    ← instalacja do DESTDIR (obraz, pakiet, test)
containers/<id>.toml     ← kontener: lista boxów i kontenerów
tools/                   ← narzędzia repo (testy QEMU, wyniki CI, eksport), nie trafiają do produktu
```

## box.toml

```toml
[box]
id = "panko"                 # = nazwa katalogu
version = "0.1.0"
kind = "app"                 # app | lib | data | system | theme | image | tool | web
platform = "linux"           # linux (domyślnie) | windows | web
summary = "Cofanie zmian w systemie"
depends = []                 # boxy, które muszą być w tym samym kontenerze (i idą wcześniej)
build_depends = ["shrimp-i18n", "help-links"]  # boxy potrzebne tylko do budowy i testów
ci = "panko"                 # .github/workflows/panko.yml (jego paths: składa shrimp-box)

[packages]
image = ["libgtk-4-1", "libadwaita-1-0", "snapper"]   # RPM w obrazie (config.kiwi)
collections = []                                     # wzorce KIWI, np. gnome_basic
build = ["rustup", "gcc", "pkgconfig(gtk4)"]         # RPM do kroku chroot

[build]
chroot = "cargo build --release --locked"   # w chroot Leap (biblioteki jak w obrazie)
outputs = ["target/release/panko"]          # kopiowane z chroot z powrotem
install = "install.sh"                      # install.sh DESTDIR [argumenty z kontenera]

[image]
configure = "configure.sh"   # domyślnie configure.sh, jeśli jest
kiwi_template = "config.kiwi"   # tylko box image-base
kiwi_files = ["edit_boot_config.sh"]   # pliki obok config.kiwi
enable = ["panko-cleanup.timer"]       # usługi systemd włączane w obrazie i po instalacji
user_enable = []                       # usługi użytkownika (systemctl --global enable)
```

Rodzaje: `lib` i `data` niczego nie instalują, wchodzą do innych boxów przez `build_depends`
(biblioteka Rust z `path = "../<box>"`, teksty, schemat). `image` to szkielet obrazu KIWI.

## Zasady (pilnuje ich `shrimp-box check`, workflow **Boxy**)

1. Kod leży w boxie. W korzeniu repo są tylko `boxes/`, `containers/`, `tools/`, `.github/` i opisy.
2. Box sięga do innego boxa tylko przez `../<id>/` i tylko, gdy ma go w `depends` albo `build_depends`.
3. Zależności nie mają cykli. Biblioteka nie zna swoich użytkowników (np. `shrimp-i18n` nie czyta
   tekstów aplikacji: aplikacja wbudowuje je sama w `build.rs`).
4. Kontener ma wszystkie `depends` swoich boxów, boxy z jednej platformy i bez `lib`/`data`.
5. Dwa boxy w kontenerze nie kładą tego samego pliku w `root/`.
6. `paths:` workflow wynikają z grafu (box + jego zależności); `shrimp-box ci --write` je ustawia.
7. Box, którego nic nie używa (kontener, zależność, CI), to martwy kod.

## Kontenery

| Kontener | Co to jest | Boxy |
|---|---|---|
| `desktop` | pulpit Shrimp (sesja live i system po instalacji) | motywy, GNOME, rozszerzenia, Linu(x), Flathub, Shrimp Apps, Cocktail, Panko |
| `iso-live` | nośnik live | `desktop` + `image-base`, `setup --live`, `live-session` |
| `shrimp-install` | program dla Windows | `shrimp-install` |
| `site` | strona shrimp-organization.dev | `site` |

Kolejność w kontenerze to kolejność fragmentów `config.sh`; box zawsze idzie po swoich `depends`.
Argumenty pozycji (`{ id = "setup", args = ["--live"] }`) dostaje `install.sh`, a fragment `configure.sh`
widzi je w `BOX_ARGS`.

## Budowa ISO

`tools/build-iso.sh` w kontenerze Tumbleweed: buduje `shrimp-box`, potem
`shrimp-box image iso-live out/desc --repo <Leap>`:

1. `config.kiwi` = szablon z `image-base` + pakiety `[packages] image` wszystkich boxów (bez powtórzeń),
2. `config.sh` = nagłówek + `configure.sh` boxów w kolejności kontenera,
3. `root/` = nakładki `root/` boxów, pliki `kiwi_files` obok `config.kiwi`,
4. chroot Leap z pakietami `[packages] build`, kroki `build.chroot`, kopie `build.outputs`,
5. `install.sh` boxów do `out/desc/root` (z `SHRIMP_PREBUILT=1` po kroku chroot),
6. manifesty w obrazie, `/usr/share/shrimp/containers/<kontener>.{packages,files,units}`: pakiety,
   pliki z nakładek `root/` i z `install.sh`, usługi z `enable`/`user_enable`,

a na końcu `kiwi-ng`. Podgląd bez budowania: `shrimp-box kiwi iso-live /tmp/desc` i
`shrimp-box build iso-live /tmp/root --dry-run`.

## Polecenia

```
cd boxes/shrimp-box
cargo run -- list                 # boxy i kontenery
cargo run -- check                # zasady
cargo run -- order iso-live       # kolejność boxów
cargo run -- deps shrimp-apps     # zależności (przechodnio)
cargo run -- paths panko          # paths: dla CI
cargo run -- ci --write           # ustawia paths: we wszystkich workflow
cargo run -- graph | dot -Tsvg    # graf zależności
```

## Nowy box

1. `boxes/<id>/box.toml` (id = nazwa katalogu), kod obok.
2. Dopisz go do kontenera albo jako zależność innego boxa.
3. Jeśli ma własne CI: `ci = "<workflow>"` i `cargo run -- ci --write`.
4. `cargo run -- check`.

## Po instalacji na dysk

Agama instaluje z repozytorium tylko pakiety produktu. `post-install.sh` (box `setup`) dokłada resztę
z manifestów kontenera `desktop`: brakujące pakiety (`desktop.packages`), pliki boxów (`desktop.files`,
także programy Shrimp Apps, Cocktail i Panko z ich `install.sh`) i usługi (`desktop.units`). Box pulpitu
nie potrzebuje więc osobnego wpisu w `post-install.sh`; wyjątek to pliki, które `configure.sh` generuje
przy budowie (motywy, ikony MoreWaita, rozszerzenia), bo nie ma ich w manifestach.

## Co dalej

- Pliki generowane przez `configure.sh` (MoreWaita, rozszerzenia, motyw GRUB) też w manifeście, żeby
  `post-install.sh` nie miał listy katalogów.
- Jeden box = jeden pakiet RPM: `box.toml` ma już wersję, pakiety, usługi i instalację do DESTDIR.
