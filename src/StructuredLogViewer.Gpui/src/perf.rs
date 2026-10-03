//! Render counters, read by `--automation`'s `perf` command: how many times
//! each view rebuilt its element tree (and each list its rows), and how long
//! that took. The cheapest way to catch a view re-rendering for something it
//! does not show. Off — one relaxed load — unless automation is on.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default, Clone, Copy)]
struct Tally {
    calls: u64,
    items: u64,
    time: Duration,
}

static TALLIES: Mutex<BTreeMap<&'static str, Tally>> = Mutex::new(BTreeMap::new());

/// Times its own lifetime under `name`. Hold it for the length of a
/// `render` (or a list's row processor).
pub struct Scope {
    name: &'static str,
    items: u64,
    start: Option<Instant>,
}

impl Scope {
    /// Rows (or lines) this call produced, for list processors.
    pub fn items(&mut self, n: usize) {
        self.items += n as u64;
    }
}

pub fn scope(name: &'static str) -> Scope {
    let start = crate::automation::enabled().then(Instant::now);
    Scope { name, items: 0, start }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        let elapsed = start.elapsed();
        let mut tallies = TALLIES.lock().unwrap();
        let tally = tallies.entry(self.name).or_default();
        tally.calls += 1;
        tally.items += self.items;
        tally.time += elapsed;
    }
}

/// Everything counted since the last call, then starts over.
pub fn take() -> serde_json::Value {
    let tallies = std::mem::take(&mut *TALLIES.lock().unwrap());
    let map: serde_json::Map<String, serde_json::Value> = tallies
        .into_iter()
        .map(|(name, t)| {
            let mut entry = serde_json::json!({
                "calls": t.calls,
                "ms": (t.time.as_secs_f64() * 1000. * 100.).round() / 100.,
            });
            if t.items > 0 {
                entry["items"] = t.items.into();
            }
            (name.to_string(), entry)
        })
        .collect();
    serde_json::Value::Object(map)
}
