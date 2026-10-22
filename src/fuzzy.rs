use std::collections::{HashMap, HashSet};
use strsim::jaro_winkler;

use crate::models::{DuplicateGroup, IgnoredPair, Payee, PayeeCandidate};

/// Normalize and clean a payee name for fuzzy comparison.
pub fn normalize_payee_name(name: &str) -> String {
    let mut cleaned = name.to_lowercase();

    // Replace punctuation with spaces
    cleaned = cleaned
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else {
                ' '
            }
        })
        .collect();

    // Noise words to strip
    let noise_words: HashSet<&'static str> = [
        "inc", "llc", "ltd", "corp", "co", "com", "net", "org",
        "store", "shop", "market", "mktp", "pos", "debit", "checkcard",
        "sq", "the", "and",
    ]
    .into_iter()
    .collect();

    // Filter tokens: strip noise words and isolated numbers
    let tokens: Vec<String> = cleaned
        .split_whitespace()
        .filter(|token| {
            if token.is_empty() {
                return false;
            }
            // Strip pure numeric tokens (e.g. store numbers like 1234, 052)
            if token.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            if noise_words.contains(token) {
                return false;
            }
            true
        })
        .map(|s| s.to_string())
        .collect();

    if tokens.is_empty() {
        // Fallback to original lowercased alphanumeric characters if everything was stripped
        return cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    }

    tokens.join(" ")
}

/// Generate a sorted token string (e.g. "depot home" for "Home Depot").
pub fn token_sort_key(name: &str) -> String {
    let mut tokens: Vec<&str> = name.split_whitespace().collect();
    tokens.sort_unstable();
    tokens.join(" ")
}

/// Score a payee name for cleanliness. Higher score = cleaner name.
pub fn score_cleanliness(name: &str) -> i32 {
    let mut score = 100;
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return -1000;
    }

    // Penalize all uppercase (shouting)
    let has_lower = trimmed.chars().any(|c| c.is_lowercase());
    let has_upper = trimmed.chars().any(|c| c.is_uppercase());
    if has_upper && !has_lower && trimmed.len() > 3 {
        score -= 25;
    }

    // Penalize digits (store numbers, card digits, dates)
    let digit_count = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    score -= (digit_count as i32) * 8;

    // Penalize special noise characters (*, #, @, /, -)
    for c in trimmed.chars() {
        if matches!(c, '*' | '#' | '@' | '/' | '\\' | '_' | '|') {
            score -= 15;
        }
    }

    // Prefer reasonable length (5 - 25 chars)
    let len = trimmed.len();
    if len < 3 {
        score -= 20;
    } else if len > 35 {
        score -= (len as i32 - 35) * 2;
    }

    // Reward Title Case
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let title_cased = words.iter().all(|w| {
        let mut chars = w.chars();
        match chars.next() {
            Some(first) => first.is_uppercase() && chars.all(|c| !c.is_uppercase() || !c.is_alphabetic()),
            None => true,
        }
    });
    if title_cased && !words.is_empty() {
        score += 15;
    }

    score
}

/// Pick the cleanest candidate name from a slice of payees.
pub fn pick_cleanest_name(payees: &[Payee]) -> String {
    payees
        .iter()
        .max_by_key(|p| score_cleanliness(&p.name))
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

/// Disjoint-set (Union-Find) structure for clustering indices.
struct UnionFind {
    parent: Vec<usize>,
    members: Vec<Vec<usize>>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            members: (0..size).map(|i| vec![i]).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        if self.parent[i] == i {
            i
        } else {
            let root = self.find(self.parent[i]);
            self.parent[i] = root;
            root
        }
    }

    fn try_union(
        &mut self,
        i: usize,
        j: usize,
        eligible_payees: &[&Payee],
        ignored_pair_ids: &HashSet<String>,
    ) -> bool {
        let root_i = self.find(i);
        let root_j = self.find(j);
        if root_i == root_j {
            return true;
        }

        // Check if any member of root_i is paired with any member of root_j as an ignored pair
        if !ignored_pair_ids.is_empty() {
            for &m_i in &self.members[root_i] {
                for &m_j in &self.members[root_j] {
                    let pair_id = IgnoredPair::make_id(&eligible_payees[m_i].id, &eligible_payees[m_j].id);
                    if ignored_pair_ids.contains(&pair_id) {
                        return false;
                    }
                }
            }
        }

        // Merge root_i into root_j
        self.parent[root_i] = root_j;
        let mut moved_members = std::mem::take(&mut self.members[root_i]);
        self.members[root_j].append(&mut moved_members);
        true
    }
}

/// Find duplicate payee groups using multi-tier fuzzy matching.
/// `similarity_threshold` is typically 0.85 (balanced) or between 0.80 and 0.95.
pub fn find_duplicate_groups(
    payees: &[Payee],
    similarity_threshold: f64,
    ignored_pair_ids: &HashSet<String>,
) -> Vec<DuplicateGroup> {
    // Filter out deleted payees and transfer payees
    let eligible_payees: Vec<&Payee> = payees
        .iter()
        .filter(|p| !p.deleted && p.transfer_account_id.as_deref().unwrap_or("").trim().is_empty())
        .collect();

    let n = eligible_payees.len();
    if n < 2 {
        return Vec::new();
    }

    // Precompute normalized strings and token-sort keys
    let normalized_names: Vec<String> = eligible_payees
        .iter()
        .map(|p| normalize_payee_name(&p.name))
        .collect();

    let token_keys: Vec<String> = normalized_names
        .iter()
        .map(|s| token_sort_key(s))
        .collect();

    let mut uf = UnionFind::new(n);
    let mut match_reasons: HashMap<(usize, usize), String> = HashMap::new();

    // 1. Bucket by token-sort key (Tier 1: Exact normalized match)
    let mut token_buckets: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, key) in token_keys.iter().enumerate() {
        if !key.trim().is_empty() {
            token_buckets.entry(key.as_str()).or_default().push(i);
        }
    }

    for indices in token_buckets.values() {
        if indices.len() > 1 {
            for a in 0..indices.len() {
                for b in (a + 1)..indices.len() {
                    let i = indices[a];
                    let j = indices[b];
                    if uf.try_union(i, j, &eligible_payees, ignored_pair_ids) {
                        match_reasons.insert((i.min(j), i.max(j)), "Exact Normalized Match".to_string());
                    }
                }
            }
        }
    }

    // 2. Fuzzy / Prefix comparison with blocking
    // Block by first 2 characters of normalized string
    let mut prefix_buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, norm) in normalized_names.iter().enumerate() {
        let prefix: String = norm.chars().take(2).collect();
        if !prefix.is_empty() {
            prefix_buckets.entry(prefix).or_default().push(i);
        }
    }

    for indices in prefix_buckets.values() {
        let count = indices.len();
        // If a prefix bucket is unusually large (e.g. > 300), compare first 50 or use higher threshold
        for a in 0..count {
            for b in (a + 1)..count {
                let i = indices[a];
                let j = indices[b];

                let norm_i = &normalized_names[i];
                let norm_j = &normalized_names[j];

                if norm_i.is_empty() || norm_j.is_empty() {
                    continue;
                }

                // Check substring / prefix containment
                let is_prefix = (norm_i.len() >= 4 && norm_j.starts_with(norm_i))
                    || (norm_j.len() >= 4 && norm_i.starts_with(norm_j));

                if is_prefix {
                    if uf.try_union(i, j, &eligible_payees, ignored_pair_ids) {
                        match_reasons.insert((i.min(j), i.max(j)), "Prefix / Substring Match".to_string());
                    }
                    continue;
                }

                // Jaro-Winkler similarity
                let sim = jaro_winkler(norm_i, norm_j);
                if sim >= similarity_threshold {
                    if uf.try_union(i, j, &eligible_payees, ignored_pair_ids) {
                        let pct = (sim * 100.0).round() as u32;
                        match_reasons.insert((i.min(j), i.max(j)), format!("Fuzzy Match ({}%)", pct));
                    }
                }
            }
        }
    }

    // Group items by their root in UnionFind
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        clusters.entry(root).or_default().push(i);
    }

    // Build DuplicateGroup list for clusters with >= 2 payees
    let mut duplicate_groups = Vec::new();
    for (_, member_indices) in clusters {
        if member_indices.len() < 2 {
            continue;
        }

        let group_payees: Vec<Payee> = member_indices
            .iter()
            .map(|&idx| (*eligible_payees[idx]).clone())
            .collect();

        let suggested_target_name = pick_cleanest_name(&group_payees);

        // Find primary match reason among pairs
        let mut reason = "Fuzzy Duplicate Group".to_string();
        for a in 0..member_indices.len() {
            for b in (a + 1)..member_indices.len() {
                let pair = (member_indices[a].min(member_indices[b]), member_indices[a].max(member_indices[b]));
                if let Some(r) = match_reasons.get(&pair) {
                    reason = r.clone();
                    break;
                }
            }
        }

        let candidates: Vec<PayeeCandidate> = group_payees
            .into_iter()
            .map(|payee| PayeeCandidate {
                payee,
                is_checked: true, // User requirement: Prechecked by default!
            })
            .collect();

        let group_id = uuid::Uuid::new_v4().to_string();

        duplicate_groups.push(DuplicateGroup {
            id: group_id,
            suggested_target_name,
            candidates,
            match_reason: reason,
        });
    }

    // Sort groups by number of candidates descending
    duplicate_groups.sort_by(|a, b| b.candidates.len().cmp(&a.candidates.len()));

    duplicate_groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization() {
        assert_eq!(normalize_payee_name("AMAZON.COM*12345"), "amazon");
        assert_eq!(normalize_payee_name("Starbucks Store #0421"), "starbucks");
        assert_eq!(normalize_payee_name("Trader Joe's Inc."), "trader joe s");
    }

    #[test]
    fn test_score_cleanliness() {
        let clean = score_cleanliness("Amazon");
        let noisy = score_cleanliness("AMAZON.COM*987213");
        assert!(clean > noisy);

        let clean_starbucks = score_cleanliness("Starbucks");
        let noisy_starbucks = score_cleanliness("STARBUCKS #1203");
        assert!(clean_starbucks > noisy_starbucks);
    }

    #[test]
    fn test_find_duplicate_groups() {
        let payees = vec![
            Payee {
                id: "1".into(),
                name: "Amazon".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "2".into(),
                name: "AMAZON.COM*123".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "3".into(),
                name: "Starbucks".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "4".into(),
                name: "Starbucks Store #412".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "5".into(),
                name: "Target".into(),
                transfer_account_id: None,
                deleted: false,
            },
        ];

        let ignored = HashSet::new();
        let groups = find_duplicate_groups(&payees, 0.85, &ignored);
        assert_eq!(groups.len(), 2);
        for group in &groups {
            assert!(group.candidates.iter().all(|c| c.is_checked));
        }
    }

    #[test]
    fn test_find_duplicate_groups_with_ignored_pairs() {
        let payees = vec![
            Payee {
                id: "1".into(),
                name: "Amazon".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "2".into(),
                name: "Amazon Kids".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "3".into(),
                name: "Amazon Prime".into(),
                transfer_account_id: None,
                deleted: false,
            },
            Payee {
                id: "4".into(),
                name: "Amazon Retail".into(),
                transfer_account_id: None,
                deleted: false,
            },
        ];

        let mut ignored = HashSet::new();
        // Ignore pairs (1, 2), (1, 3), (2, 3)
        ignored.insert(IgnoredPair::make_id("1", "2"));
        ignored.insert(IgnoredPair::make_id("1", "3"));
        ignored.insert(IgnoredPair::make_id("2", "3"));

        let groups = find_duplicate_groups(&payees, 0.85, &ignored);
        // Amazon (1), Amazon Kids (2), and Amazon Prime (3) should NOT cluster with each other!
        for g in &groups {
            let ids: HashSet<String> = g.candidates.iter().map(|c| c.payee.id.clone()).collect();
            assert!(!(ids.contains("1") && ids.contains("2")));
            assert!(!(ids.contains("1") && ids.contains("3")));
            assert!(!(ids.contains("2") && ids.contains("3")));
        }
    }
}
