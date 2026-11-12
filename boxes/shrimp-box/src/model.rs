//! Model danych: box (`boxes/<id>/box.toml`) i kontener (`containers/<id>.toml`).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Rodzaj boxa. `lib` i `data` dają tylko źródła innym boxom (nie trafiają same do obrazu),
/// `image` to szkielet obrazu KIWI, reszta coś instaluje albo konfiguruje.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    App,
    Lib,
    Data,
    System,
    Theme,
    Image,
    Tool,
    Web,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxFile {
    #[serde(rename = "box")]
    pub meta: BoxMeta,
    #[serde(default)]
    pub packages: Packages,
    #[serde(default)]
    pub build: Build,
    #[serde(default)]
    pub image: Image,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxMeta {
    pub id: String,
    pub version: String,
    pub kind: Kind,
    #[serde(default)]
    pub summary: String,
    /// `linux` (obraz Shrimp, domyślnie), `windows` (Shrimp Install), `web` (strona).
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Boxy, które muszą być w tym samym kontenerze (zależność w działającym systemie).
    #[serde(default)]
    pub depends: Vec<String>,
    /// Boxy potrzebne tylko do budowania i testów (biblioteka Rust z `path =`, wspólne teksty, schemat).
    #[serde(default)]
    pub build_depends: Vec<String>,
    /// Workflow CI boxa (`.github/workflows/<ci>.yml`); jego `paths:` składa `shrimp-box ci --write`.
    pub ci: Option<String>,
}

fn default_platform() -> String {
    "linux".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    /// Pakiety RPM w obrazie (trafiają do config.kiwi).
    #[serde(default)]
    pub image: Vec<String>,
    /// Kolekcje (wzorce) KIWI, np. gnome_basic.
    #[serde(default)]
    pub collections: Vec<String>,
    /// Pakiety RPM do zbudowania w chroot Leap (krok `build.chroot`).
    #[serde(default)]
    pub build: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// Polecenie w chroot Leap, w katalogu boxa (te same biblioteki co obraz), np. `cargo build --release --locked`.
    pub chroot: Option<String>,
    /// Pliki zbudowane w chroot (względem katalogu boxa), kopiowane z powrotem do repo.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Skrypt instalujący (względem katalogu boxa): `<install> DESTDIR [argumenty z kontenera]`.
    /// Po kroku chroot dostaje SHRIMP_PREBUILT=1 i nie buduje drugi raz.
    pub install: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    /// Fragment config.sh (chroot obrazu), względem katalogu boxa. Domyślnie `configure.sh`, jeśli jest.
    pub configure: Option<String>,
    /// Szablon config.kiwi z `@SHRIMP_PACKAGES@` (tylko jeden box w kontenerze).
    pub kiwi_template: Option<String>,
    /// Pliki kopiowane obok config.kiwi (np. hook editbootconfig).
    #[serde(default)]
    pub kiwi_files: Vec<String>,
    /// Usługi systemd włączane w obrazie (`systemctl enable`) i w zainstalowanym systemie (post-install Setup).
    #[serde(default)]
    pub enable: Vec<String>,
    /// Usługi użytkownika włączane dla wszystkich (`systemctl --global enable`).
    #[serde(default)]
    pub user_enable: Vec<String>,
}

/// Box wczytany z dysku.
#[derive(Debug, Clone)]
pub struct Boxx {
    pub file: BoxFile,
    /// Katalog boxa, względem korzenia repo (`boxes/<id>`).
    pub dir: PathBuf,
}

impl Boxx {
    pub fn id(&self) -> &str {
        &self.file.meta.id
    }
    pub fn meta(&self) -> &BoxMeta {
        &self.file.meta
    }
    /// Wzorzec `paths:` GitHub Actions dla plików boxa.
    pub fn path_glob(&self) -> String {
        format!("{}/**", self.dir.display())
    }
    /// Fragment config.sh (jawny albo `configure.sh`, jeśli istnieje).
    pub fn configure(&self, root: &Path) -> Option<PathBuf> {
        match &self.file.image.configure {
            Some(c) => Some(self.dir.join(c)),
            None => {
                let p = self.dir.join("configure.sh");
                root.join(&p).is_file().then_some(p)
            }
        }
    }
    /// Nakładka plików obrazu (`root/`), jeśli istnieje.
    pub fn root_overlay(&self, root: &Path) -> Option<PathBuf> {
        let p = self.dir.join("root");
        root.join(&p).is_dir().then_some(p)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerFile {
    pub container: ContainerMeta,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerMeta {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Inne kontenery, których boxy wchodzą do tego (przed własnymi), np. iso-live = desktop + nośnik.
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub boxes: Vec<Entry>,
    /// Workflow CI budujący kontener; jego `paths:` = wszystkie boxy kontenera i ich zależności + `ci_extra`.
    pub ci: Option<String>,
    /// Dodatkowe ścieżki dla `paths:` workflow kontenera (np. skrypt budowy).
    #[serde(default)]
    pub ci_extra: Vec<String>,
}

/// Pozycja w kontenerze: samo id albo id z argumentami dla skryptu instalacji i fragmentu config.sh.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Id(String),
    With {
        id: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl Entry {
    pub fn id(&self) -> &str {
        match self {
            Entry::Id(s) | Entry::With { id: s, .. } => s,
        }
    }
    pub fn args(&self) -> &[String] {
        match self {
            Entry::Id(_) => &[],
            Entry::With { args, .. } => args,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pub file: ContainerFile,
}

impl Container {
    pub fn meta(&self) -> &ContainerMeta {
        &self.file.container
    }
}

/// Wszystkie boxy i kontenery repo.
#[derive(Debug, Default)]
pub struct Repo {
    pub root: PathBuf,
    pub boxes: BTreeMap<String, Boxx>,
    pub containers: BTreeMap<String, Container>,
    /// Błędy wczytywania (zły TOML, id inne niż katalog); `check` je pokazuje.
    pub load_errors: Vec<String>,
}

impl Repo {
    pub fn load(root: &Path) -> Repo {
        let mut repo = Repo { root: root.to_path_buf(), ..Default::default() };
        for dir in sorted_entries(&root.join("boxes")).into_iter().filter(|p| p.is_dir()) {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            let rel = PathBuf::from("boxes").join(&name);
            let manifest = dir.join("box.toml");
            if !manifest.is_file() {
                repo.load_errors.push(format!("{}: brak box.toml", rel.display()));
                continue;
            }
            match read_toml::<BoxFile>(&manifest) {
                Ok(file) => {
                    if file.meta.id != name {
                        repo.load_errors.push(format!(
                            "{}/box.toml: id „{}” różni się od nazwy katalogu „{name}”",
                            rel.display(),
                            file.meta.id
                        ));
                        continue;
                    }
                    repo.boxes.insert(name, Boxx { file, dir: rel });
                }
                Err(e) => repo.load_errors.push(format!("{}/box.toml: {e}", rel.display())),
            }
        }
        for p in sorted_entries(&root.join("containers")) {
            if p.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap_or(&p).to_path_buf();
            match read_toml::<ContainerFile>(&p) {
                Ok(file) => {
                    let id = file.container.id.clone();
                    if p.file_stem().is_some_and(|s| s != id.as_str()) {
                        repo.load_errors.push(format!("{}: id „{id}” różni się od nazwy pliku", rel.display()));
                        continue;
                    }
                    repo.containers.insert(id, Container { file });
                }
                Err(e) => repo.load_errors.push(format!("{}: {e}", rel.display())),
            }
        }
        repo
    }

    pub fn get(&self, id: &str) -> Result<&Boxx, String> {
        self.boxes.get(id).ok_or_else(|| format!("nie ma boxa „{id}”"))
    }

    pub fn container(&self, id: &str) -> Result<&Container, String> {
        self.containers.get(id).ok_or_else(|| format!("nie ma kontenera „{id}”"))
    }
}

fn read_toml<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    let text = fs::read_to_string(p).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}
