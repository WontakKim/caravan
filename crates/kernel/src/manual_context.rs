//! User-attached read-only tool output for prompt context injection.

pub const MANUAL_TOOL_CONTEXT_MAX_BYTES: usize = 4 * 1024;

/// A snapshot of tool output attached by the user for inclusion in the
/// prompt's Context section.
///
/// `Clone` is required because T-5 copies `last_tool_output_candidate`
/// into `pending_manual_tool_context`.
#[derive(Clone, Debug)]
pub struct ManualToolContext {
    pub source: String,
    pub content: String,
    pub truncated: bool,
}

impl ManualToolContext {
    /// Builds a context from a `read_file` tool result.
    ///
    /// The stored `content` (including any truncation marker) is at most
    /// [`MANUAL_TOOL_CONTEXT_MAX_BYTES`] bytes and is always aligned to a
    /// valid UTF-8 char boundary.
    pub fn from_read_file(path: &str, content: &str) -> Self {
        let source = format!("tool=read_file path=\"{path}\"");
        let marker = "\n... [truncated]";

        if content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES {
            return Self {
                source,
                content: content.to_string(),
                truncated: false,
            };
        }

        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len();
        // Retreat to a valid UTF-8 char boundary using a backward scan,
        // mirroring the pattern in `crates/tui/src/app.rs push_tool_read_output`.
        let mut cut = budget;
        while cut > 0 && !content.is_char_boundary(cut) {
            cut -= 1;
        }
        let truncated_content = format!("{}{}", &content[..cut], marker);

        Self {
            source,
            content: truncated_content,
            truncated: true,
        }
    }

    /// Builds a context from a `list_files` tool result.
    ///
    /// Entries are appended as `"- <entry>\n"` lines until the next line
    /// would exceed the budget (`MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len()`).
    /// When not all entries fit the truncation marker is appended instead.
    pub fn from_list_files(path: &str, entries: &[String]) -> Self {
        let source = format!("tool=list_files path=\"{path}\"");
        let marker = "... [truncated]";
        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len();

        let mut content = String::new();
        let mut truncated = false;

        for entry in entries {
            let line = format!("- {}\n", entry);
            if content.len() + line.len() > budget {
                content.push_str(marker);
                truncated = true;
                break;
            }
            content.push_str(&line);
        }

        Self {
            source,
            content,
            truncated,
        }
    }

    /// Returns a summary-only string suitable for event detail — does NOT
    /// embed the raw content.
    pub fn attach_summary(&self) -> String {
        format!(
            "source={} bytes={} truncated={}",
            self.source,
            self.content.len(),
            self.truncated
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (a) Small read content stored untruncated.
    #[test]
    fn read_file_small_content_is_untruncated() {
        let ctx = ManualToolContext::from_read_file("foo.txt", "hello");
        assert_eq!(ctx.content, "hello");
        assert!(!ctx.truncated);
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
    }

    // (b) Oversized read content is truncated within the cap.
    #[test]
    fn read_file_oversized_content_is_truncated() {
        let big = "x".repeat(MANUAL_TOOL_CONTEXT_MAX_BYTES + 100);
        let ctx = ManualToolContext::from_read_file("big.txt", &big);
        assert!(ctx.truncated);
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
        assert!(
            ctx.content.ends_with("\n... [truncated]"),
            "expected truncation marker, got: {:?}",
            &ctx.content[ctx.content.len().saturating_sub(20)..]
        );
    }

    // (c) Long entry list is capped.
    #[test]
    fn list_files_long_list_is_capped() {
        let entries: Vec<String> = (0..500).map(|i| format!("entry_{:04}", i)).collect();
        let ctx = ManualToolContext::from_list_files("/some/dir", &entries);
        assert!(ctx.truncated);
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
    }

    // (c) Short entry list fits without truncation.
    #[test]
    fn list_files_short_list_is_untruncated() {
        let entries: Vec<String> = vec!["a.txt".to_string(), "b.txt".to_string()];
        let ctx = ManualToolContext::from_list_files("/dir", &entries);
        assert!(!ctx.truncated);
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
        assert!(ctx.content.contains("- a.txt\n"));
        assert!(ctx.content.contains("- b.txt\n"));
    }

    // (d) Source strings match expected formats.
    #[test]
    fn source_strings_match_expected_format() {
        let rf = ManualToolContext::from_read_file("/a/b.txt", "data");
        assert_eq!(rf.source, r#"tool=read_file path="/a/b.txt""#);

        let lf = ManualToolContext::from_list_files("/a/b", &[]);
        assert_eq!(lf.source, r#"tool=list_files path="/a/b""#);
    }

    // (e) attach_summary contains required fields and not the raw content.
    #[test]
    fn attach_summary_contains_required_fields_without_raw_content() {
        let raw = "secret raw content that must not appear in the summary";
        let ctx = ManualToolContext::from_read_file("secret.txt", raw);
        let summary = ctx.attach_summary();
        assert!(summary.contains("bytes="), "missing bytes= in: {summary}");
        assert!(summary.contains("truncated="), "missing truncated= in: {summary}");
        assert!(
            !summary.contains(raw),
            "summary must not contain raw content: {summary}"
        );
    }

    // (f) Char-boundary truncation: a 2-byte char straddling the budget boundary.
    #[test]
    fn read_file_char_boundary_truncation_is_safe() {
        let marker = "\n... [truncated]";
        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len();

        // Place 'é' (U+00E9, 2 bytes) so its first byte is at (budget - 1) and its
        // second byte is at `budget`.  Then append enough ASCII so the total length
        // exceeds MANUAL_TOOL_CONTEXT_MAX_BYTES and triggers truncation.
        //
        //  [0 .. budget-1)  ASCII 'a'  (budget - 1 bytes)
        //  [budget-1 .. budget+1)  'é' (2 bytes)
        //  [budget+1 ..)   ASCII 'b'  (enough to exceed cap)
        let mut input = "a".repeat(budget - 1);
        input.push('é'); // bytes at positions [budget-1, budget]
        input.push_str(&"b".repeat(MANUAL_TOOL_CONTEXT_MAX_BYTES)); // exceed the cap

        // Verify test setup: byte at `budget` is the 2nd byte of 'é' — not a boundary.
        assert!(
            !input.is_char_boundary(budget),
            "test setup: byte at budget must be mid-char"
        );
        // Input must exceed the cap so truncation is triggered.
        assert!(
            input.len() > MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "test setup: input must exceed cap"
        );

        let ctx = ManualToolContext::from_read_file("chars.txt", &input);

        assert!(ctx.truncated, "must be truncated");
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
        // Stored content must be valid UTF-8.
        std::str::from_utf8(ctx.content.as_bytes()).expect("stored content must be valid UTF-8");
        // The end of the stored content (after stripping the marker) must be char-boundary aligned.
        let without_marker = ctx
            .content
            .strip_suffix(marker)
            .expect("must end with the truncation marker");
        assert!(
            input.is_char_boundary(without_marker.len()),
            "cut point must be a char boundary"
        );
    }
}
