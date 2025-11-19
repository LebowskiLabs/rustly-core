use super::arena::Arena;
use super::input::{PreparedInput, PreparedOwned};
use super::options::ValidationOptions;
use super::value::{
    extend_dict_lifetime, extend_list_lifetime, extend_str_lifetime, extend_struct_lifetime,
    extend_value_lifetime, OwnedDict, OwnedStruct, OwnedValue,
};
use crate::schema::ir::{
    CollectionConstraints, ExtraBehavior, Node, NodeId, PrimitiveType, SchemaIr, StringConstraints,
    StringFormat, StructFieldPresence,
};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use smallvec::SmallVec;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub path: String,
    pub code: &'static str,
    pub meta: JsonMap<String, JsonValue>,
}

impl ValidationIssue {
    fn new(path: String, code: &'static str, meta: JsonMap<String, JsonValue>) -> Self {
        Self { path, code, meta }
    }
}

#[derive(Debug)]
pub struct ValidationResult {
    pub output: Option<PreparedOwned>,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

type Path = SmallVec<[PathSegment; 8]>;

pub fn validate_no_gvl(
    schema: Arc<SchemaIr>,
    input: PreparedInput,
    options: ValidationOptions,
) -> ValidationResult {
    validate_prepared_input(schema, input, options)
}

pub fn validate_prepared_input(
    schema: Arc<SchemaIr>,
    input: PreparedInput,
    options: ValidationOptions,
) -> ValidationResult {
    match input {
        PreparedInput::Owned(prepared) => validate_owned(schema, prepared, options),
        PreparedInput::Json(source) => validate_json(schema, source, options),
    }
}

fn validate_owned(
    schema: Arc<SchemaIr>,
    prepared: PreparedOwned,
    options: ValidationOptions,
) -> ValidationResult {
    let arena = prepared.arena().clone();
    let root = prepared.value();
    let mut validator = Validator::new(schema, options, arena.clone());
    let validated = validator.validate_root(root);
    validator.finish(validated)
}

fn validate_json(
    schema: Arc<SchemaIr>,
    source: String,
    options: ValidationOptions,
) -> ValidationResult {
    match serde_json::from_str::<JsonValue>(&source) {
        Ok(json) => {
            let arena = Arena::new();
            let owned = OwnedValue::from_json(json, &arena);
            let owned = extend_value_lifetime(owned);
            let mut validator = Validator::new(schema, options, arena);
            let validated = validator.validate_root(&owned);
            validator.finish(validated)
        }
        Err(_) => {
            let mut meta = JsonMap::new();
            meta.insert("kind".into(), JsonValue::String("json".into()));
            ValidationResult {
                output: None,
                issues: vec![ValidationIssue::new(String::new(), "format_invalid", meta)],
            }
        }
    }
}

struct Validator {
    schema: Arc<SchemaIr>,
    options: ValidationOptions,
    arena: Arena,
    path: Path,
    issues: Vec<ValidationIssue>,
}

impl Validator {
    fn new(schema: Arc<SchemaIr>, options: ValidationOptions, arena: Arena) -> Self {
        Self {
            schema,
            options,
            arena,
            path: SmallVec::new(),
            issues: Vec::new(),
        }
    }

    fn validate_root(&mut self, value: &OwnedValue<'static>) -> Option<OwnedValue<'static>> {
        let root = self.schema.root();
        self.validate_node(root, value)
    }

    fn finish(self, output: Option<OwnedValue<'static>>) -> ValidationResult {
        let Validator {
            schema: _,
            options: _,
            arena,
            path: _,
            issues,
        } = self;

        if issues.is_empty() {
            let prepared = output.map(|owned| PreparedOwned::new(arena, owned));
            ValidationResult {
                output: prepared,
                issues,
            }
        } else {
            ValidationResult {
                output: None,
                issues,
            }
        }
    }

    fn validate_node(
        &mut self,
        node_id: NodeId,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        let node = self.schema.arena().get(node_id).clone();
        match node {
            Node::Primitive(primitive) => self.validate_primitive(&primitive, value),
            Node::Enum { variants } => self.validate_enum(&variants, value),
            Node::Optional { of } => self.validate_optional(of, value),
            Node::List { of, constraints } => self.validate_list(of, &constraints, value),
            Node::Dict {
                value: inner,
                constraints,
            } => self.validate_dict(inner, &constraints, value),
            Node::Struct { fields, extra } => self.validate_struct(&fields, extra, value),
        }
    }

    fn validate_primitive(
        &mut self,
        primitive: &PrimitiveType,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        match primitive {
            PrimitiveType::Any => Some(value.clone()),
            PrimitiveType::Bool => self.coerce_bool(value).map(OwnedValue::Bool),
            PrimitiveType::Int(constraints) => self.coerce_int(value).map(|coerced| {
                self.apply_int_constraints(coerced, constraints);
                OwnedValue::Int(coerced)
            }),
            PrimitiveType::Float(constraints) => self.coerce_float(value).map(|coerced| {
                self.apply_float_constraints(coerced, constraints);
                OwnedValue::Float(coerced)
            }),
            PrimitiveType::String(constraints) => self.coerce_string(value).map(|coerced| {
                self.apply_string_constraints(coerced, constraints);
                OwnedValue::String(coerced)
            }),
            PrimitiveType::Symbol => self.coerce_symbol(value).map(OwnedValue::Symbol),
        }
    }

    fn validate_enum(
        &mut self,
        variants: &[String],
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        match value {
            OwnedValue::Symbol(sym) => {
                if variants.iter().any(|variant| variant == sym) {
                    Some(OwnedValue::Symbol(sym))
                } else {
                    self.record_value_not_in(variants);
                    None
                }
            }
            OwnedValue::String(str_value) if !self.options.strict => {
                if variants.iter().any(|variant| variant == str_value) {
                    Some(OwnedValue::Symbol(str_value))
                } else {
                    self.record_value_not_in(variants);
                    None
                }
            }
            _ => {
                self.record_type_mismatch("enum");
                None
            }
        }
    }

    fn validate_optional(
        &mut self,
        inner: NodeId,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        match value {
            OwnedValue::Null => Some(OwnedValue::Null),
            _ => self.validate_node(inner, value),
        }
    }

    fn validate_list(
        &mut self,
        inner: NodeId,
        constraints: &CollectionConstraints,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        let list = match value {
            OwnedValue::List(items) => items,
            _ => {
                self.record_type_mismatch("list");
                return None;
            }
        };

        // SAFETY: we only use `arena_ptr` for allocation helpers without keeping a borrow on `self`.
        let arena_ptr: *const Arena = &self.arena;
        let mut output = unsafe { (*arena_ptr).bump_vec_with_capacity(list.len()) };
        for (index, element) in list.iter().enumerate() {
            self.path.push(PathSegment::Index(index));
            let validated = self.validate_node(inner, element);
            self.path.pop();
            if let Some(validated) = validated {
                output.push(validated);
            }
        }
        let len = output.len();
        self.apply_collection_constraints(len, constraints);
        Some(OwnedValue::List(extend_list_lifetime(output)))
    }

    fn validate_dict(
        &mut self,
        inner: NodeId,
        constraints: &CollectionConstraints,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        let map = match value {
            OwnedValue::Dict(map) => map,
            _ => {
                self.record_type_mismatch("dict");
                return None;
            }
        };

        let arena_ptr: *const Arena = &self.arena;
        let mut output = unsafe { (*arena_ptr).bump_vec_with_capacity(map.len()) };
        for &(key, ref val) in map.iter() {
            self.path.push(PathSegment::Key(key));
            let validated = self.validate_node(inner, val);
            self.path.pop();
            if let Some(validated) = validated {
                output.push((key, validated));
            }
        }
        let len = output.len();
        self.apply_collection_constraints(len, constraints);
        Some(OwnedValue::Dict(extend_dict_lifetime(output)))
    }

    fn validate_struct(
        &mut self,
        fields: &[crate::schema::ir::StructField],
        extra: ExtraBehavior,
        value: &OwnedValue<'static>,
    ) -> Option<OwnedValue<'static>> {
        let map = match value {
            OwnedValue::Dict(map) => map,
            _ => {
                self.record_type_mismatch("struct");
                return None;
            }
        };

        let arena_ptr: *const Arena = &self.arena;
        let mut field_names = unsafe { (*arena_ptr).bump_vec_with_capacity(fields.len()) };
        for field in fields {
            let name_ref = unsafe { (*arena_ptr).alloc_str_from_bytes(field.name.as_bytes()) };
            field_names.push(extend_str_lifetime(name_ref));
        }

        let mut field_values = unsafe { (*arena_ptr).bump_vec_with_capacity(fields.len()) };
        for _ in 0..fields.len() {
            field_values.push(None);
        }

        let mut field_present = unsafe { (*arena_ptr).bump_vec_with_capacity(fields.len()) };
        for _ in 0..fields.len() {
            field_present.push(false);
        }

        for (idx, field) in fields.iter().enumerate() {
            let field_name = field_names[idx];
            if let Some(field_value) = dict_find(map, field_name) {
                self.path.push(PathSegment::Key(field_name));
                if let Some(validated) = self.validate_node(field.node, field_value) {
                    field_values[idx] = Some(validated);
                    field_present[idx] = true;
                }
                self.path.pop();
            } else if matches!(field.presence, StructFieldPresence::Required) {
                self.path.push(PathSegment::Key(field_name));
                self.record_required_missing();
                self.path.pop();
            }
        }

        let effective_extra = if self.path.is_empty() {
            self.options.extra_behavior
        } else {
            extra
        };

        let mut extras_temp: Option<OwnedDict<'static>> = None;

        for &(key, ref value) in map.iter() {
            if let Some(field_idx) = find_field_index(&field_names, key) {
                if !field_present[field_idx] {
                    continue;
                }
                continue;
            }

            match effective_extra {
                ExtraBehavior::Forbid => {
                    self.path.push(PathSegment::Key(key));
                    self.record_extra_key(key);
                    self.path.pop();
                }
                ExtraBehavior::Ignore => {}
                ExtraBehavior::Allow => {
                    let extras = extras_temp.get_or_insert_with(|| unsafe {
                        (*arena_ptr).bump_vec_with_capacity(map.len())
                    });
                    let cloned: OwnedValue<'static> = value.clone();
                    extras.push((key, cloned));
                }
            }
        }

        let structure = OwnedStruct {
            names: field_names,
            fields: field_values,
            extras: extras_temp.map(extend_dict_lifetime),
        };

        Some(OwnedValue::Struct(extend_struct_lifetime(structure)))
    }

    fn coerce_bool(&mut self, value: &OwnedValue<'static>) -> Option<bool> {
        match value {
            OwnedValue::Bool(b) => Some(*b),
            OwnedValue::String(raw) if !self.options.strict => {
                if raw.eq_ignore_ascii_case("true") || *raw == "1" {
                    Some(true)
                } else if raw.eq_ignore_ascii_case("false") || *raw == "0" {
                    Some(false)
                } else {
                    self.record_coerce_failed(value);
                    None
                }
            }
            _ => {
                self.record_type_mismatch("bool");
                None
            }
        }
    }

    fn coerce_int(&mut self, value: &OwnedValue<'static>) -> Option<i64> {
        match value {
            OwnedValue::Int(i) => Some(*i),
            OwnedValue::String(raw) if !self.options.strict => raw
                .trim()
                .parse::<i64>()
                .map_err(|_| self.record_coerce_failed(value))
                .ok(),
            _ => {
                self.record_type_mismatch("int");
                None
            }
        }
    }

    fn coerce_float(&mut self, value: &OwnedValue<'static>) -> Option<f64> {
        match value {
            OwnedValue::Float(f) => Some(*f),
            OwnedValue::Int(i) => Some(*i as f64),
            OwnedValue::String(raw) if !self.options.strict => raw
                .trim()
                .parse::<f64>()
                .map_err(|_| self.record_coerce_failed(value))
                .ok(),
            _ => {
                self.record_type_mismatch("float");
                None
            }
        }
    }

    fn coerce_string(&mut self, value: &OwnedValue<'static>) -> Option<&'static str> {
        match value {
            OwnedValue::String(s) => Some(*s),
            _ => {
                self.record_type_mismatch("str");
                None
            }
        }
    }

    fn coerce_symbol(&mut self, value: &OwnedValue<'static>) -> Option<&'static str> {
        match value {
            OwnedValue::Symbol(s) => Some(*s),
            OwnedValue::String(s) if !self.options.strict => Some(*s),
            _ => {
                self.record_type_mismatch("sym");
                None
            }
        }
    }

    fn push_issue(&mut self, code: &'static str, meta: JsonMap<String, JsonValue>) {
        let path = format_path(&self.path);
        self.issues.push(ValidationIssue::new(path, code, meta));
    }

    fn record_required_missing(&mut self) {
        self.push_issue("required_missing", JsonMap::new());
    }

    fn record_type_mismatch(&mut self, expected: &str) {
        let mut meta = JsonMap::new();
        meta.insert("expected".into(), JsonValue::String(expected.into()));
        self.push_issue("type_mismatch", meta);
    }

    fn record_coerce_failed(&mut self, from: &OwnedValue<'static>) {
        let mut meta = JsonMap::new();
        meta.insert("from".into(), JsonValue::String(from.type_name().into()));
        self.push_issue("coerce_failed", meta);
    }

    fn record_value_not_in(&mut self, variants: &[String]) {
        let allowed = variants
            .iter()
            .map(|v| JsonValue::String(v.clone()))
            .collect();
        let mut meta = JsonMap::new();
        meta.insert("list".into(), JsonValue::Array(allowed));
        self.push_issue("value_not_in", meta);
    }

    fn record_extra_key(&mut self, key: &str) {
        let mut meta = JsonMap::new();
        meta.insert("name".into(), JsonValue::String(key.into()));
        self.push_issue("extra_key", meta);
    }

    fn record_format_invalid(&mut self, format: &str) {
        let mut meta = JsonMap::new();
        meta.insert("kind".into(), JsonValue::String(format.into()));
        self.push_issue("format_invalid", meta);
    }

    fn apply_int_constraints(
        &mut self,
        value: i64,
        constraints: &crate::schema::ir::IntConstraints,
    ) {
        if let Some(min) = constraints.min {
            if value < min {
                let mut meta = JsonMap::new();
                meta.insert("min".into(), JsonValue::from(min));
                self.push_issue("too_small", meta);
            }
        }
        if let Some(max) = constraints.max {
            if value > max {
                let mut meta = JsonMap::new();
                meta.insert("max".into(), JsonValue::from(max));
                self.push_issue("too_large", meta);
            }
        }
    }

    fn apply_float_constraints(
        &mut self,
        value: f64,
        constraints: &crate::schema::ir::FloatConstraints,
    ) {
        if let Some(min) = constraints.min {
            let min_value = min.into_inner();
            if value < min_value {
                let mut meta = JsonMap::new();
                if let Some(number) = JsonNumber::from_f64(min_value) {
                    meta.insert("min".into(), JsonValue::Number(number));
                }
                self.push_issue("too_small", meta);
            }
        }
        if let Some(max) = constraints.max {
            let max_value = max.into_inner();
            if value > max_value {
                let mut meta = JsonMap::new();
                if let Some(number) = JsonNumber::from_f64(max_value) {
                    meta.insert("max".into(), JsonValue::Number(number));
                }
                self.push_issue("too_large", meta);
            }
        }
    }

    fn apply_string_constraints(&mut self, value: &str, constraints: &StringConstraints) {
        let len = value.chars().count();
        if let Some(min_len) = constraints.min_size {
            if len < min_len {
                let mut meta = JsonMap::new();
                meta.insert("min".into(), JsonValue::from(min_len as i64));
                self.push_issue("too_short", meta);
            }
        }
        if let Some(max_len) = constraints.max_size {
            if len > max_len {
                let mut meta = JsonMap::new();
                meta.insert("max".into(), JsonValue::from(max_len as i64));
                self.push_issue("too_long", meta);
            }
        }

        if let Some(format) = constraints.format {
            self.apply_format(format, value);
        }
    }

    fn apply_collection_constraints(&mut self, length: usize, constraints: &CollectionConstraints) {
        if let Some(min) = constraints.min_size {
            if length < min {
                let mut meta = JsonMap::new();
                meta.insert("min".into(), JsonValue::from(min as i64));
                self.push_issue("too_short", meta);
            }
        }
        if let Some(max) = constraints.max_size {
            if length > max {
                let mut meta = JsonMap::new();
                meta.insert("max".into(), JsonValue::from(max as i64));
                self.push_issue("too_long", meta);
            }
        }
    }

    fn apply_format(&mut self, format: StringFormat, value: &str) {
        match format {
            StringFormat::Email => {
                if !value.contains('@') {
                    self.record_format_invalid("email");
                }
            }
            StringFormat::Uuid => {
                if !is_uuid(value) {
                    self.record_format_invalid("uuid");
                }
            }
            StringFormat::Url => {
                if !is_url_like(value) {
                    self.record_format_invalid("url");
                }
            }
        }
    }
}

fn dict_find<'a>(dict: &'a OwnedDict<'static>, key: &str) -> Option<&'a OwnedValue<'static>> {
    dict.iter().find_map(|(existing_key, value)| {
        if *existing_key == key {
            Some(value)
        } else {
            None
        }
    })
}

fn find_field_index(names: &[&'static str], key: &str) -> Option<usize> {
    names.iter().position(|candidate| *candidate == key)
}

enum PathSegment {
    Key(&'static str),
    Index(usize),
}

fn format_path(path: &[PathSegment]) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            PathSegment::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            PathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
        }
    }
    out
}

fn is_uuid(value: &str) -> bool {
    const DASH_POS: [usize; 4] = [8, 13, 18, 23];
    if value.len() != 36 {
        return false;
    }
    for (idx, byte) in value.bytes().enumerate() {
        if DASH_POS.contains(&idx) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn is_url_like(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}
