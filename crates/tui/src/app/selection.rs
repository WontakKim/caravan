impl super::App {
    pub fn select_next(&mut self) {
        let len_before = self.event_log.len();
        if len_before == 0 {
            return;
        }
        let new_idx = match self.selected_event {
            None => 0,
            Some(i) => {
                let clamped = (i + 1).min(len_before - 1);
                if clamped == i {
                    return;
                }
                clamped
            }
        };
        self.selected_event = Some(new_idx);
        self.inspector_scroll = 0;
    }

    pub fn select_prev(&mut self) {
        let len_before = self.event_log.len();
        if len_before == 0 {
            return;
        }
        let new_idx = match self.selected_event {
            None => len_before - 1,
            Some(i) => {
                if i == 0 {
                    return;
                }
                i - 1
            }
        };
        self.selected_event = Some(new_idx);
        self.inspector_scroll = 0;
    }

    pub fn scroll_inspector_down(&mut self) {
        self.inspector_scroll = self
            .inspector_scroll
            .saturating_add(super::INSPECTOR_SCROLL_STEP);
    }

    pub fn scroll_inspector_up(&mut self) {
        self.inspector_scroll = self
            .inspector_scroll
            .saturating_sub(super::INSPECTOR_SCROLL_STEP);
    }
}
