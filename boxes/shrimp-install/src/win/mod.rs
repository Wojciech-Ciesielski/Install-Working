//! Część dla Windows: okno, nośniki, pobieranie, dane z Windows, wpis rozruchowy.
pub mod app;
mod boot;
mod collect;
mod disk;
mod fetch;
mod ps;
mod sys;
mod window;

use crate::ctx::Ctx;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

include!(concat!(env!("OUT_DIR"), "/native_i18n.rs"));

/// Komunikat sprzed startu interfejsu w języku Windows (klucze native.* z i18n/), inaczej po angielsku.
fn native(i: usize) -> String {
    let tag = sys::ui_languages().first().map_or_else(|| "en".to_string(), |l| crate::lang::tag(l));
    NATIVE.iter().find(|(t, _)| *t == tag).unwrap_or(&NATIVE[0]).1[i].to_string()
}

fn args() -> app::Opts {
    let a: Vec<String> = std::env::args().collect();
    let mut o = app::Opts::default();
    let mut i = 1;
    while i < a.len() {
        let v = a.get(i + 1).cloned();
        let mut used = true;
        match a[i].as_str() {
            "--screen" => o.screen = v,
            "--theme" => o.theme = v,
            "--lang" => o.lang = v,
            "--manifest" => o.manifest = v,
            "--source" => o.source = v.map(PathBuf::from),
            "--collect" => o.collect = v.map(PathBuf::from),
            "--auto-disk" => o.auto_disk = v.and_then(|s| s.parse().ok()),
            "--backup-test" if a.len() > i + 3 => {
                sys::attach_console();
                match crate::backup::self_test(a[i + 1].as_ref(), a[i + 2].as_ref(), a[i + 3].as_ref()) {
                    Ok(m) => {
                        println!("{m}");
                        std::process::exit(0)
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1)
                    }
                }
            }
            "--demo" => {
                o.demo = true;
                used = false;
            }
            _ => used = false,
        }
        i += if used { 2 } else { 1 };
    }
    o
}

/// --collect PLIK: tylko dane z Windows (bez okna), do testu w CI i zgłoszeń.
fn collect_only(path: &std::path::Path) -> ! {
    let bitlocker = disk::bitlocker_on();
    let mut v = collect::collect(bitlocker);
    v["version"] = json!(1);
    v["installer"] = json!(concat!("Shrimp Install ", env!("CARGO_PKG_VERSION")));
    v["folders"] = json!(sys::folders()
        .into_iter()
        .map(|(k, p)| { let s = crate::tree::size(&crate::tree::long(&p), None); json!({ "key": k, "path": p, "size": s }) })
        .collect::<Vec<_>>());
    v["devices"] = json!(disk::list(None).unwrap_or_default().iter().map(disk::Disk::to_json).collect::<Vec<_>>());
    let ok = std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).is_ok();
    std::process::exit(if ok { 0 } else { 1 })
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn main() {
    let opts = args();
    if let Some(p) = &opts.collect {
        collect_only(p);
    }
    crate::log::open(&fetch::cache_dir());
    crate::log!("Shrimp Install {}", env!("CARGO_PKG_VERSION"));

    let mut o = json!({ "demo": opts.demo });
    if let Some(s) = &opts.screen {
        o["screen"] = json!(s);
    }
    if let Some(t) = &opts.theme {
        o["theme"] = json!(t);
    }
    if let Some(l) = &opts.lang {
        o["lang"] = json!(l);
    }
    o["langs"] = json!(sys::ui_languages());

    let auto = opts.auto_disk;
    let demo = opts.demo;
    let code = window::run(
        |proxy| {
            let last_progress = AtomicU64::new(0);
            // Zdarzenia: do interfejsu i do dziennika (postęp najwyżej co 5 s); --auto-disk kończy proces na końcu przebiegu.
            let ctx = Arc::new(Ctx::new(move |s: String| {
                let progress = s.contains(r#""event":"progress""#);
                let quiet = s.starts_with(r#"{"id""#) || s.contains(r#""event":"folders""#) || s.contains(r#""event":"devices""#);
                if progress {
                    let now = now_ms();
                    if now - last_progress.load(Ordering::Relaxed) >= 5000 {
                        last_progress.store(now, Ordering::Relaxed);
                        crate::log::line(&s);
                    }
                } else if !quiet {
                    crate::log::line(&s);
                }
                if auto.is_some() && s.contains(r#""event":"done""#) {
                    crate::log!("--auto-disk: gotowe");
                    std::process::exit(0);
                }
                if auto.is_some() && s.contains(r#""event":"fail""#) {
                    crate::log!("--auto-disk: błąd");
                    std::process::exit(3);
                }
                let _ = proxy.lock().unwrap().send_event(window::UserEvent::Emit(s));
            }));
            let a = app::App::new(ctx, opts);
            if !demo {
                a.start_background();
            }
            // Test w CI: przebieg od razu, jak po „Przygotuj nośnik” w interfejsie (bez folderów użytkownika).
            if let Some(n) = auto {
                crate::log!("--auto-disk {n}");
                let a2 = a.clone();
                std::thread::spawn(move || {
                    a2.dispatch(&format!("0\tstart\t{n}\t"));
                });
            }
            a
        },
        o.to_string(),
        native,
    );
    std::process::exit(code);
}
