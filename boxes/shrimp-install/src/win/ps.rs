//! PowerShell (moduł Storage Windows) do rzeczy, które Windows i tak robi najlepiej:
//! lista dysków, czyszczenie, partycje, formatowanie, montowanie obrazu.
use base64::Engine;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn exe() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// Uruchamia skrypt; wynik to standardowe wyjście (UTF-8), błąd to treść wyjątku PowerShell.
pub fn run(script: &str) -> Result<String, String> {
    let full = format!(
        "$ProgressPreference='SilentlyContinue'\n$ErrorActionPreference='Stop'\n[Console]::OutputEncoding=[Text.Encoding]::UTF8\ntry {{\n{script}\n}} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
    );
    let utf16: Vec<u8> = full.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let b64 = base64::engine::general_purpose::STANDARD.encode(utf16);
    let out = Command::new(exe())
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &b64])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().replace(['\r', '\n'], " ");
        Err(if err.is_empty() { format!("kod {}", out.status.code().unwrap_or(-1)) } else { err })
    }
}

/// Tekst w apostrofach PowerShell.
pub fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Uruchamia skrypt i czyta JSON z wyjścia.
pub fn json<T: serde::de::DeserializeOwned>(script: &str) -> Result<T, String> {
    let s = run(script)?;
    serde_json::from_str(&s).map_err(|e| format!("odpowiedź PowerShell: {e}: {}", s.chars().take(200).collect::<String>()))
}
