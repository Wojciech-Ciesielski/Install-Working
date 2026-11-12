//! Wspólne dla przebiegu: zdarzenia dla interfejsu, postęp z prędkością, anulowanie i błędy.
use serde_json::json;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use std::time::Instant;

/// Błąd dla interfejsu: zdarzenie `fail { phase, kind, message? }`. kind = "cancel" oznacza anulowanie.
#[derive(Debug, Clone, PartialEq)]
pub struct Fail {
    pub phase: u8,
    pub kind: &'static str,
    pub message: Option<String>,
}

pub fn fail(phase: u8, kind: &'static str, message: Option<&str>) -> Fail {
    Fail { phase, kind, message: message.map(String::from) }
}

pub fn cancelled(phase: u8) -> Fail {
    fail(phase, "cancel", None)
}

pub type R<T> = Result<T, Fail>;

struct Speed {
    t: Instant,
    done: u64,
    ema: f64,
}

struct Last {
    t: Option<Instant>,
    phase: u8,
    step: Option<&'static str>,
}

pub struct Ctx {
    emit: Box<dyn Fn(String) + Send + Sync>,
    pub cancel: AtomicBool,
    speed: Mutex<Speed>,
    last: Mutex<Last>,
}

impl Ctx {
    pub fn new(emit: impl Fn(String) + Send + Sync + 'static) -> Self {
        Ctx {
            emit: Box::new(emit),
            cancel: AtomicBool::new(false),
            speed: Mutex::new(Speed { t: Instant::now(), done: 0, ema: 0.0 }),
            last: Mutex::new(Last { t: None, phase: 0, step: None }),
        }
    }

    pub fn emit(&self, s: String) {
        (self.emit)(s)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn check(&self, phase: u8) -> R<()> {
        if self.is_cancelled() { Err(cancelled(phase)) } else { Ok(()) }
    }

    pub fn speed_reset(&self, done: u64) {
        *self.speed.lock().unwrap() = Speed { t: Instant::now(), done, ema: 0.0 };
    }

    /// Prędkość w B/s: średnia ruchoma z pomiarów co sekundę.
    pub fn speed(&self, done: u64) -> u64 {
        let mut s = self.speed.lock().unwrap();
        let dt = s.t.elapsed().as_secs_f64();
        if dt >= 1.0 && done >= s.done {
            let inst = (done - s.done) as f64 / dt;
            s.ema = if s.ema > 0.0 { s.ema * 0.7 + inst * 0.3 } else { inst };
            s.t = Instant::now();
            s.done = done;
        }
        s.ema as u64
    }

    /// Zdarzenie `progress` (najwyżej co 150 ms dla tej samej fazy i kroku, koniec zawsze).
    pub fn progress(&self, phase: u8, step: Option<&'static str>, done: u64, total: u64, speed: u64) {
        {
            let mut l = self.last.lock().unwrap();
            if l.phase == phase && l.step == step && done < total && l.t.is_some_and(|t| t.elapsed().as_millis() < 150) {
                return;
            }
            *l = Last { t: Some(Instant::now()), phase, step };
        }
        let mut v = json!({ "event": "progress", "phase": phase, "done": done, "total": total, "speed": speed });
        if let Some(s) = step {
            v["step"] = json!(s);
        }
        self.emit(v.to_string());
    }

    /// Postęp głównej pracy fazy z prędkością liczoną od `speed_reset`.
    pub fn bytes(&self, phase: u8, done: u64, total: u64) {
        let sp = self.speed(done);
        self.progress(phase, None, done, total, sp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn throttles_progress() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let c = Ctx::new(move |s| g.lock().unwrap().push(s));
        for i in 0..100 {
            c.progress(1, None, i, 100, 0);
        }
        c.progress(1, None, 100, 100, 0);
        let v = got.lock().unwrap();
        assert_eq!(v.len(), 2);
        assert!(v[1].contains("\"done\":100"));
    }
}
