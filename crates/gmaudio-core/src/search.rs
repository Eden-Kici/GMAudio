//! In-memory search over the library.
//!
//! The whole library is kept in memory (a 10,000-file library is a few
//! megabytes), so a search is one pass over a vector: no database round
//! trip, well under the 50 ms budget.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::Asset;

struct Entry {
    asset: Asset,
    name_lc: String,
    tags_lc: Vec<String>,
    /// Folder part of the path, lowercased.
    dir_lc: String,
}

impl Entry {
    fn new(asset: Asset) -> Self {
        let path_lc = asset.path.to_lowercase();
        let dir_lc = match path_lc.rfind(['/', '\\']) {
            Some(i) => path_lc[..i].to_string(),
            None => String::new(),
        };
        Self {
            name_lc: asset.name.to_lowercase(),
            tags_lc: asset.tags.iter().map(|t| t.name.to_lowercase()).collect(),
            dir_lc,
            asset,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Query {
    /// Free text. Words are ANDed. A word starting with `#` matches tags only.
    pub text: String,
    pub folder_id: Option<i64>,
    /// Every tag listed must be on the asset.
    pub tags: Vec<String>,
    /// 0 means no limit.
    pub limit: usize,
    pub include_missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// How many assets matched, before `limit` was applied.
    pub total: usize,
    pub assets: Vec<Asset>,
}

#[derive(Default)]
pub struct Index {
    entries: Vec<Entry>,
    by_id: HashMap<i64, usize>,
}

impl Index {
    pub fn new(assets: Vec<Asset>) -> Self {
        let mut index = Self::default();
        index.rebuild(assets);
        index
    }

    pub fn rebuild(&mut self, assets: Vec<Asset>) {
        self.entries = assets.into_iter().map(Entry::new).collect();
        self.by_id = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.asset.id, i))
            .collect();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: i64) -> Option<&Asset> {
        self.by_id.get(&id).map(|&i| &self.entries[i].asset)
    }

    /// Inserts or replaces one asset.
    pub fn upsert(&mut self, asset: Asset) {
        match self.by_id.get(&asset.id) {
            Some(&i) => self.entries[i] = Entry::new(asset),
            None => {
                self.by_id.insert(asset.id, self.entries.len());
                self.entries.push(Entry::new(asset));
            }
        }
    }

    pub fn search(&self, query: &Query) -> SearchResult {
        let text = query.text.to_lowercase();
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let required_tags: Vec<String> = query.tags.iter().map(|t| t.to_lowercase()).collect();

        let mut hits: Vec<(i32, usize)> = Vec::new();
        'entries: for (i, e) in self.entries.iter().enumerate() {
            if e.asset.missing && !query.include_missing {
                continue;
            }
            if query.folder_id.is_some_and(|f| f != e.asset.folder_id) {
                continue;
            }
            for tag in &required_tags {
                if !e.tags_lc.iter().any(|t| t == tag) {
                    continue 'entries;
                }
            }
            let mut score = 0;
            for token in &tokens {
                match score_token(token, e) {
                    Some(s) => score += s,
                    None => continue 'entries,
                }
            }
            hits.push((score, i));
        }

        let total = hits.len();
        hits.sort_unstable_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.entries[a.1].name_lc.cmp(&self.entries[b.1].name_lc))
                .then_with(|| self.entries[a.1].asset.id.cmp(&self.entries[b.1].asset.id))
        });
        if query.limit > 0 {
            hits.truncate(query.limit);
        }
        SearchResult {
            total,
            assets: hits
                .into_iter()
                .map(|(_, i)| self.entries[i].asset.clone())
                .collect(),
        }
    }
}

/// Scores one query word against an asset. `None` means no match.
fn score_token(token: &str, e: &Entry) -> Option<i32> {
    if let Some(tag) = token.strip_prefix('#') {
        return score_tags(tag, e);
    }
    let mut best: Option<i32> = None;

    if let Some(pos) = e.name_lc.find(token) {
        let at_word_start = pos == 0
            || e.name_lc[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric());
        let mut s = match (pos, at_word_start) {
            (0, _) => 120,
            (_, true) => 100,
            _ => 80,
        };
        if e.name_lc.len() == token.len() {
            s += 30;
        }
        best = best.max(Some(s));
    }
    best = best.max(score_tags(token, e));
    if e.dir_lc.contains(token) {
        best = best.max(Some(40));
    }
    // Fuzzy matching is the last resort, only when nothing matched literally.
    best.or_else(|| fuzzy(token, &e.name_lc))
}

fn score_tags(token: &str, e: &Entry) -> Option<i32> {
    if token.is_empty() {
        return (!e.tags_lc.is_empty()).then_some(50);
    }
    e.tags_lc
        .iter()
        .filter_map(|t| {
            if t == token {
                Some(110)
            } else if t.starts_with(token) {
                Some(90)
            } else if t.contains(token) {
                Some(70)
            } else {
                None
            }
        })
        .max()
}

/// Subsequence match ("tvrn" finds "tavern"), scored by how tightly the
/// letters sit together. Only used when nothing matched literally.
fn fuzzy(token: &str, name: &str) -> Option<i32> {
    let needle: Vec<char> = token.chars().collect();
    // Numbers are meant literally ("take 042" should not find "take 0412").
    if needle.len() < 3 || needle.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hay: Vec<char> = name.chars().collect();
    let max_span = needle.len() * 2 + 1;

    let mut best_span: Option<usize> = None;
    for start in 0..hay.len() {
        if hay[start] != needle[0] {
            continue;
        }
        let mut n = 1;
        let mut end = start;
        for (j, &c) in hay.iter().enumerate().skip(start + 1) {
            if n == needle.len() || j - start >= max_span {
                break;
            }
            if c == needle[n] {
                n += 1;
                end = j;
            }
        }
        if n == needle.len() {
            let span = end - start + 1;
            if best_span.is_none_or(|b| span < b) {
                best_span = Some(span);
            }
        }
    }
    best_span.map(|span| (50 - 5 * (span - needle.len()) as i32).max(10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisStatus, Tag};
    use std::time::Instant;

    fn asset(id: i64, name: &str, dir: &str, tags: &[&str]) -> Asset {
        Asset {
            id,
            folder_id: 1,
            path: format!("{dir}\\{name}.ogg"),
            name: name.into(),
            ext: "ogg".into(),
            size: 1,
            mtime: 1,
            hash: None,
            duration_ms: Some(1000),
            sample_rate: Some(48000),
            channels: Some(2),
            loudness_lufs: None,
            gain_db: 0.0,
            bpm: None,
            beat_offset_ms: None,
            status: AnalysisStatus::Ok,
            error: None,
            missing: false,
            source: "local".into(),
            license: None,
            attribution: None,
            trim_db: 0.0,
            always_loop: false,
            edit: None,
            tags: tags
                .iter()
                .map(|t| Tag {
                    name: (*t).into(),
                    category: "custom".into(),
                })
                .collect(),
        }
    }

    fn names(r: &SearchResult) -> Vec<&str> {
        r.assets.iter().map(|a| a.name.as_str()).collect()
    }

    fn q(text: &str) -> Query {
        Query {
            text: text.into(),
            ..Default::default()
        }
    }

    fn small() -> Index {
        Index::new(vec![
            asset(1, "Tavern Night", "D:\\Audio\\Ambience", &["tavern", "calm"]),
            asset(2, "Old Tavern Brawl", "D:\\Audio\\Music", &["combat"]),
            asset(3, "Forest Rain", "D:\\Audio\\Ambience", &["forest", "calm"]),
            asset(4, "Door Creak", "D:\\Audio\\SFX\\Tavern", &[]),
            asset(5, "Battle_Drums-Loop", "D:\\Audio\\Music", &["combat", "Tavern Fight"]),
        ])
    }

    #[test]
    fn empty_query_lists_everything_by_name() {
        let r = small().search(&q(""));
        assert_eq!(r.total, 5);
        assert_eq!(names(&r)[0], "Battle_Drums-Loop");
    }

    #[test]
    fn ranks_name_start_over_word_start_over_tag_and_folder() {
        let r = small().search(&q("tavern"));
        assert_eq!(
            names(&r),
            vec![
                "Tavern Night",      // name starts with it
                "Old Tavern Brawl",  // word inside the name
                "Battle_Drums-Loop", // tag starts with it
                "Door Creak",        // only the folder matches
            ]
        );
    }

    #[test]
    fn words_are_anded_and_case_is_ignored() {
        let r = small().search(&q("CALM rain"));
        assert_eq!(names(&r), vec!["Forest Rain"]);
        assert_eq!(small().search(&q("tavern forest")).total, 0);
    }

    #[test]
    fn separators_count_as_word_starts() {
        let r = small().search(&q("drums"));
        assert_eq!(names(&r), vec!["Battle_Drums-Loop"]);
    }

    #[test]
    fn hash_prefix_searches_tags_only() {
        let r = small().search(&q("#tavern"));
        assert_eq!(names(&r), vec!["Tavern Night", "Battle_Drums-Loop"]);
    }

    #[test]
    fn fuzzy_finds_skipped_letters_but_not_noise() {
        let idx = small();
        assert_eq!(names(&idx.search(&q("tvrn"))), vec!["Old Tavern Brawl", "Tavern Night"]);
        assert_eq!(names(&idx.search(&q("frst"))), vec!["Forest Rain"]);
        // Letters present but scattered across the whole name: not a match.
        assert_eq!(idx.search(&q("tnt")).total, 0);
        assert_eq!(idx.search(&q("zzz")).total, 0);
    }

    #[test]
    fn numbers_are_never_fuzzy() {
        let idx = Index::new(vec![
            asset(1, "take 0412", "D:\\Audio", &[]),
            asset(2, "take 042", "D:\\Audio", &[]),
        ]);
        assert_eq!(names(&idx.search(&q("042"))), vec!["take 042"]);
    }

    #[test]
    fn filters_by_folder_tags_and_missing() {
        let mut idx = small();
        let mut gone = asset(6, "Tavern Ghost", "D:\\Audio", &["calm"]);
        gone.missing = true;
        gone.folder_id = 2;
        idx.upsert(gone);

        assert_eq!(idx.search(&q("tavern")).total, 4);
        let r = idx.search(&Query {
            text: "tavern".into(),
            include_missing: true,
            ..Default::default()
        });
        assert_eq!(r.total, 5);

        let r = idx.search(&Query {
            tags: vec!["Calm".into()],
            ..Default::default()
        });
        assert_eq!(names(&r), vec!["Forest Rain", "Tavern Night"]);

        let r = idx.search(&Query {
            folder_id: Some(2),
            include_missing: true,
            ..Default::default()
        });
        assert_eq!(names(&r), vec!["Tavern Ghost"]);
    }

    #[test]
    fn limit_keeps_total() {
        let r = small().search(&Query {
            limit: 2,
            ..Default::default()
        });
        assert_eq!((r.total, r.assets.len()), (5, 2));
    }

    #[test]
    fn upsert_replaces_in_place() {
        let mut idx = small();
        idx.upsert(asset(3, "Jungle Rain", "D:\\Audio", &[]));
        assert_eq!(idx.len(), 5);
        assert_eq!(idx.search(&q("forest")).total, 0);
        assert_eq!(idx.search(&q("jungle")).total, 1);
    }

    /// Milestone 1 acceptance: search under 50 ms on a 10,000-file library.
    #[test]
    fn ten_thousand_files_search_under_50ms() {
        let words = [
            "tavern", "forest", "battle", "rain", "dungeon", "market", "storm", "cave", "dragon",
            "river", "night", "camp", "temple", "crypt", "harbor", "wind",
        ];
        let assets: Vec<Asset> = (0..10_000i64)
            .map(|i| {
                let a = words[(i % 16) as usize];
                let b = words[((i / 16) % 16) as usize];
                let c = words[((i / 256) % 16) as usize];
                asset(
                    i,
                    &format!("{a}_{b} {c} take {i:05}"),
                    &format!("D:\\Audio\\Pack {}\\{c}", i % 40),
                    &[a, c],
                )
            })
            .collect();
        let idx = Index::new(assets);

        let queries = [
            "",
            "tavern",
            "dragon cave",
            "#storm night",
            "tvrn",
            "take 0420",
            "zzzzqq",
            "t",
            "rain forest market 1",
        ];
        let mut worst = 0u128;
        for text in queries {
            let query = Query {
                text: text.into(),
                limit: 200,
                ..Default::default()
            };
            let started = Instant::now();
            let r = idx.search(&query);
            let micros = started.elapsed().as_micros();
            worst = worst.max(micros);
            println!("{text:>24?}: {:>5} hits in {micros} µs", r.total);
        }
        assert!(worst < 50_000, "slowest search took {worst} µs");
    }
}
