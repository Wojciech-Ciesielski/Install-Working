//! Pobieranie obrazu Shrimp: manifest (opcjonalnie z podpisem RSA-SHA256 `manifest.json.sig`), obraz z wznawianiem,
//! suma SHA-256 sprawdzana zawsze. Obraz trafia do %LOCALAPPDATA%\Shrimp Install\ i jest usuwany po zapisie.
//!
//! Pobieranie przetrwa zerwane połączenie: `shrimp-<wersja>.iso.<16 znaków sumy>.part` zostaje na dysku, kolejna
//! próba (do 4, z przerwą 2, 4, 8 s, albo po ponownym uruchomieniu) dociąga resztę nagłówkiem Range.
use crate::config;
use crate::ctx::{cancelled, fail, Ctx, R};
use crate::manifest::{self, hex, Manifest};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ureq::tls::{TlsConfig, TlsProvider};
use ureq::Agent;

const ATTEMPTS: u32 = 4;

pub struct Image {
    pub path: PathBuf,
    pub size: u64,
    pub version: String,
    pub sha256: String,
    /// Obraz wskazany przez --source zostaje; pobrany jest usuwany po zapisie.
    pub keep: bool,
}

fn agent() -> Agent {
    Agent::config_builder()
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).build())
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(6)
        .timeout_connect(Some(Duration::from_secs(20)))
        // Limit na pojedynczy odczyt, nie na całe ciało odpowiedzi: timeout_recv_body w ureq 3 to łączny czas
        // pobierania, więc obraz 6 GB był przerywany co 60 s i po 4 próbach pobieranie kończyło się błędem.
        .timeout_per_read(Some(Duration::from_secs(60)))
        .user_agent(concat!("ShrimpInstall/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn net_kind(e: &ureq::Error) -> &'static str {
    match e {
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Timeout(_) | ureq::Error::Io(_) => "offline",
        _ => "other",
    }
}

fn small(a: &Agent, url: &str) -> R<Vec<u8>> {
    crate::log!("pobieram {url}");
    let mut r = a.get(url).call().map_err(|e| {
        crate::log!("{url}: {e:?}");
        fail(1, net_kind(&e), None)
    })?;
    let st = r.status().as_u16();
    if st != 200 {
        crate::log!("{url}: HTTP {st}");
        return Err(fail(1, "other", Some(&format!("http:{st}"))));
    }
    r.body_mut().with_config().limit(256 * 1024).read_to_vec().map_err(|e| {
        crate::log!("{url}: odczyt: {e:?}");
        fail(1, net_kind(&e), None)
    })
}

fn verify_sig(data: &[u8], sig: &[u8]) -> bool {
    use rsa::pkcs8::DecodePublicKey;
    let Ok(key) = rsa::RsaPublicKey::from_public_key_pem(config::MANIFEST_PUBKEY) else { return false };
    key.verify(rsa::Pkcs1v15Sign::new::<Sha256>(), &Sha256::digest(data), sig).is_ok()
}

pub fn cache_dir() -> PathBuf {
    let d = super::sys::local_app_data().join("Shrimp Install");
    let _ = fs::create_dir_all(&d);
    d
}

/// Suma pliku z postępem (krok „verify”).
fn hash_file(ctx: &Ctx, path: &Path, size: u64) -> R<String> {
    let mut f = File::open(path).map_err(|_| fail(1, "other", Some("writeDl")))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0;
    ctx.speed_reset(0);
    loop {
        ctx.check(1)?;
        let n = f.read(&mut buf).map_err(|_| fail(1, "other", Some("writeDl")))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        ctx.progress(1, Some("verify"), done, size, ctx.speed(done));
    }
    Ok(hex(&h.finalize()))
}

/// Usuwa stare pliki pobierania shrimp-*<ext> poza `keep` (części starych pobrań, obrazy starszych wydań).
fn remove_stale(dir: &Path, ext: &str, keep: &Path) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.starts_with("shrimp-") && n.ends_with(ext) && p != keep && fs::remove_file(&p).is_ok() {
            crate::log!("usunięto stary plik pobierania {n}");
        }
    }
}

pub fn emit_image(ctx: &Ctx, size: u64, version: &str) {
    ctx.emit(serde_json::json!({ "event": "image", "size": size, "version": version }).to_string());
}

pub fn manifest(url: &str) -> R<Manifest> {
    let a = agent();
    let data = small(&a, url)?;
    if !config::MANIFEST_PUBKEY.is_empty() {
        let sig = small(&a, &format!("{url}.sig")).map_err(|_| fail(1, "verify", Some("sigBad")))?;
        if !verify_sig(&data, &sig) {
            return Err(fail(1, "verify", Some("sigBad")));
        }
    } else {
        crate::log!("UWAGA: kompilacja bez klucza, podpis manifestu nie jest sprawdzany");
    }
    manifest::parse(&data).map_err(|m| fail(1, "other", Some(&m)))
}

/// Faza 1: obraz gotowy na dysku (pobrany i sprawdzony albo wskazany przez --source).
pub fn obtain(ctx: &Ctx, source: Option<&Path>, manifest_url: &str, known: Option<Manifest>) -> R<Image> {
    if let Some(p) = source {
        let size = fs::metadata(p).map_err(|_| fail(1, "other", Some("Nie znaleziono wskazanego obrazu.")))?.len();
        emit_image(ctx, size, "");
        ctx.progress(1, None, size, size, 0);
        return Ok(Image { path: p.into(), size, version: String::new(), sha256: String::new(), keep: true });
    }
    ctx.progress(1, None, 0, 0, 0);
    let m = match known {
        Some(m) => m,
        None => manifest(manifest_url)?,
    };
    crate::log!("{}, {} ({} B)", m.version, m.url, m.size);
    emit_image(ctx, m.size, &m.version);
    let dir = cache_dir();
    let iso = dir.join(format!("{}.iso", manifest::stem(&m.version)));
    let part = PathBuf::from(format!("{}.{}.part", iso.display(), &m.sha256[..16]));
    remove_stale(&dir, ".part", &part);
    remove_stale(&dir, ".iso", &iso);
    let image = |path: PathBuf| Image { path, size: m.size, version: m.version.clone(), sha256: m.sha256.to_lowercase(), keep: false };

    // Już pobrane? Wystarczy sprawdzić sumę.
    if fs::metadata(&iso).is_ok_and(|x| x.len() == m.size) {
        crate::log!("obraz już pobrany, sprawdzanie sumy {}", iso.display());
        if hash_file(ctx, &iso, m.size)?.eq_ignore_ascii_case(&m.sha256) {
            return Ok(image(iso));
        }
        let _ = fs::remove_file(&iso);
    }

    let a = agent();
    let mut last_kind = "offline";
    for attempt in 0..ATTEMPTS {
        ctx.check(1)?;
        let mut have = fs::metadata(&part).map_or(0, |x| x.len());
        if have > m.size {
            let _ = fs::remove_file(&part);
            have = 0;
        }
        if attempt == 0 {
            let need = m.size - have + (256 << 20);
            if super::sys::free_space(&dir).is_some_and(|f| f < need) {
                let gb = format!("{:.1}", need as f64 / (1u64 << 30) as f64).replace('.', ",");
                let drive = dir.to_string_lossy().chars().next().unwrap_or('C');
                return Err(fail(1, "space", Some(&format!("Do pobrania Shrimp potrzeba {gb} GB wolnego miejsca na dysku {drive}:."))));
            }
        }
        if have < m.size {
            let mut req = a.get(&m.url);
            if have > 0 {
                req = req.header("Range", format!("bytes={have}-"));
            }
            match req.call() {
                Err(e) => {
                    crate::log!("{}: {e:?}", m.url);
                    last_kind = net_kind(&e);
                    if last_kind != "offline" {
                        return Err(fail(1, last_kind, Some("connect")));
                    }
                }
                Ok(mut r) => {
                    let st = r.status().as_u16();
                    if st != 200 && st != 206 {
                        crate::log!("{}: HTTP {st}", m.url);
                        return Err(fail(1, "other", Some(&format!("http:{st}"))));
                    }
                    if have > 0 && st != 206 {
                        crate::log!("serwer nie wznawia pobierania (HTTP {st}), od początku");
                        have = 0;
                    } else if have > 0 {
                        crate::log!("wznawiam pobieranie od {have} B");
                    }
                    let mut f = OpenOptions::new().create(true).write(true).truncate(have == 0).open(&part).map_err(|_| fail(1, "other", Some("writeDl")))?;
                    f.set_len(have).and_then(|_| f.seek(SeekFrom::Start(have))).map_err(|_| fail(1, "other", Some("writeDl")))?;
                    let mut body = r.body_mut().with_config().limit(m.size + 1).reader();
                    let mut buf = vec![0u8; 1 << 20];
                    let mut done = have;
                    ctx.speed_reset(done);
                    loop {
                        if ctx.is_cancelled() {
                            return Err(cancelled(1)); // część zostaje: następna próba ją dokończy
                        }
                        let n = match body.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) => {
                                crate::log!("odczyt przerwany ({e}) po {done} B");
                                last_kind = "offline";
                                break;
                            }
                        };
                        if done + n as u64 > m.size {
                            drop(f);
                            let _ = fs::remove_file(&part);
                            return Err(fail(1, "verify", None)); // serwer daje więcej, niż mówi manifest
                        }
                        f.write_all(&buf[..n]).map_err(|e| fail(1, if e.raw_os_error() == Some(112) { "space" } else { "other" }, Some("writeDl")))?;
                        done += n as u64;
                        ctx.bytes(1, done, m.size);
                    }
                    have = done;
                }
            }
        }
        if have == m.size {
            // Suma z pliku na dysku: obejmuje też część pobraną przy wcześniejszym uruchomieniu.
            if !hash_file(ctx, &part, m.size)?.eq_ignore_ascii_case(&m.sha256) {
                let _ = fs::remove_file(&part);
                return Err(fail(1, "verify", None));
            }
            fs::rename(&part, &iso).map_err(|_| fail(1, "other", Some("writeDl")))?;
            return Ok(image(iso));
        }
        if attempt + 1 < ATTEMPTS {
            crate::log!("próba {} z {ATTEMPTS} nieudana, ponawiam", attempt + 1);
            for _ in 0..(20u32 << attempt) {
                ctx.check(1)?;
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(fail(1, last_kind, None))
}
