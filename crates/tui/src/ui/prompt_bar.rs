use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// Terminal display width (in columns) of the prompt input. Wide characters
/// such as Hangul/CJK occupy two columns, so the cursor must advance by the
/// rendered width rather than the scalar count; otherwise it lands inside the
/// typed text. Saturates to `u16::MAX` for pathologically long input.
fn input_display_width(input: &str) -> u16 {
    u16::try_from(UnicodeWidthStr::width(input)).unwrap_or(u16::MAX)
}

pub(super) fn render(frame: &mut ratatui::Frame, app: &crate::app::App, area: Rect) {
    // Prompt Bar
    let cmd_text = format!("> {}", app.input);
    let cmd =
        Paragraph::new(cmd_text).block(Block::default().borders(Borders::ALL).title("Prompt"));
    frame.render_widget(cmd, area);

    // Place the cursor just after the "> " prompt. The offset is 3 columns from
    // cmd_area.x: 1 for the block's left border, plus 2 for the "> " prefix.
    // Saturating arithmetic guards against extreme input lengths / tiny terminals.
    let inner_max_x = area.x.saturating_add(area.width.saturating_sub(2));
    let typed = input_display_width(&app.input);
    let cursor_x = area
        .x
        .saturating_add(3)
        .saturating_add(typed)
        .min(inner_max_x);
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

#[cfg(test)]
mod tests {
    use super::input_display_width;

    #[test]
    fn ascii_width_equals_char_count() {
        assert_eq!(input_display_width("hello"), 5);
        assert_eq!(input_display_width(""), 0);
    }

    #[test]
    fn hangul_chars_are_two_columns_each() {
        // Each Hangul syllable renders as two terminal columns, so "한글"
        // (2 scalars) occupies 4 columns. The cursor must advance by 4, not 2.
        assert_eq!("한글".chars().count(), 2);
        assert_eq!(input_display_width("한글"), 4);
    }

    #[test]
    fn mixed_ascii_and_hangul_width() {
        // "hi한" -> h(1) + i(1) + 한(2) = 4 columns.
        assert_eq!(input_display_width("hi한"), 4);
    }
}
