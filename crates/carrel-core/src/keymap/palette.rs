//! Small, bounded fuzzy matching for named commands and sources.

/// Score an in-order character match; lower scores prefer starts and contiguous text.
/// Empty queries match everything. Overlong queries and missing characters do not match.
pub fn fuzzy_score(query: &str, label: &str) -> Option<usize> {
    let query: Vec<_> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .take(129)
        .collect();
    if query.len() > 128 {
        return None;
    }
    let label: Vec<_> = label.chars().flat_map(char::to_lowercase).collect();
    let mut position = 0;
    let mut score = 0;
    for character in query {
        let next = label[position..]
            .iter()
            .position(|candidate| *candidate == character)?
            + position;
        score += (next - position) * 4;
        if next > 0 && label[next - 1].is_alphanumeric() && next != position {
            score += 2;
        }
        position = next + 1;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_matches_are_ordered_case_insensitive_and_unicode_safe() {
        assert_eq!(fuzzy_score("", "Reading"), Some(0));
        assert!(fuzzy_score("rdg", "Reading").is_some());
        assert_eq!(fuzzy_score("READ", "Reading"), Some(0));
        assert!(fuzzy_score("café", "CAFÉ reader").is_some());
        assert!(fuzzy_score("dg r", "Reading").is_none());
        assert!(fuzzy_score("x", "").is_none());
        assert!(fuzzy_score(&"a".repeat(129), &"a".repeat(200)).is_none());
        assert!(
            fuzzy_score("read", "Reading").unwrap() < fuzzy_score("read", "Mark read").unwrap()
        );
    }
}
