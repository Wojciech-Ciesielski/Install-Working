//! Shrimp Install: przygotowuje nośnik z Shrimp z poziomu Windows, bez Rufusa.
//!
//!   start → pobranie obrazu (manifest, SHA-256) → kopia zawartości nośnika ze sprawdzeniem sum
//!         → GPT: FAT32 „SHRIMP_BOOT” (EFI, boot/) + exFAT „SHRIMP_LIVE” (LiveOS, dane)
//!         → pliki obrazu → shrimp-backup, pliki z Windows, shrimp.json → BootNext i restart
//!
//! Copyright © 2026 Shrimp. GPLv3 albo nowsza.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
// Poza Windows kompiluje się tylko część testowana na Linuksie (kopia nośnika, opis plików).
#![cfg_attr(not(windows), allow(dead_code))]

mod backup;
mod ctx;
mod lang;
mod log;
mod manifest;
mod transfer;
mod tree;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod config {
    include!(concat!(env!("OUT_DIR"), "/config.rs"));
}

fn main() {
    #[cfg(windows)]
    win::main();
    #[cfg(not(windows))]
    {
        // Poza Windows działa tylko test kopii nośnika na katalogach (jak w CI).
        let a: Vec<String> = std::env::args().collect();
        if a.len() == 5 && a[1] == "--backup-test" {
            match backup::self_test(a[2].as_ref(), a[3].as_ref(), a[4].as_ref()) {
                Ok(m) => println!("{m}"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1)
                }
            }
        } else {
            eprintln!("Shrimp Install działa w Windows (poza nim tylko: --backup-test ŹRÓDŁO CEL KOPIA)");
            std::process::exit(2)
        }
    }
}
