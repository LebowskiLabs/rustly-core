#![allow(dead_code)]

use ordered_float::OrderedFloat;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn new(index: usize) -> Self {
        debug_assert!(index < u32::MAX as usize);
        Self(index as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Any,
    Bool,
    Int(IntConstraints),
    Float(FloatConstraints),
    String(StringConstraints),
    Symbol,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct IntConstraints {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FloatConstraints {
    pub min: Option<OrderedFloat<f64>>,
    pub max: Option<OrderedFloat<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringFormat {
    Email,
    Uuid,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct StringConstraints {
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
    pub format: Option<StringFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CollectionConstraints {
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtraBehavior {
    Forbid,
    Ignore,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructFieldPresence {
    Required,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructField {
    pub name: String,
    pub presence: StructFieldPresence,
    pub node: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    Primitive(PrimitiveType),
    Enum {
        variants: Vec<String>,
    },
    Optional {
        of: NodeId,
    },
    List {
        of: NodeId,
        constraints: CollectionConstraints,
    },
    Dict {
        value: NodeId,
        constraints: CollectionConstraints,
    },
    Struct {
        fields: Vec<StructField>,
        extra: ExtraBehavior,
    },
}

#[derive(Debug, Clone, Default)]
pub struct SchemaArena {
    nodes: Vec<Node>,
}

impl SchemaArena {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn alloc(&mut self, node: Node) -> NodeId {
        let id = NodeId::new(self.nodes.len());
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }
}

#[derive(Debug, Clone)]
pub struct SchemaIr {
    arena: SchemaArena,
    root: NodeId,
}

impl SchemaIr {
    pub fn new(arena: SchemaArena, root: NodeId) -> Self {
        Self { arena, root }
    }

    pub fn empty() -> Self {
        let mut arena = SchemaArena::new();
        let root = arena.alloc(Node::Primitive(PrimitiveType::Any));
        Self { arena, root }
    }

    pub fn arena(&self) -> &SchemaArena {
        &self.arena
    }

    pub fn root(&self) -> NodeId {
        self.root
    }
}

#[derive(Debug, Default)]
pub struct InternTable {
    entries: HashMap<Node, NodeId>,
}

impl InternTable {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn intern(&mut self, arena: &mut SchemaArena, node: Node) -> NodeId {
        if let Some(existing) = self.entries.get(&node) {
            return *existing;
        }
        let id = arena.alloc(node.clone());
        self.entries.insert(node, id);
        id
    }
}
