#![allow(dead_code)]

use super::ir::{Node, SchemaIr, StructField};
use crate::validation::ValidationOptions;
use rb_sys::bindings::{ID, rb_intern, rb_intern2};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct MaterializeEntry {
    pub name: String,
    pub ivar_id: ID,
    pub symbol_id: ID,
}

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
    summary: String,
    ir: Arc<SchemaIr>,
    plan: MaterializePlan,
    options: ValidationOptions,
}

impl CompiledSchema {
    pub fn new(summary: String, ir: SchemaIr, plan: MaterializePlan) -> Self {
        let id = NEXT_SCHEMA_ID.fetch_add(1, Ordering::SeqCst);
        Self {
            id,
            summary,
            ir: Arc::new(ir),
            plan,
            options: ValidationOptions::default(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn summary(&self) -> &str {
        &self.summary
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
        Self::new(
            "{}".to_string(),
            SchemaIr::empty(),
            MaterializePlan::empty(),
        )
    }
}

pub fn build_materialize_plan(ir: &SchemaIr) -> MaterializePlan {
    use std::os::raw::c_long;

    let mut entries = Vec::new();
    let root = ir.root();
    if let Node::Struct { fields, .. } = ir.arena().get(root) {
        for StructField { name, .. } in fields {
            let key_cstr = CString::new(name.as_str()).expect("valid field name");
            let symbol_id = unsafe { rb_intern2(key_cstr.as_ptr(), name.len() as c_long) };

            let ivar = format!("@{name}");
            let ivar_cstr = CString::new(ivar.as_str()).expect("valid ivar name");
            let ivar_id = unsafe { rb_intern(ivar_cstr.as_ptr()) };

            entries.push(MaterializeEntry {
                name: name.clone(),
                ivar_id,
                symbol_id,
            });
        }
    }

    MaterializePlan::new(entries)
}
