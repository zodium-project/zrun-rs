/// Built-in fuzzy scorer — no external deps, no fzf required.
///
/// Strategy:
///   1. All query chars must appear in the target in order (subsequence match).
///   2. Score bonuses for:
///      - Consecutive character runs
///      - Match at word boundary (after `_`, `-`, `.`, start)
///      - Match at start of string
///   3. Length penalty: prefer shorter targets for equal matches.
///
/// Returns `None` if the query is not a subsequence of the target.
/// Returns `Some(score)` where higher is better.

pub fn score(query: &str, target: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();

    let mut qi = 0usize; // query index
    let mut ti = 0usize; // target index

    // Store match positions for scoring
    let mut positions: Vec<usize> = Vec::with_capacity(q.len());

    while qi < q.len() && ti < t.len() {
        if q[qi] == t[ti] {
            positions.push(ti);
            qi += 1;
        }
        ti += 1;
    }

    if qi < q.len() {
        return None; // not all query chars matched
    }

    let mut score: i32 = 0;

    for (i, &pos) in positions.iter().enumerate() {
        // Start-of-string bonus
        if pos == 0 {
            score += 16;
        }

        // Word boundary bonus
        if pos > 0 {
            let prev = t[pos - 1];
            if prev == '_' || prev == '-' || prev == '.' || prev == '/' {
                score += 10;
            }
        }

        // Consecutive match bonus
        if i > 0 && pos == positions[i - 1] + 1 {
            score += 8;
        }

        // Base match
        score += 1;
    }

    // Length penalty — prefer shorter targets
    score -= (t.len() as i32 - q.len() as i32) / 4;

    Some(score)
}

/// Filter and rank a list of scripts by fuzzy score against `query`.
/// Returns indices into the original slice, sorted best-first.
pub fn rank<'a>(
    query: &str,
    items: &'a [crate::scripts::Script],
) -> Vec<(usize, i32)> {
    if query.is_empty() {
        return items.iter().enumerate().map(|(i, _)| (i, 0)).collect();
    }

    let mut results: Vec<(usize, i32)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            // Score against name (primary) and description (secondary)
            let name_score  = score(query, &s.name);
            let desc_score  = score(query, &s.description).map(|s| s / 2);
            let tag_score   = s.tags.iter()
                .filter_map(|t| score(query, t))
                .map(|s| s / 3)
                .max();

            // Best of name / desc / tags
            let best = [name_score, desc_score, tag_score]
                .into_iter()
                .flatten()
                .max();

            best.map(|sc| (i, sc))
        })
        .collect();

    results.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    results
}

/// Collect the match positions for highlighting in the UI.
pub fn match_positions(query: &str, target: &str) -> Vec<usize> {
    if query.is_empty() {
        return vec![];
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();

    let mut positions = Vec::new();
    let mut qi = 0;

    for (ti, tc) in t.iter().enumerate() {
        if qi < q.len() && *tc == q[qi] {
            positions.push(ti);
            qi += 1;
        }
    }
    positions
}