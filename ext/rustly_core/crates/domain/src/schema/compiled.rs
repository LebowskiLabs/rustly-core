#![allow(dead_code)]

use super::ir::{Node, SchemaIr, StructField};
use crate::validation::ValidationOptions;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Materialization plan entry built from the schema IR.
#[derive(Debug, Clone)]
pub struct MaterializeEntry {
    pub name: String,
}

/// Plan describing how to materialize validated data.
#[derive(Debug, Clone)]
pub struct MaterializePlan {
    entries: Arc<Vec<MaterializeEntry>>,
    index: Arc<HashMap<String, usize>>,
}

impl MaterializePlan {
    pub fn new(entries: Vec<MaterializeEntry>) -> Self {
        let index = entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| (entry.name.clone(), idx))
            .collect();
        Self {
            entries: Arc::new(entries),
            index: Arc::new(index),
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn entries(&self) -> &[MaterializeEntry] {
        self.entries.as_slice()
    }

    pub fn get(&self, name: &str) -> Option<&MaterializeEntry> {
        self.index.get(name).map(|idx| &self.entries[*idx])
    }
}

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct CompiledSchema {
    id: u64,
    ir: Arc<SchemaIr>,
    plan: MaterializePlan,
    options: ValidationOptions,
}

impl CompiledSchema {
    pub fn new(ir: SchemaIr, plan: MaterializePlan) -> Self {
        let id = NEXT_SCHEMA_ID.fetch_add(1, Ordering::SeqCst);
        Self {
            id,
            ir: Arc::new(ir),
            plan,
            options: ValidationOptions::default(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn ir(&self) -> &SchemaIr {
        &self.ir
    }

    pub fn ir_arc(&self) -> Arc<SchemaIr> {
        Arc::clone(&self.ir)
    }

    pub fn plan(&self) -> &MaterializePlan {
        &self.plan
    }

    pub fn options(&self) -> &ValidationOptions {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut ValidationOptions {
        &mut self.options
    }
}

impl Default for CompiledSchema {
    fn default() -> Self {
        Self::new(SchemaIr::empty(), MaterializePlan::empty())
    }
}

pub fn build_materialize_plan(ir: &SchemaIr) -> MaterializePlan {
    let mut entries = Vec::new();
    let root = ir.root();
    if let Node::Struct { fields, .. } = ir.arena().get(root) {
        for StructField { name, .. } in fields {
            entries.push(MaterializeEntry { name: name.clone() });
        }
    }

    MaterializePlan::new(entries)
}
