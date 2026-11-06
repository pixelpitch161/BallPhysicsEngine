//! What a block remembers per ball (or once for the rule), at most
//! [`bpe_runtime::rules::MOST_REMEMBERED`] at a time, the least lately used forgotten first.

use std::collections::BTreeMap;

use bpe_runtime::Party;
use bpe_runtime::rules::MOST_REMEMBERED;

/// Per key (a ball, or `None` for the whole rule), a `V` and when it was last used.
#[derive(Clone, Debug)]
pub struct Remembered<V> {
    map: BTreeMap<Option<Party>, (V, u64)>,
    clock: u64,
}

impl<V> Default for Remembered<V> {
    fn default() -> Self {
        Remembered { map: BTreeMap::new(), clock: 0 }
    }
}

impl<V: Default> Remembered<V> {
    /// The value for `key`, made when there is none (forgetting the least lately used one when
    /// full).
    pub fn get(&mut self, key: Option<Party>) -> &mut V {
        self.clock += 1;
        let clock = self.clock;
        if !self.map.contains_key(&key) && self.map.len() >= MOST_REMEMBERED {
            let oldest = self.map.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.map.remove(&k);
            }
        }
        let entry = self.map.entry(key).or_insert_with(|| (V::default(), clock));
        entry.1 = clock;
        &mut entry.0
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
