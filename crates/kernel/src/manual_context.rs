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

    /// Builds a context from a `read_file` range result.
    ///
    /// Reuses the 4 KiB char-boundary truncation from [`from_read_file`] and
    /// sets the source label to include the input range parameters so the model
    /// knows which portion of the file was attached:
    /// `tool=read_file path="{path}" offset={offset} limit={limit}`.
    pub fn from_read_file_range(path: &str, content: &str, offset: usize, limit: usize) -> Self {
        let source = format!("tool=read_file path=\"{path}\" offset={offset} limit={limit}");
        let marker = "\n... [truncated]";

        if content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES {
            return Self {
                source,
                content: content.to_string(),
                truncated: false,
            };
        }

        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len();
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

    /// Builds a context from a `read_file` tool result with line numbers.
    ///
    /// The stored `content` is formatted by
    /// [`crate::file_snippet::render_numbered_file_snippet`] with a `File:`/
    /// `Lines:` header and `N | ` prefixes on each line. The byte ceiling is
    /// still [`MANUAL_TOOL_CONTEXT_MAX_BYTES`]. Unlike [`from_read_file`], this
    /// constructor must **not** be used as the write-candidate payload.
    pub fn from_read_file_numbered(path: &str, content: &str) -> Self {
        let source = format!("tool=read_file path=\"{path}\"");
        let rendered = crate::file_snippet::render_numbered_file_snippet(
            path,
            content,
            1,
            None,
            false,
            MANUAL_TOOL_CONTEXT_MAX_BYTES,
        );
        let truncated = rendered.ends_with("... [truncated]");
        Self {
            source,
            content: rendered,
            truncated,
        }
    }

    /// Builds a context from a `read_file` range result with line numbers.
    ///
    /// Like [`from_read_file_numbered`] but `start_line` is set to `offset` so
    /// the displayed line numbers reflect the position inside the original file.
    /// The source label matches [`from_read_file_range`] exactly.
    pub fn from_read_file_range_numbered(
        path: &str,
        content: &str,
        offset: usize,
        limit: usize,
    ) -> Self {
        let source = format!("tool=read_file path=\"{path}\" offset={offset} limit={limit}");
        let rendered = crate::file_snippet::render_numbered_file_snippet(
            path,
            content,
            offset,
            None,
            false,
            MANUAL_TOOL_CONTEXT_MAX_BYTES,
        );
        let truncated = rendered.ends_with("... [truncated]");
        Self {
            source,
            content: rendered,
            truncated,
        }
    }

    /// Builds a context from a `list_files` tool result.
    ///
    /// When the full rendered list fits within [`MANUAL_TOOL_CONTEXT_MAX_BYTES`]
    /// it is returned untruncated. Otherwise entries are appended as
    /// `"- <entry>\n"` lines until the next line would exceed the
    /// marker-reserved budget, then the truncation marker is appended. In both
    /// branches the stored `content` is at most
    /// [`MANUAL_TOOL_CONTEXT_MAX_BYTES`] bytes.
    pub fn from_list_files(path: &str, entries: &[String]) -> Self {
        let source = format!("tool=list_files path=\"{path}\"");
        let marker = "... [truncated]";

        // Each rendered line is "- <entry>\n" = 3 bytes ("- " + "\n") + entry.
        let total_len: usize = entries.iter().map(|entry| 3 + entry.len()).sum();
        if total_len <= MANUAL_TOOL_CONTEXT_MAX_BYTES {
            let mut content = String::new();
            for entry in entries {
                content.push_str(&format!("- {}\n", entry));
            }
            return Self {
                source,
                content,
                truncated: false,
            };
        }

        // Truncation is required: reserve marker bytes, then fill up to budget.
        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - marker.len();
        let mut content = String::new();
        for entry in entries {
            let line = format!("- {}\n", entry);
            if content.len() + line.len() > budget {
                break;
            }
            content.push_str(&line);
        }
        content.push_str(marker);

        Self {
            source,
            content,
            truncated: true,
        }
    }

    /// Builds a context from a `search_text` tool result.
    ///
    /// The stored `content` is a newline-joined `path:line: text` listing
    /// bounded to [`MANUAL_TOOL_CONTEXT_MAX_BYTES`] bytes with char-boundary
    /// truncation, mirroring [`from_read_file`].
    pub fn from_search_text(
        query: &str,
        matches: &[crate::tool::registry::SearchMatch],
        search_truncated: bool,
    ) -> Self {
        let source = format!("tool=search_text query=\"{query}\"");
        let byte_marker = "\n... [context truncated by byte budget]";
        // Suffix that always applies when the search itself hit its match cap.
        let search_suffix = if search_truncated {
            "\n... [search results truncated by match limit]"
        } else {
            ""
        };

        let body: String = matches
            .iter()
            .map(|m| format!("{}:{}: {}", m.path, m.line, m.text))
            .collect::<Vec<_>>()
            .join("\n");

        // No byte-budget truncation needed: keep the body plus the search-cap
        // marker (if any). `truncated` still reflects the match-cap truncation.
        if body.len() + search_suffix.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES {
            return Self {
                source,
                content: format!("{body}{search_suffix}"),
                truncated: search_truncated,
            };
        }

        // Byte-budget truncation needed. Reserve room for BOTH applicable
        // markers and append them AFTER slicing the body, so a byte-budget cut
        // can never erase the search-cap marker. Both causes are then preserved
        // deterministically.
        let budget =
            MANUAL_TOOL_CONTEXT_MAX_BYTES.saturating_sub(search_suffix.len() + byte_marker.len());
        let mut cut = budget.min(body.len());
        while cut > 0 && !body.is_char_boundary(cut) {
            cut -= 1;
        }

        Self {
            source,
            content: format!("{}{search_suffix}{byte_marker}", &body[..cut]),
            truncated: true,
        }
    }

    /// Builds a context from a `glob_files` tool result.
    ///
    /// The stored `content` is a newline-joined list of workspace-relative paths,
    /// or the literal `No matches.` when `paths` is empty. Content is bounded to
    /// [`MANUAL_TOOL_CONTEXT_MAX_BYTES`] bytes with char-boundary truncation.
    /// When `truncated == true` (the kernel hit its match cap), a visible note is
    /// appended so the model does not assume the list is complete.
    pub fn from_glob_files(pattern: &str, paths: &[String], truncated: bool) -> Self {
        let source = format!("tool=glob_files pattern=\"{pattern}\"");
        let byte_marker = "\n... [context truncated by byte budget]";
        let glob_suffix = if truncated {
            "\n... [glob results truncated by match limit]"
        } else {
            ""
        };

        if paths.is_empty() {
            return Self {
                source,
                content: format!("No matches.{glob_suffix}"),
                truncated,
            };
        }

        let body = paths.join("\n");

        // No byte-budget truncation needed: keep body plus the match-cap marker.
        if body.len() + glob_suffix.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES {
            return Self {
                source,
                content: format!("{body}{glob_suffix}"),
                truncated,
            };
        }

        // Byte-budget truncation needed. Reserve room for both applicable markers.
        let budget =
            MANUAL_TOOL_CONTEXT_MAX_BYTES.saturating_sub(glob_suffix.len() + byte_marker.len());
        let mut cut = budget.min(body.len());
        while cut > 0 && !body.is_char_boundary(cut) {
            cut -= 1;
        }

        Self {
            source,
            content: format!("{}{glob_suffix}{byte_marker}", &body[..cut]),
            truncated: true,
        }
    }

    /// Returns a summary-only string suitable for event detail — does NOT
    /// embed the raw content.
    pub fn attach_summary(&self) -> String {
        format!(
            "{} risk=read_only bytes={} truncated={}",
            self.source,
            self.content.len(),
            self.truncated
        )
    }

    /// Returns the canonical source label that includes risk and truncation
    /// metadata — does NOT embed the raw content.
    ///
    /// `risk=read_only` is hardcoded because every `ManualToolContext` is
    /// constructed from a read-only tool (`read_file` / `list_files`).
    pub fn source_label(&self) -> String {
        format!(
            "{} risk=read_only truncated={}",
            self.source, self.truncated
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

    // --- from_glob_files tests ---

    // (g1) Source label uses the expected tool=glob_files pattern= format.
    #[test]
    fn from_glob_files_source_label_format() {
        let ctx = ManualToolContext::from_glob_files("**/*.rs", &[], false);
        assert_eq!(ctx.source, r#"tool=glob_files pattern="**/*.rs""#);
    }

    // (g2) Empty path list stores "No matches." and is untruncated.
    #[test]
    fn from_glob_files_empty_paths_stores_no_matches() {
        let ctx = ManualToolContext::from_glob_files("*.txt", &[], false);
        assert_eq!(ctx.content, "No matches.");
        assert!(!ctx.truncated);
        assert!(ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES);
    }

    // (g3) Many long paths are capped to MANUAL_TOOL_CONTEXT_MAX_BYTES and carry
    // the truncation marker.
    #[test]
    fn from_glob_files_many_long_paths_are_within_byte_budget_with_marker() {
        // 200 paths, each 30 bytes → total ~6000 bytes, well above the 4 KiB cap.
        let paths: Vec<String> = (0..200)
            .map(|i| format!("src/very/deep/module/path_{:04}.rs", i))
            .collect();
        let ctx = ManualToolContext::from_glob_files("**/*.rs", &paths, false);
        assert!(
            ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "content must stay within budget, len={}",
            ctx.content.len()
        );
        assert!(ctx.truncated, "must be truncated when paths exceed budget");
        assert!(
            ctx.content.contains("[context truncated by byte budget]"),
            "byte-budget marker must be present: {}",
            ctx.content
        );
        std::str::from_utf8(ctx.content.as_bytes()).expect("content must be valid UTF-8");
    }

    // (g4) truncated=true (from kernel match cap) adds a visible note even when
    // the byte budget is not exceeded.
    #[test]
    fn from_glob_files_truncated_true_includes_match_cap_note() {
        let paths: Vec<String> = vec!["foo.rs".to_string(), "bar.rs".to_string()];
        let ctx = ManualToolContext::from_glob_files("**/*.rs", &paths, true);
        assert!(ctx.truncated, "truncated flag must be true");
        assert!(
            ctx.content
                .contains("[glob results truncated by match limit]"),
            "match-cap note must be present when truncated=true: {}",
            ctx.content
        );
    }

    // (g5) Both truncation causes: byte-budget cut must NOT erase the match-cap
    // note, and content stays within budget.
    #[test]
    fn from_glob_files_both_truncation_causes_preserve_both_markers() {
        let paths: Vec<String> = (0..200)
            .map(|i| format!("src/very/deep/module/path_{:04}.rs", i))
            .collect();
        let ctx = ManualToolContext::from_glob_files("**/*.rs", &paths, true);
        assert!(
            ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "content must stay within budget"
        );
        assert!(ctx.truncated);
        assert!(
            ctx.content
                .contains("[glob results truncated by match limit]"),
            "match-cap marker must survive byte-budget truncation"
        );
        assert!(
            ctx.content.contains("[context truncated by byte budget]"),
            "byte-budget marker must be present"
        );
        std::str::from_utf8(ctx.content.as_bytes()).expect("content must be valid UTF-8");
    }

    // (g) Regression: a list whose full rendered length exceeds the
    // marker-reserved budget but still fits within the inclusive cap must NOT
    // be truncated.
    #[test]
    fn list_files_fits_within_cap_above_budget_is_untruncated() {
        let budget = MANUAL_TOOL_CONTEXT_MAX_BYTES - "... [truncated]".len();
        // Each "- e000000\n" line is 10 bytes (3 + 7).
        let entries: Vec<String> = (0..409).map(|i| format!("e{:06}", i)).collect();
        let total_len: usize = entries.iter().map(|e| 3 + e.len()).sum();
        assert!(
            total_len > budget && total_len <= MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "test setup: total_len={total_len} must be in (budget={budget}, cap={MANUAL_TOOL_CONTEXT_MAX_BYTES}]"
        );
        let ctx = ManualToolContext::from_list_files("/dir", &entries);
        assert!(
            !ctx.truncated,
            "list fitting within cap must not be truncated"
        );
        assert_eq!(ctx.content.len(), total_len);
        assert!(ctx.content.contains("- e000000\n"));
        assert!(ctx.content.contains("- e000408\n"));
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
        assert!(
            summary.starts_with("tool="),
            "summary must start with 'tool=': {summary}"
        );
        assert!(
            summary.contains("risk=read_only"),
            "missing risk=read_only in: {summary}"
        );
        assert!(summary.contains("bytes="), "missing bytes= in: {summary}");
        assert!(
            summary.contains("truncated="),
            "missing truncated= in: {summary}"
        );
        assert!(
            !summary.contains(raw),
            "summary must not contain raw content: {summary}"
        );
    }

    // (e2) attach_summary returns the exact canonical fixed format.
    #[test]
    fn attach_summary_exact_fixed_format() {
        let path = "/tmp/known.txt";
        let content = "hello";
        let ctx = ManualToolContext::from_read_file(path, content);
        let summary = ctx.attach_summary();
        assert_eq!(
            summary,
            format!(
                "tool=read_file path=\"{}\" risk=read_only bytes={} truncated=false",
                path,
                content.len()
            )
        );
    }

    // (h) source_label returns the canonical label with risk and truncated fields.
    #[test]
    fn source_label_includes_risk_and_truncated() {
        let content = "This is the README content.";
        let ctx = ManualToolContext::from_read_file("README.md", content);
        assert_eq!(
            ctx.source_label(),
            "tool=read_file path=\"README.md\" risk=read_only truncated=false"
        );
        // Must not leak raw file content.
        assert!(
            !ctx.source_label().contains(content),
            "source_label must not contain raw file content"
        );
        // Must not include bytes= field.
        assert!(
            !ctx.source_label().contains("bytes="),
            "source_label must not contain bytes="
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

    // (g) Search-cap truncation with content within the byte budget: the
    // match-limit marker is present and truncated=true, with no byte-budget marker.
    #[test]
    fn from_search_text_preserves_match_cap_without_byte_truncation() {
        let matches = vec![crate::tool::registry::SearchMatch {
            path: "a.rs".to_string(),
            line: 1,
            text: "hit".to_string(),
        }];
        let ctx = ManualToolContext::from_search_text("q", &matches, true);
        assert!(
            ctx.truncated,
            "search-cap truncation must set truncated=true"
        );
        assert!(
            ctx.content
                .contains("[search results truncated by match limit]"),
            "must mark match-cap truncation: {}",
            ctx.content
        );
        assert!(
            !ctx.content.contains("[context truncated by byte budget]"),
            "no byte-budget marker when within budget"
        );
    }

    // --- from_read_file_numbered tests ---

    // (n1) Small content produces an untruncated snippet matching
    // render_numbered_file_snippet directly.
    #[test]
    fn read_file_numbered_small_content_is_untruncated() {
        let ctx = ManualToolContext::from_read_file_numbered("foo.txt", "hello");
        let expected = crate::file_snippet::render_numbered_file_snippet(
            "foo.txt",
            "hello",
            1,
            None,
            false,
            MANUAL_TOOL_CONTEXT_MAX_BYTES,
        );
        assert_eq!(ctx.content, expected);
        assert!(!ctx.truncated);
        assert!(
            ctx.content.starts_with("File: foo.txt"),
            "content must start with 'File: foo.txt', got: {:?}",
            &ctx.content[..ctx.content.len().min(30)]
        );
    }

    // (n2) Oversized input is truncated within the byte cap and ends with the
    // truncation marker.
    #[test]
    fn read_file_numbered_oversized_is_truncated_with_marker() {
        let big = "x".repeat(MANUAL_TOOL_CONTEXT_MAX_BYTES + 1000);
        let ctx = ManualToolContext::from_read_file_numbered("big.txt", &big);
        assert!(ctx.truncated);
        assert!(
            ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "content len {} must be within cap",
            ctx.content.len()
        );
        assert!(
            ctx.content.ends_with("... [truncated]"),
            "must end with truncation marker, got: {:?}",
            &ctx.content[ctx.content.len().saturating_sub(20)..]
        );
        std::str::from_utf8(ctx.content.as_bytes()).expect("content must be valid UTF-8");
    }

    // (n3) Range-numbered constructor uses offset for line numbering and
    // includes the correct source label parameters.
    #[test]
    fn read_file_range_numbered_uses_offset_and_formats_lines() {
        let ctx = ManualToolContext::from_read_file_range_numbered("bar.rs", "a\nb\nc", 5, 3);
        assert!(
            ctx.content.contains("Lines: 5-7"),
            "must contain 'Lines: 5-7', got:\n{}",
            ctx.content
        );
        assert!(
            ctx.content.contains("   5 | a"),
            "must contain '   5 | a', got:\n{}",
            ctx.content
        );
        assert!(
            ctx.source.contains("offset=5"),
            "source must contain 'offset=5', got: {}",
            ctx.source
        );
        assert!(
            ctx.source.contains("limit=3"),
            "source must contain 'limit=3', got: {}",
            ctx.source
        );
    }

    // (h) Both causes fire: the byte-budget cut must NOT erase the match-cap
    // marker — both markers must survive and the content stays within budget.
    #[test]
    fn from_search_text_preserves_both_markers_when_byte_budget_exceeded() {
        let matches: Vec<_> = (0..200)
            .map(|i| crate::tool::registry::SearchMatch {
                path: format!("file_{i}.rs"),
                line: i + 1,
                text: "x".repeat(200),
            })
            .collect();
        let ctx = ManualToolContext::from_search_text("q", &matches, true);

        assert!(ctx.truncated);
        assert!(
            ctx.content.len() <= MANUAL_TOOL_CONTEXT_MAX_BYTES,
            "content must stay within budget, len={}",
            ctx.content.len()
        );
        assert!(
            ctx.content
                .contains("[search results truncated by match limit]"),
            "match-cap marker must survive byte-budget truncation: {}",
            ctx.content
        );
        assert!(
            ctx.content.contains("[context truncated by byte budget]"),
            "byte-budget marker must be present"
        );
        std::str::from_utf8(ctx.content.as_bytes()).expect("stored content must be valid UTF-8");
    }
}
