//! The build tree flattened to visible rows for `uniform_list`. Children
//! come from the bridge on demand (see `TreeView` for the async fetch);
//! this model only knows how to splice them in and out.

use crate::model::{NodeSummary, SharedNode};
use std::sync::Arc;

#[derive(Clone)]
pub struct Row {
    pub node: SharedNode,
    pub depth: usize,
    pub expanded: bool,
    /// Children requested from the bridge and not yet applied.
    pub loading: bool,
}

impl Row {
    pub fn has_children(&self) -> bool {
        self.node.has_children
    }
}

#[derive(Default)]
pub struct TreeModel {
    rows: Vec<Row>,
}

impl TreeModel {
    pub fn new(root: NodeSummary) -> Self {
        TreeModel {
            rows: vec![Row { node: Arc::new(root), depth: 0, expanded: false, loading: false }],
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, ix: usize) -> Option<&Row> {
        self.rows.get(ix)
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn index_of(&self, node_id: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.node.id == node_id)
    }

    pub fn parent_index(&self, ix: usize) -> Option<usize> {
        let depth = self.rows.get(ix)?.depth;
        (0..ix).rev().find(|&i| self.rows[i].depth < depth)
    }

    pub fn mark_loading(&mut self, ix: usize) {
        if let Some(row) = self.rows.get_mut(ix) {
            row.loading = true;
        }
    }

    /// Inserts fetched children under `parent_id` (wherever that row is
    /// now). Returns the parent's index, or None if it was collapsed away.
    pub fn apply_children(&mut self, parent_id: &str, children: Vec<NodeSummary>) -> Option<usize> {
        let ix = self.index_of(parent_id)?;
        let row = &mut self.rows[ix];
        row.loading = false;
        if row.expanded {
            return Some(ix);
        }
        row.expanded = true;
        let depth = row.depth + 1;
        let inserted = children
            .into_iter()
            .map(|c| Row { node: Arc::new(c), depth, expanded: false, loading: false });
        self.rows.splice(ix + 1..ix + 1, inserted);
        Some(ix)
    }

    pub fn collapse(&mut self, ix: usize) -> bool {
        let Some(row) = self.rows.get(ix) else { return false };
        if !row.expanded {
            return false;
        }
        let depth = row.depth;
        let mut end = ix + 1;
        while end < self.rows.len() && self.rows[end].depth > depth {
            end += 1;
        }
        self.rows.drain(ix + 1..end);
        self.rows[ix].expanded = false;
        true
    }

    /// Whether `ix` needs a bridge fetch before it can show children.
    pub fn needs_fetch(&self, ix: usize) -> bool {
        self.rows
            .get(ix)
            .map(|r| r.has_children() && !r.expanded && !r.loading)
            .unwrap_or(false)
    }
}
