// Budowa Shrimp Install:
//  - interfejs (ui/dist/index.html, jeden plik z `npm run build`; inna ścieżka: SHRIMP_UI_HTML) wkompilowany w program,
//  - komunikaty sprzed startu interfejsu z i18n/ (klucze native.*) jako tabela Rust,
//  - adres manifestu (SHRIMP_MANIFEST_URL) i klucz podpisu manifestu (SHRIMP_MANIFEST_PUBKEY: plik PEM, opcjonalnie),
//  - ikona i manifest Windows (administrator, DPI) przy kompilacji MSVC.
use std::{env, fs, path::PathBuf};

const NATIVE: [&str; 4] = ["native.wv2missing", "native.windowFail", "native.wv2fail", "native.noUi"];

fn rows(s: &str) -> Vec<Vec<String>> {
    s.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(String::from).collect())
        .collect()
}

fn main() {
    let here = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for v in ["SHRIMP_UI_HTML", "SHRIMP_MANIFEST_URL", "SHRIMP_MANIFEST_PUBKEY"] {
        println!("cargo:rerun-if-env-changed={v}");
    }

    // Interfejs
    let ui = env::var("SHRIMP_UI_HTML").map(PathBuf::from).unwrap_or_else(|_| here.join("ui/dist/index.html"));
    println!("cargo:rerun-if-changed={}", ui.display());
    let html = fs::read_to_string(&ui).unwrap_or_else(|_| {
        println!("cargo:warning=brak {}: najpierw `cd ui && npm run build`; program dostaje stronę zastępczą", ui.display());
        String::new()
    });
    fs::write(out.join("ui.html"), html).unwrap();

    // Komunikaty natywne
    let dir = here.join("i18n");
    println!("cargo:rerun-if-changed={}", dir.display());
    let keys = rows(&fs::read_to_string(dir.join("_keys.tsv")).expect("i18n/_keys.tsv"));
    let ids: Vec<(String, String)> = NATIVE
        .iter()
        .map(|k| {
            let r = keys.iter().find(|r| r.get(1).map(String::as_str) == Some(*k)).unwrap_or_else(|| panic!("brak klucza {k}"));
            (r[0].clone(), r[2].replace("\\n", "\n"))
        })
        .collect();
    let mut tags: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    tags.sort();
    let mut src = String::from("pub static NATIVE: &[(&str, [&str; 4])] = &[\n");
    src += &format!("    (\"en\", {:?}),\n", ids.iter().map(|(_, en)| en.as_str()).collect::<Vec<_>>());
    for (tag, p) in tags {
        let t = rows(&fs::read_to_string(&p).unwrap());
        let tr: Vec<String> = ids
            .iter()
            .map(|(id, en)| t.iter().find(|r| r[0] == *id).and_then(|r| r.get(1)).map(|s| s.replace("\\n", "\n")).unwrap_or(en.clone()))
            .collect();
        src += &format!("    ({tag:?}, {tr:?}),\n");
    }
    src += "];\n";
    fs::write(out.join("native_i18n.rs"), src).unwrap();

    // Manifest pobierania i klucz podpisu
    let url = env::var("SHRIMP_MANIFEST_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "https://download.shrimp.example/install/manifest.json".into());
    assert!(url.starts_with("https://") && url.chars().all(|c| c.is_ascii_alphanumeric() || "._/:@%?=&-~".contains(c)), "niedozwolony adres manifestu");
    let key = match env::var("SHRIMP_MANIFEST_PUBKEY") {
        Ok(p) if !p.is_empty() => fs::read_to_string(&p).expect("SHRIMP_MANIFEST_PUBKEY: plik PEM"),
        _ => String::new(),
    };
    fs::write(out.join("config.rs"), format!("pub const MANIFEST_URL: &str = {url:?};\npub const MANIFEST_PUBKEY: &str = {key:?};\n")).unwrap();

    // Zasoby Windows (ikona, manifest) tylko z kompilatorem zasobów MSVC.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rerun-if-changed=data/shrimp-install.rc");
        println!("cargo:rerun-if-changed=data/shrimp-install.manifest");
        let _ = embed_resource::compile("data/shrimp-install.rc", embed_resource::NONE);
    }
}
