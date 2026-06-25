use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

/// Returns the header text, appending `| Workspace Context: pending` when
/// `app.pending_manual_tool_context` is `Some` and `| Workspace Context: none` otherwise.
/// The `Workspace Context` indicator reflects ONLY `pending_manual_tool_context`;
/// `last_tool_output_candidate` alone (with pending still `None`) yields
/// `| Workspace Context: none`.
fn header_text(app: &crate::app::App) -> String {
    let context_label = if app.pending_manual_tool_context.is_some() {
        "pending"
    } else {
        "none"
    };
    format!(
        "Caravan | TUI Shell | Status: Ready | Workspace Context: {}",
        context_label
    )
}

pub(super) fn render(frame: &mut ratatui::Frame, app: &crate::app::App, area: Rect) {
    let header = Paragraph::new(header_text(app))
        .block(Block::default().borders(Borders::ALL).title("Header"));
    frame.render_widget(header, area);
}

#[cfg(test)]
mod tests {
    use super::header_text;
    use crate::app::App;

    #[test]
    fn header_text_no_pending_shows_none() {
        let app = App::new();
        assert_eq!(
            header_text(&app),
            "Caravan | TUI Shell | Status: Ready | Workspace Context: none"
        );
    }

    #[test]
    fn header_text_with_pending_shows_pending() {
        let mut app = App::new();
        app.pending_manual_tool_context = Some(
            kernel::manual_context::ManualToolContext::from_read_file("f.txt", "x"),
        );
        assert_eq!(
            header_text(&app),
            "Caravan | TUI Shell | Status: Ready | Workspace Context: pending"
        );
    }

    #[test]
    fn header_text_last_candidate_only_stays_none() {
        let mut app = App::new();
        app.last_tool_output_candidate = Some(
            kernel::manual_context::ManualToolContext::from_read_file("f.txt", "x"),
        );
        // pending_manual_tool_context is still None, so header must show "none"
        let result = header_text(&app);
        assert_eq!(
            result,
            "Caravan | TUI Shell | Status: Ready | Workspace Context: none"
        );
        assert!(
            !result.contains("Workspace Context: pending"),
            "result must not contain 'Workspace Context: pending' when only last_tool_output_candidate is set"
        );
    }

    #[test]
    fn header_text_hides_request_even_when_model_tool_request_pending() {
        use kernel::model_tool_request::{ModelToolRequest, ModelToolRequestKind};

        let mut app = App::new();
        app.pending_model_tool_request = Some(ModelToolRequest {
            kind: ModelToolRequestKind::ReadFile,
            path: "README.md".to_string(),
        });
        let result = header_text(&app);
        assert_eq!(
            result,
            "Caravan | TUI Shell | Status: Ready | Workspace Context: none"
        );
        assert!(
            !result.contains("Request"),
            "header must not expose pending_model_tool_request"
        );
    }
}
