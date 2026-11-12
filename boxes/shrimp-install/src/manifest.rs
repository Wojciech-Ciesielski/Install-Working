//! Manifest pobierania (R2, wydanie CoreOS-Working):
//! `{ "version": "Beta 5", "url": "https://…/Shrimp.iso", "size": 2087512064, "sha256": "…", "helpUrl"?: "https://…" }`.
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default, rename = "helpUrl")]
    pub help_url: Option<String>,
}

pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| "Manifest pobierania jest niekompletny.".to_string())?;
    if !m.url.starts_with("https://") || m.sha256.len() != 64 || !m.sha256.bytes().all(|c| c.is_ascii_hexdigit()) || m.size == 0 {
        return Err("Manifest pobierania jest niekompletny.".into());
    }
    Ok(m)
}

/// Nazwa pliku obrazu z wersji, tylko bezpieczne znaki: „Beta 5” → „shrimp-Beta-5”.
pub fn stem(version: &str) -> String {
    let v: String = version.chars().take(48).map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    format!("shrimp-{v}")
}

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Adres strony pomocy: tylko https:// i zwykłe znaki adresu (ShellExecute otwiera też pliki i programy).
pub fn safe_url(url: &str) -> bool {
    url.len() >= 12 && url.len() <= 500 && url.starts_with("https://")
        && url.chars().all(|c| c.is_ascii_alphanumeric() || ".-_~:/?&=%".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest() {
        let ok = br#"{"version":"Beta 5","url":"https://x.example/a.iso","size":5,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        let m = parse(ok).unwrap();
        assert_eq!(m.size, 5);
        assert!(parse(br#"{"url":"http://x/a.iso","size":5,"sha256":"aa"}"#).is_err());
        assert_eq!(stem("Beta 3.5/x"), "shrimp-Beta-3.5-x");
    }
    #[test]
    fn urls() {
        assert!(safe_url("https://shrimp.example/help/usb?lang=pl"));
        assert!(!safe_url("file:///C:/Windows/notepad.exe"));
        assert!(!safe_url("https://a.example/\"&calc"));
    }
}
