//! Shared regex-compilation helpers for the pattern matchers (spam, csam,
//! gore, moderation words, glitter sanitizer).
//!
//! Every matcher used to inline its own `RegexSetBuilder` with its own size
//! limit; these two builders are the single source of that shape and of the
//! DFA cache budget.

/// Memory budget (bytes) for a single compiled regex program / DFA cache.
pub(crate) const REGEX_SIZE_LIMIT: usize = 1 << 30;

/// Build a case-insensitive [`regex::RegexSet`] from `patterns`, falling back
/// to an empty set (matches nothing) when a pattern fails to compile or
/// exceeds the size budget — the same failure mode the old per-module code
/// used, so a hostile pattern can't disable moderation by panicking.
pub(crate) fn build_regex_set<I, S>(patterns: I) -> regex::RegexSet
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    regex::RegexSetBuilder::new(patterns)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
        .unwrap_or_else(|_| regex::RegexSet::empty())
}

/// Build a single case-insensitive [`regex::Regex`] bounded by the shared
/// size budget. On failure it degrades to a never-matching pattern rather
/// than refusing to compile.
pub(crate) fn build_regex(pat: &str) -> regex::Regex {
    regex::RegexBuilder::new(pat)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
        .unwrap_or_else(|_| regex::Regex::new(r"^$").expect("fallback regex must compile"))
}
