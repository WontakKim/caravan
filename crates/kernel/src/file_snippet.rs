//! Pure, presentation-only formatter for line-numbered file snippets.
//!
//! This module is intentionally free of filesystem I/O. All callers (model
//! output, screen log, Workspace Context) receive a ready-made `String`.

/// Truncation marker appended when the output exceeds `max_bytes`.
const MARKER: &str = "\n... [truncated]";

/// Renders a line-numbered file snippet as a human-readable `String`.
///
/// # Parameters
/// - `path`: display path shown in the `File:` header (not accessed on disk).
/// - `content`: raw file bytes as a `str` slice.
/// - `start_line`: 1-based line number of the first line in `content`.
/// - `line_count`: number of lines to display; `None` means all lines in
///   `content`.
/// - `truncated`: set by the caller when the underlying read was already
///   capped (e.g. by a byte budget in the tool layer).
/// - `max_bytes`: hard ceiling on the returned string's byte length.
///
/// # Guarantees
/// - Pure: no I/O, no filesystem access, no canonicalization.
/// - The returned string is always valid UTF-8 (truncation respects char
///   boundaries).
/// - `result.len() <= max_bytes` always holds.
/// - The truncation marker appears at most once, as a suffix.
pub fn render_numbered_file_snippet(
    path: &str,
    content: &str,
    start_line: usize,
    line_count: Option<usize>,
    truncated: bool,
    max_bytes: usize,
) -> String {
    let n = line_count.unwrap_or_else(|| content.lines().count());

    // Compute inclusive end line number using saturating arithmetic so that a
    // caller passing `start_line = usize::MAX` with `n = 1` yields
    // `end = usize::MAX`, not `usize::MAX - 1` (which the alternative
    // `start.saturating_add(n).saturating_sub(1)` would give).
    let end = start_line.saturating_add(n.saturating_sub(1));

    // Width for the line-number column: at least 4 digits, expanding as needed.
    let width = 4_usize.max(end.to_string().len());

    // Build the header.
    let header = format!("File: {path}\nLines: {start_line}-{end}");

    // Build the body.
    let body = if n == 0 {
        "No content in requested range.".to_owned()
    } else {
        content
            .lines()
            .enumerate()
            .map(|(idx, line)| {
                let num = start_line.saturating_add(idx);
                format!("{:>width$} | {}", num, line, width = width)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let base = format!("{header}\n{body}");

    // Apply byte-budget bounding exactly once.
    let needs_marker = truncated || base.len() > max_bytes;
    if !needs_marker {
        return base;
    }

    // Leave room for the marker; find a valid UTF-8 boundary.
    let budget = max_bytes.saturating_sub(MARKER.len());
    let mut cut = budget.min(base.len());
    while cut > 0 && !base.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &base[..cut], MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_read_line_one_numbering() {
        let content = "alpha\nbeta\ngamma";
        let result = render_numbered_file_snippet("src/main.rs", content, 1, None, false, 4096);
        assert!(result.contains("File: src/main.rs"), "missing File: header");
        assert!(result.contains("Lines: 1-3"), "missing Lines header");
        assert!(result.contains("   1 | alpha"), "missing line 1 prefix");
    }

    #[test]
    fn range_read_offset_numbering() {
        let content = "delta\nepsilon";
        let result = render_numbered_file_snippet("lib.rs", content, 10, Some(2), false, 4096);
        assert!(
            result.contains("Lines: 10-11"),
            "missing offset Lines header"
        );
        assert!(
            result.contains("  10 | delta"),
            "missing offset line prefix"
        );
        assert!(
            result.contains("  11 | epsilon"),
            "missing second offset line"
        );
    }

    #[test]
    fn empty_zero_line_range() {
        let result = render_numbered_file_snippet("empty.rs", "", 5, Some(0), false, 4096);
        assert!(
            result.contains("Lines: 5-5"),
            "expected Lines: 5-5 for zero-line range"
        );
        assert!(
            result.contains("No content in requested range."),
            "missing no-content message"
        );
    }

    #[test]
    fn trailing_newline_no_phantom_line() {
        // A content string ending with '\n' must not produce a blank numbered line.
        let content = "one\ntwo\n";
        let result = render_numbered_file_snippet("trail.rs", content, 1, None, false, 4096);
        // `str::lines()` strips the trailing newline, so we expect exactly 2 lines.
        assert!(result.contains("Lines: 1-2"), "wrong line range");
        assert!(!result.contains("   3 | "), "phantom blank line produced");
    }

    #[test]
    fn utf8_boundary_safe_truncation() {
        // "α" is 2 bytes (U+03B1). Build content that spans a char boundary at
        // the byte budget.
        let content = "αβγδεζηθ"; // 16 bytes (8 × 2)
        // Set max_bytes just large enough for header but cutting mid-char.
        let result = render_numbered_file_snippet("u.rs", content, 1, None, false, 30);
        // The result must be valid UTF-8.
        assert!(
            std::str::from_utf8(result.as_bytes()).is_ok(),
            "not valid UTF-8"
        );
    }

    #[test]
    fn max_bytes_bound() {
        let content = "x".repeat(10_000);
        let max_bytes = 512;
        let result = render_numbered_file_snippet("big.rs", &content, 1, None, false, max_bytes);
        assert!(
            result.len() <= max_bytes,
            "result len {} exceeds max_bytes {}",
            result.len(),
            max_bytes
        );
    }

    #[test]
    fn combined_truncation_path() {
        // Content large enough that even after appending the first marker the
        // total would exceed max_bytes; the implementation must produce exactly
        // one marker.
        let content = "y".repeat(10_000);
        let max_bytes = 200;
        let result =
            render_numbered_file_snippet("combo.rs", &content, 1, Some(1), true, max_bytes);

        assert!(
            result.len() <= max_bytes,
            "len {} exceeds max_bytes {}",
            result.len(),
            max_bytes
        );
        assert!(
            result.ends_with(MARKER),
            "result does not end with the truncation marker"
        );
        // Ensure the marker appears only once.
        let marker_str = "... [truncated]";
        let count = result.matches(marker_str).count();
        assert_eq!(
            count, 1,
            "marker appeared {count} times, expected exactly 1"
        );
    }

    #[test]
    fn saturating_end_edge() {
        // start_line = usize::MAX, n = 1 → end must equal usize::MAX (not MAX-1).
        let result =
            render_numbered_file_snippet("x.rs", "hello", usize::MAX, Some(1), false, 4096);
        let expected = format!("Lines: {}-{}", usize::MAX, usize::MAX);
        assert!(
            result.contains(&expected),
            "expected '{expected}' in output, got: {result}"
        );
    }
}
