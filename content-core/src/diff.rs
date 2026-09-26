//! Text and word diffing for post edit history, note revisions, and content audit logs.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// Type of change in a text diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffTag {
    /// Content present in both versions.
    Equal,
    /// Content added in the new version.
    Insert,
    /// Content removed from the old version.
    Delete,
}

impl From<ChangeTag> for DiffTag {
    fn from(tag: ChangeTag) -> Self {
        match tag {
            ChangeTag::Equal => DiffTag::Equal,
            ChangeTag::Insert => DiffTag::Insert,
            ChangeTag::Delete => DiffTag::Delete,
        }
    }
}

/// A single change segment in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffItem {
    pub tag: DiffTag,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_index: Option<usize>,
}

/// Aggregated diff statistics comparing two versions of a post.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Number of inserted units (lines or words).
    pub insertions: usize,
    /// Number of deleted units.
    pub deletions: usize,
    /// Number of unchanged units.
    pub unchanged: usize,
    /// Overall similarity ratio between 0.0 (completely different) and 1.0 (identical).
    pub similarity_ratio: f32,
}

/// Compute line-by-line diff between two post versions.
pub fn compute_line_diff(old_text: &str, new_text: &str) -> Vec<DiffItem> {
    let diff = TextDiff::from_lines(old_text, new_text);
    diff.iter_all_changes()
        .map(|change| DiffItem {
            tag: change.tag().into(),
            value: change.value().to_string(),
            old_index: change.old_index(),
            new_index: change.new_index(),
        })
        .collect()
}

/// Compute word-by-word diff between two short text strings (e.g. status updates or titles).
pub fn compute_word_diff(old_text: &str, new_text: &str) -> Vec<DiffItem> {
    let diff = TextDiff::from_words(old_text, new_text);
    diff.iter_all_changes()
        .map(|change| DiffItem {
            tag: change.tag().into(),
            value: change.value().to_string(),
            old_index: change.old_index(),
            new_index: change.new_index(),
        })
        .collect()
}

/// Compute high-level diff statistics comparing two versions.
pub fn compute_diff_summary(old_text: &str, new_text: &str) -> DiffSummary {
    let diff = TextDiff::from_lines(old_text, new_text);
    let mut insertions = 0;
    let mut deletions = 0;
    let mut unchanged = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => insertions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => unchanged += 1,
        }
    }

    DiffSummary {
        insertions,
        deletions,
        unchanged,
        similarity_ratio: diff.ratio(),
    }
}

/// Render a standard unified diff header and hunks.
pub fn render_unified_diff(old_text: &str, new_text: &str, context_lines: usize) -> String {
    let diff = TextDiff::from_lines(old_text, new_text);
    diff.unified_diff()
        .context_radius(context_lines)
        .header("original", "edited")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_text_diff() {
        let text = "Hello world\nThis is Soshal.\n";
        let diff = compute_line_diff(text, text);
        assert!(diff.iter().all(|d| d.tag == DiffTag::Equal));

        let summary = compute_diff_summary(text, text);
        assert_eq!(summary.insertions, 0);
        assert_eq!(summary.deletions, 0);
        assert_eq!(summary.unchanged, 2);
        assert!((summary.similarity_ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_word_diff_edits() {
        let old = "The quick brown fox";
        let new = "The fast brown fox";
        let diff = compute_word_diff(old, new);

        let tags: Vec<DiffTag> = diff.iter().map(|d| d.tag).collect();
        assert!(tags.contains(&DiffTag::Delete));
        assert!(tags.contains(&DiffTag::Insert));
        assert!(tags.contains(&DiffTag::Equal));

        let deleted_word = diff.iter().find(|d| d.tag == DiffTag::Delete).unwrap();
        assert_eq!(deleted_word.value, "quick");

        let inserted_word = diff.iter().find(|d| d.tag == DiffTag::Insert).unwrap();
        assert_eq!(inserted_word.value, "fast");
    }

    #[test]
    fn test_line_diff_insertion_deletion() {
        let old = "line 1\nline 2\n";
        let new = "line 1\nline 2 edited\nline 3\n";
        let diff = compute_line_diff(old, new);

        assert_eq!(diff.len(), 4);
        assert_eq!(diff[0].tag, DiffTag::Equal);
        assert_eq!(diff[1].tag, DiffTag::Delete);
        assert_eq!(diff[2].tag, DiffTag::Insert);
        assert_eq!(diff[3].tag, DiffTag::Insert);
    }

    #[test]
    fn test_unified_diff_rendering() {
        let old = "alpha\nbeta\n";
        let new = "alpha\ngamma\n";
        let unified = render_unified_diff(old, new, 3);
        assert!(unified.contains("--- original"));
        assert!(unified.contains("+++ edited"));
        assert!(unified.contains("-beta"));
        assert!(unified.contains("+gamma"));
    }

    #[test]
    fn test_serde_diff_item() {
        let item = DiffItem {
            tag: DiffTag::Insert,
            value: "new line\n".to_string(),
            old_index: None,
            new_index: Some(2),
        };
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("\"tag\":\"insert\""));
        assert!(!json.contains("old_index"));

        let roundtrip: DiffItem = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip, item);
    }
}
