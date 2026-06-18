use serde::{Deserialize, Serialize};

use super::ids::EventSeq;
use super::kind::EventKind;

/// A single application event with its sequence number, kind, and detail string.
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct AppEvent {
    pub seq: EventSeq,
    pub kind: EventKind,
    pub detail: String,
}
