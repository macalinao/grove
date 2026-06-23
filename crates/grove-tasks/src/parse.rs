//! Parse a KDL `tasks { … }` block into [`TaskSpec`]s.
//!
//! STUB — implemented by the parse agent.

use kdl::KdlDocument;

use crate::{Result, TaskSpec};

pub fn parse_tasks(doc: &KdlDocument) -> Result<Vec<TaskSpec>> {
    let _ = doc;
    unimplemented!("parse::parse_tasks")
}
