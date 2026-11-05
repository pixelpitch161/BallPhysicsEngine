//! The aims worked out by test-firing, kept for the process: one emitter's round (or one ball it
//! sends back out) in one scene always aims the same, since the physics is deterministic, so an
//! answer worked out once (by the editor's background aiming, the sound, an export) serves every
//! world of that scene. Keyed by the scene's physics fingerprint ([`bpe_runtime::World::fingerprint`]).
//!
//! An answer being worked out by one thread is waited for by another, never worked out twice.
//! Only the last [`MOST_SCENES`] fingerprints used are kept, and about [`MOST_BALLS`] aimed balls.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use bpe_core::ThingId;

/// How many scenes' aims are kept (more in tests, which run many scenes at once).
pub const MOST_SCENES: usize = if cfg!(test) { 256 } else { 4 };
/// About how many aimed balls are kept in all.
pub const MOST_BALLS: usize = 200_000;

/// What of an emitter's an answer aims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum What {
    /// Round `round`, fired on step `fire`.
    Round { round: u32, fire: u64 },
    /// The ball `row` of `thing` it caught on step `step` and sends back out.
    Catch { step: u64, thing: ThingId, row: u32 },
}

type Key = (u64, ThingId, What);
type Answer = Arc<dyn Any + Send + Sync>;

enum Entry {
    Working,
    Done { answer: Answer, balls: usize },
}

#[derive(Default)]
struct Store {
    entries: HashMap<Key, Entry>,
    /// The fingerprints kept, the one used longest ago first.
    scenes: Vec<u64>,
}

impl Store {
    /// Notes `fp` as just used.
    fn touch(&mut self, fp: u64) {
        self.scenes.retain(|&s| s != fp);
        self.scenes.push(fp);
    }

    /// Lets the scenes used longest ago go while there are more than `most_scenes` of them or
    /// more than `most_balls` aimed balls kept; the one used last always stays.
    fn trim(&mut self, most_scenes: usize, most_balls: usize) {
        let balls = |s: &Store| -> usize {
            s.entries.values().map(|e| if let Entry::Done { balls, .. } = e { *balls } else { 0 }).sum()
        };
        while self.scenes.len() > 1 && (self.scenes.len() > most_scenes || balls(self) > most_balls) {
            let old = self.scenes.remove(0);
            // An answer still being worked out stays until it is done.
            self.entries.retain(|(f, _, _), e| *f != old || matches!(e, Entry::Working));
        }
    }
}

fn store() -> &'static (Mutex<Store>, Condvar) {
    static STORE: OnceLock<(Mutex<Store>, Condvar)> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

/// Takes a `Working` entry back out if its work never finished (it panicked), so a waiting
/// thread works it out itself.
struct Unfinished(Option<Key>);

impl Drop for Unfinished {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            let (lock, wake) = store();
            if let Ok(mut s) = lock.lock() {
                s.entries.remove(&key);
            }
            wake.notify_all();
        }
    }
}

/// The answer for `what` of emitter `me` in the scene with fingerprint `fp`: kept from before,
/// or worked out by `work` now (and kept, counted as `balls(&answer)` aimed balls). With no
/// fingerprint it is simply worked out.
pub fn cached<T: Clone + Send + Sync + 'static>(
    fp: Option<u64>,
    me: ThingId,
    what: What,
    balls: impl FnOnce(&T) -> usize,
    work: impl FnOnce() -> T,
) -> T {
    let Some(fp) = fp else { return work() };
    let key = (fp, me, what);
    let (lock, wake) = store();
    {
        let mut s = lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match s.entries.get(&key) {
                Some(Entry::Done { answer, .. }) => {
                    if let Some(t) = answer.downcast_ref::<T>() {
                        let t = t.clone();
                        s.touch(fp);
                        return t;
                    }
                    // Another kind of answer under the same key: work it out afresh.
                    s.entries.remove(&key);
                }
                Some(Entry::Working) => s = wake.wait(s).unwrap_or_else(|e| e.into_inner()),
                None => break,
            }
        }
        s.entries.insert(key, Entry::Working);
    }
    let mut unfinished = Unfinished(Some(key));
    let answer = work();
    unfinished.0 = None;
    let mut s = lock.lock().unwrap_or_else(|e| e.into_inner());
    let n = balls(&answer);
    s.entries.insert(key, Entry::Done { answer: Arc::new(answer.clone()), balls: n });
    s.touch(fp);
    s.trim(MOST_SCENES, MOST_BALLS);
    drop(s);
    wake.notify_all();
    answer
}

/// Whether the answer for `what` of `me` in scene `fp` is kept.
pub fn is_kept(fp: u64, me: ThingId, what: What) -> bool {
    let s = store().0.lock().unwrap_or_else(|e| e.into_inner());
    matches!(s.entries.get(&(fp, me, what)), Some(Entry::Done { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Fingerprints no real scene has, one range per test, so tests running at once don't meet.
    const BASE: u64 = 0xA1A1_0000_0000_0000;

    #[test]
    fn an_answer_is_worked_out_once() {
        let fp = BASE + 1;
        let calls = AtomicU32::new(0);
        let what = What::Round { round: 1, fire: 10 };
        let work = || {
            calls.fetch_add(1, Ordering::SeqCst);
            42u32
        };
        assert_eq!(cached(Some(fp), ThingId(7), what, |_| 1, work), 42);
        assert_eq!(cached(Some(fp), ThingId(7), what, |_| 1, || 0u32), 42, "kept");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(is_kept(fp, ThingId(7), what));
        assert_eq!(cached(None, ThingId(7), what, |_| 1, || 5u32), 5, "no fingerprint: no keeping");
    }

    #[test]
    fn a_second_thread_waits_for_the_first() {
        let fp = BASE + 100;
        let what = What::Catch { step: 3, thing: ThingId(2), row: 0 };
        let calls = AtomicU32::new(0);
        std::thread::scope(|scope| {
            let hands: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        cached(
                            Some(fp),
                            ThingId(1),
                            what,
                            |_| 1,
                            || {
                                calls.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(std::time::Duration::from_millis(30));
                                7u64
                            },
                        )
                    })
                })
                .collect();
            for h in hands {
                assert_eq!(h.join().unwrap(), 7);
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn old_scenes_go() {
        let mut s = Store::default();
        let what = What::Round { round: 1, fire: 0 };
        for fp in 1..=5u64 {
            s.entries.insert((fp, ThingId(1), what), Entry::Done { answer: Arc::new(()), balls: 10 });
            s.touch(fp);
            s.trim(4, 1000);
        }
        assert_eq!(s.scenes, vec![2, 3, 4, 5]);
        assert!(!s.entries.contains_key(&(1, ThingId(1), what)));
        // Too many balls: down to the last scene used, whatever its size.
        s.touch(3);
        s.trim(4, 15);
        assert_eq!(s.scenes, vec![3]);
        assert_eq!(s.entries.len(), 1);
    }
}
