use kernel::ApprovalCommand;
use kernel::ApprovalQueue;

impl super::App {
    pub(super) fn handle_approval_command(&mut self, ac: ApprovalCommand) {
        match ac {
            ApprovalCommand::Status => {
                let queue = ApprovalQueue::from_event_log(&self.event_log);
                self.log.extend(queue.render_status_lines());
            }
        }
    }
}
