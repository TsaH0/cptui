//! Output comparison (judging) logic.
//!
//! The default judge normalizes line endings, trailing whitespace and trailing
//! blank lines, then compares token-for-token. Different meaningful content
//! still fails; only cosmetic whitespace differences pass.

use crate::model::Verdict;

/// Comparison mode. Only the default `Whitespace` mode is implemented; the others
/// are extension points for future checkers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum JudgeMode {
    /// Exact byte comparison (after normalization).
    Exact,
    /// Token comparison ignoring inter-token whitespace (the robust default).
    #[default]
    Whitespace,
    /// Floating-point comparison (not implemented).
    Float { epsilon: f64 },
    /// Custom checker (not implemented).
    Custom,
}

/// Normalize a piece of program/judge output for comparison.
///
/// * CRLF and CR are converted to LF.
/// * Trailing whitespace on each line is removed.
/// * Trailing blank lines are removed.
/// * A single trailing newline is ensured so the "no trailing newline" case is
///   treated as equal to a trailing newline.
pub fn normalize(s: &str) -> String {
    // Normalize line endings.
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    // Split into lines (keeping empty lines), trim trailing whitespace per line.
    let mut lines: Vec<String> = s.split('\n').map(|l| l.trim_end().to_string()).collect();
    // If the input ended with a newline, split produces a trailing "" which we
    // keep for now; remove trailing blank lines below.
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Token-wise comparison: split on any run of whitespace and compare tokens in
/// order. This is the common competitive-programming "ignore extra spaces /
/// blank lines" judge.
fn tokens_equal(a: &str, b: &str) -> bool {
    let ta: Vec<&str> = a.split_whitespace().collect();
    let tb: Vec<&str> = b.split_whitespace().collect();
    ta == tb
}

/// Compare expected vs actual output and return a verdict.
pub fn compare(expected: &str, actual: &str) -> Verdict {
    let e = normalize(expected);
    let a = normalize(actual);
    if e == a || tokens_equal(&e, &a) {
        Verdict::Ac
    } else {
        Verdict::Wa
    }
}

/// Produce a unified-ish diff of expected vs actual for display.
pub fn diff(expected: &str, actual: &str) -> Vec<String> {
    let e = normalize(expected);
    let a = normalize(actual);
    let el: Vec<&str> = e.split('\n').collect();
    let al: Vec<&str> = a.split('\n').collect();
    let mut out = Vec::new();
    let n = el.len().max(al.len());
    for i in 0..n {
        let eline = el.get(i).copied().unwrap_or("");
        let aline = al.get(i).copied().unwrap_or("");
        if eline == aline {
            out.push(format!("  {eline}"));
        } else {
            if !eline.is_empty() {
                out.push(format!("- {eline}"));
            }
            if !aline.is_empty() {
                out.push(format!("+ {aline}"));
            }
        }
    }
    if out.is_empty() {
        out.push("(no differences)".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(expected: &str, actual: &str) -> Verdict {
        compare(expected, actual)
    }

    #[test]
    fn exact_match() {
        assert_eq!(v("15\n", "15\n"), Verdict::Ac);
        assert_eq!(v("1 2 3", "1 2 3"), Verdict::Ac);
    }

    #[test]
    fn crlf_vs_lf() {
        assert_eq!(v("15\r\n", "15\n"), Verdict::Ac);
        assert_eq!(v("a\r\nb\r\n", "a\nb\n"), Verdict::Ac);
    }

    #[test]
    fn trailing_whitespace() {
        assert_eq!(v("15   \n", "15\n"), Verdict::Ac);
        assert_eq!(v("1 2   3", "1 2 3"), Verdict::Ac);
    }

    #[test]
    fn trailing_blank_lines() {
        assert_eq!(v("15\n\n\n", "15\n"), Verdict::Ac);
        assert_eq!(v("15", "15\n\n"), Verdict::Ac);
    }

    #[test]
    fn no_trailing_newline() {
        assert_eq!(v("15", "15\n"), Verdict::Ac);
        assert_eq!(v("15\n", "15"), Verdict::Ac);
    }

    #[test]
    fn wrong_output() {
        assert_eq!(v("15\n", "14\n"), Verdict::Wa);
    }

    #[test]
    fn multiline_difference() {
        assert_eq!(v("1\n2\n3\n", "1\n2\n4\n"), Verdict::Wa);
    }

    #[test]
    fn token_order_and_extra_spaces() {
        assert_eq!(v("1 2 3", "1  2  3"), Verdict::Ac);
        assert_eq!(v("1 2 3", "1\t2\t3"), Verdict::Ac);
    }

    #[test]
    fn meaningful_difference_fails() {
        assert_eq!(v("1 2 3", "3 2 1"), Verdict::Wa);
        assert_eq!(v("abc", "abc def"), Verdict::Wa);
    }

    #[test]
    fn empty_outputs() {
        assert_eq!(v("", ""), Verdict::Ac);
        assert_eq!(v("\n", ""), Verdict::Ac);
        assert_eq!(v("", "x"), Verdict::Wa);
    }
}
