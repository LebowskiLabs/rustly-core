use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct ErrorEntry {
    pub path: String,
    pub code: &'static str,
    pub meta: Map<String, Value>,
}

impl ErrorEntry {
    pub fn new(path: String, code: &'static str, meta: Map<String, Value>) -> Self {
        Self { path, code, meta }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ErrorSet {
    entries: Vec<ErrorEntry>,
}

impl ErrorSet {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn from_entries(entries: Vec<ErrorEntry>) -> Self {
        Self { entries }
    }

    pub fn push(&mut self, entry: ErrorEntry) {
        self.entries.push(entry);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[ErrorEntry] {
        &self.entries
    }
}
