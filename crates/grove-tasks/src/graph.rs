//! Validate [`TaskSpec`]s into an executable [`TaskGraph`].
//!
//! STUB — implemented by the graph agent. Must detect duplicate names,
//! unknown dependencies, and cycles, and resolve each task's `needs` to
//! dependency indices.

use crate::{Result, TaskGraph, TaskSpec};

pub fn build_graph(specs: Vec<TaskSpec>) -> Result<TaskGraph> {
    let _ = specs;
    unimplemented!("graph::build_graph")
}
