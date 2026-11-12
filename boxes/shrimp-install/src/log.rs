//! Dziennik: %LOCALAPPDATA%\Shrimp Install\shrimp-install.log (poprzedni: shrimp-install.old.log).
use std::{fs::File, io::Write, path::Path, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

static LOG: Mutex<Option<File>> = Mutex::new(None);

pub fn open(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("shrimp-install.log");
    let _ = std::fs::rename(&path, dir.join("shrimp-install.old.log"));
    *LOG.lock().unwrap() = File::create(path).ok();
}

pub fn line(s: &str) {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let (sec, ms) = (t.as_secs() % 86400, t.subsec_millis());
    let ts = format!("{:02}:{:02}:{:02}.{ms:03}Z ", sec / 3600, sec / 60 % 60, sec % 60);
    if let Some(f) = LOG.lock().unwrap().as_mut() {
        let _ = f.write_all(format!("{ts}{}\r\n", s.trim_end()).as_bytes());
        let _ = f.flush();
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::line(&format!($($t)*)) };
}
