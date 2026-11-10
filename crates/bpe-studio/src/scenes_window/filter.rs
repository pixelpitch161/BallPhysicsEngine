//! Which built-in scenes the Scenes window shows: one level, at most one kind and the words typed in Find a
//! scene, with the counts each filter button shows.

use bpe_kit::library::{Entry, Kind, Level};

/// The filters that are on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// `None`: every level ("All").
    pub level: Option<Level>,
    pub kind: Option<Kind>,
    /// The Find a scene box: every word must be in the scene's name, kinds or description, ignoring case.
    pub words: String,
}

fn passes(e: &Entry, level: Option<Level>, kind: Option<Kind>, words: &str) -> bool {
    level.is_none_or(|l| e.info.level == l) && kind.is_none_or(|k| e.info.kinds.contains(&k)) && e.info.matches(words)
}

impl Filter {
    /// The scenes it shows, in list order.
    pub fn shown(&self, all: &'static [Entry]) -> Vec<&'static Entry> {
        all.iter().filter(|e| passes(e, self.level, self.kind, &self.words)).collect()
    }

    /// How many scenes the level button `level` would show, with this filter's kind and words.
    pub fn level_count(&self, all: &[Entry], level: Option<Level>) -> usize {
        all.iter().filter(|e| passes(e, level, self.kind, &self.words)).count()
    }

    /// How many scenes the kind button `kind` would show, with this filter's level and words.
    pub fn kind_count(&self, all: &[Entry], kind: Kind) -> usize {
        all.iter().filter(|e| passes(e, self.level, Some(kind), &self.words)).count()
    }

    /// Nothing narrows the list.
    pub fn is_clear(&self) -> bool {
        self.level.is_none() && self.kind.is_none() && self.words.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::library;

    fn ids(list: &[&Entry]) -> Vec<&'static str> {
        list.iter().map(|e| e.info.id).collect()
    }

    #[test]
    fn no_filter_shows_all_in_order() {
        let f = Filter::default();
        assert!(f.is_clear());
        let all: Vec<_> = library::all().iter().map(|e| e.info.id).collect();
        assert_eq!(ids(&f.shown(library::all())), all);
    }

    #[test]
    fn level_and_kind_narrow() {
        let f = Filter { level: Some(Level::Spectacle), ..Default::default() };
        assert!(f.shown(library::all()).iter().all(|e| e.info.level == Level::Spectacle));
        let f = Filter { kind: Some(Kind::Hypnotic), ..Default::default() };
        let shown = f.shown(library::all());
        assert!(!shown.is_empty() && shown.iter().all(|e| e.info.kinds.contains(&Kind::Hypnotic)));
        assert!(shown.len() < library::all().len());
        assert!(!f.is_clear());
    }

    #[test]
    fn words_must_all_match_ignoring_case() {
        let f = Filter { words: "BOUNCE circle".into(), ..Default::default() };
        let shown = ids(&f.shown(library::all()));
        assert!(shown.contains(&"ball-in-a-circle") && !shown.contains(&"first-bounce"), "{shown:?}");
        let lower = Filter { words: "bounce CIRCLE".into(), ..Default::default() };
        assert_eq!(ids(&lower.shown(library::all())), shown, "case doesn't matter");
        let f = Filter { words: "  ".into(), ..Default::default() };
        assert!(f.is_clear());
        assert_eq!(f.shown(library::all()).len(), library::all().len());
    }

    #[test]
    fn counts_follow_the_other_filters() {
        let f = Filter { words: "ring".into(), ..Default::default() };
        assert_eq!(f.level_count(library::all(), None), f.shown(library::all()).len());
        let with_level = Filter { level: Some(Level::Starter), ..f.clone() };
        assert_eq!(f.level_count(library::all(), Some(Level::Starter)), with_level.shown(library::all()).len());
        for kind in Kind::ALL {
            let on = Filter { kind: Some(kind), ..f.clone() };
            assert_eq!(f.kind_count(library::all(), kind), on.shown(library::all()).len());
        }
    }

    #[test]
    fn nothing_matches_shows_nothing_and_clear_resets() {
        let mut f = Filter { level: Some(Level::Starter), words: "zebra".into(), ..Default::default() };
        assert!(f.shown(library::all()).is_empty());
        assert!(Kind::ALL.iter().all(|&k| f.kind_count(library::all(), k) == 0), "a 0 shows a kind would empty it");
        assert_eq!(f.level_count(library::all(), None), 0);
        f = Filter::default();
        assert!(f.is_clear());
        assert_eq!(f.shown(library::all()).len(), library::all().len());
    }
}
