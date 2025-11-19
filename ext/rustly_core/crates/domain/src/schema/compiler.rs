use super::compiled::{build_materialize_plan, CompiledSchema};
use super::ir::{
    CollectionConstraints, ExtraBehavior, FloatConstraints, IntConstraints, InternTable, Node,
    NodeId, PrimitiveType, SchemaArena, SchemaIr, StringConstraints, StructField,
    StructFieldPresence,
};
use ordered_float::OrderedFloat;
use serde_json::{Map, Value};
use std::collections::HashSet;
use thiserror::Error;

const ROOT_PATH: &str = "$";

pub struct SchemaCompiler;

impl SchemaCompiler {
    /// Compile a schema from a JSON string.
    pub fn from_json_str(source: &str) -> Result<CompiledSchema, CompileError> {
        let json_value: Value = serde_json::from_str(source)?;
        Self::compile(&json_value)
    }

    /// Compile a schema from a JSON value.
    pub fn compile(schema: &Value) -> Result<CompiledSchema, CompileError> {
        let document = SchemaDocument::from_value(schema)?;
        let ir = build_ir(&document)?;
        let plan = build_materialize_plan(&ir);
        Ok(CompiledSchema::new(ir, plan))
    }
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("invalid schema at {path}: {message}")]
    InvalidSchema { path: String, message: String },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl CompileError {
    fn invalid(path: impl Into<String>, message: impl Into<String>) -> Self {
        CompileError::InvalidSchema {
            path: path.into(),
            message: message.into(),
        }
    }
}

struct SchemaDocument {
    root: TypeExpr,
}

impl SchemaDocument {
    fn from_value(value: &Value) -> Result<Self, CompileError> {
        let root = parse_type_expr(value, None, ROOT_PATH)?;
        Ok(Self { root })
    }
}

#[derive(Debug, Clone)]
enum TypeExpr {
    Primitive(PrimitiveType),
    Enum {
        variants: Vec<String>,
    },
    Optional(Box<TypeExpr>),
    List {
        of: Box<TypeExpr>,
        constraints: CollectionConstraints,
    },
    Dict {
        value: Box<TypeExpr>,
        constraints: CollectionConstraints,
    },
    Struct {
        fields: Vec<FieldExpr>,
        extra: ExtraBehavior,
    },
}

#[derive(Debug, Clone)]
struct FieldExpr {
    name: String,
    presence: StructFieldPresence,
    ty: TypeExpr,
}

fn build_ir(document: &SchemaDocument) -> Result<SchemaIr, CompileError> {
    let mut arena = SchemaArena::new();
    let mut intern = InternTable::new();
    let root_id = build_type_expr(&mut arena, &mut intern, &document.root)?;
    Ok(SchemaIr::new(arena, root_id))
}

fn build_type_expr(
    arena: &mut SchemaArena,
    intern: &mut InternTable,
    expr: &TypeExpr,
) -> Result<NodeId, CompileError> {
    match expr {
        TypeExpr::Primitive(primitive) => {
            let node = Node::Primitive(primitive.clone());
            Ok(intern.intern(arena, node))
        }
        TypeExpr::Enum { variants } => {
            let node = Node::Enum {
                variants: variants.clone(),
            };
            Ok(intern.intern(arena, node))
        }
        TypeExpr::Optional(inner) => {
            let inner_id = build_type_expr(arena, intern, inner)?;
            let node = Node::Optional { of: inner_id };
            Ok(intern.intern(arena, node))
        }
        TypeExpr::List { of, constraints } => {
            let inner_id = build_type_expr(arena, intern, of)?;
            let node = Node::List {
                of: inner_id,
                constraints: constraints.clone(),
            };
            Ok(intern.intern(arena, node))
        }
        TypeExpr::Dict { value, constraints } => {
            let value_id = build_type_expr(arena, intern, value)?;
            let node = Node::Dict {
                value: value_id,
                constraints: constraints.clone(),
            };
            Ok(intern.intern(arena, node))
        }
        TypeExpr::Struct { fields, extra } => {
            let mut struct_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let node_id = build_type_expr(arena, intern, &field.ty)?;
                struct_fields.push(StructField {
                    name: field.name.clone(),
                    presence: field.presence,
                    node: node_id,
                });
            }
            let node = Node::Struct {
                fields: struct_fields,
                extra: *extra,
            };
            Ok(intern.intern(arena, node))
        }
    }
}

fn parse_type_expr(
    value: &Value,
    field_options: Option<&Map<String, Value>>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    match value {
        Value::String(name) => parse_named_type(name, None, field_options, path),
        Value::Object(map) => {
            if let Some(name) = map.get("type").and_then(|v| v.as_str()) {
                let inline_options = inline_map_without(map, &["type"]);
                parse_named_type(name, Some(&inline_options), field_options, path)
            } else {
                Err(CompileError::invalid(
                    path,
                    "object type is missing a `type` key",
                ))
            }
        }
        Value::Array(items) => parse_array_type(items, field_options, path),
        _ => Err(CompileError::invalid(
            path,
            format!("unsupported type expression: {value:?}"),
        )),
    }
}

fn parse_named_type(
    name: &str,
    inline_options: Option<&Map<String, Value>>,
    field_options: Option<&Map<String, Value>>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    let canonical = name.to_lowercase();
    match canonical.as_str() {
        "any" => Ok(TypeExpr::Primitive(PrimitiveType::Any)),
        "bool" | "boolean" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
        "int" | "integer" => {
            let mut constraints = IntConstraints::default();
            let mut options = merge_options(inline_options, field_options);
            apply_int_constraints(&mut options, &mut constraints, path)?;
            Ok(TypeExpr::Primitive(PrimitiveType::Int(constraints)))
        }
        "float" | "double" => {
            let mut constraints = FloatConstraints::default();
            let mut options = merge_options(inline_options, field_options);
            apply_float_constraints(&mut options, &mut constraints, path)?;
            Ok(TypeExpr::Primitive(PrimitiveType::Float(constraints)))
        }
        "str" | "string" => {
            let mut constraints = StringConstraints::default();
            let mut options = merge_options(inline_options, field_options);
            apply_string_constraints(&mut options, &mut constraints, path)?;
            Ok(TypeExpr::Primitive(PrimitiveType::String(constraints)))
        }
        "sym" | "symbol" => Ok(TypeExpr::Primitive(PrimitiveType::Symbol)),
        "enum" => parse_enum_type(None, inline_options, field_options, path),
        "list" => {
            let options = merge_options(inline_options, field_options);
            parse_list_type(None, options, path)
        }
        "dict" => {
            let options = merge_options(inline_options, field_options);
            parse_dict_type(None, options, path)
        }
        "optional" => {
            let options = merge_options(inline_options, field_options);
            parse_optional_type(None, options, path)
        }
        "struct" => {
            let options = merge_options(inline_options, field_options);
            parse_struct_type(None, options, path)
        }
        other => Err(CompileError::invalid(
            path,
            format!("unknown type `{other}`"),
        )),
    }
}

fn parse_array_type(
    items: &[Value],
    field_options: Option<&Map<String, Value>>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    if items.is_empty() {
        return Err(CompileError::invalid(
            path,
            "array type expression cannot be empty",
        ));
    }
    let type_name = items[0]
        .as_str()
        .ok_or_else(|| CompileError::invalid(path, "array type must start with a string tag"))?;

    match type_name {
        "optional" => {
            if items.len() < 2 {
                return Err(CompileError::invalid(
                    path,
                    "optional type requires an inner type",
                ));
            }
            let inner = parse_type_expr(&items[1], field_options, &format!("{path}.optional"))?;
            Ok(TypeExpr::Optional(Box::new(inner)))
        }
        "list" => {
            if items.len() < 2 {
                return Err(CompileError::invalid(
                    path,
                    "list type requires an element type",
                ));
            }
            let inline = if items.len() > 2 {
                items[2].as_object()
            } else {
                None
            };
            let options = merge_options(inline, field_options);
            parse_list_type(Some(&items[1]), options, path)
        }
        "dict" => {
            if items.len() < 2 {
                return Err(CompileError::invalid(
                    path,
                    "dict type requires a value type",
                ));
            }
            let inline = if items.len() > 2 {
                items[2].as_object()
            } else {
                None
            };
            let options = merge_options(inline, field_options);
            parse_dict_type(Some(&items[1]), options, path)
        }
        "enum" => {
            let variants_value = items.get(1);
            parse_enum_type(variants_value, field_options, None, path)
        }
        "struct" => {
            let inline = items.get(1).and_then(|v| v.as_object());
            let options = merge_options(inline, field_options);
            parse_struct_type(None, options, path)
        }
        other => parse_named_type(other, None, field_options, path),
    }
}

fn parse_enum_type(
    inline_values: Option<&Value>,
    inline_options: Option<&Map<String, Value>>,
    field_options: Option<&Map<String, Value>>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    let mut options = merge_options(inline_options, field_options);
    let variants_value = inline_values.or_else(|| options.get("values"));
    let variants_source = variants_value.cloned().or_else(|| options.remove("in"));
    let variants_value = variants_source
        .ok_or_else(|| CompileError::invalid(path, "enum type requires a `values` array"))?;
    let variants_array = variants_value.as_array().ok_or_else(|| {
        CompileError::invalid(path, "enum `values` must be specified as an array")
    })?;
    let mut seen = HashSet::new();
    let mut variants = Vec::with_capacity(variants_array.len());
    for (index, variant) in variants_array.iter().enumerate() {
        let as_str = variant.as_str().ok_or_else(|| {
            CompileError::invalid(
                format!("{path}.values[{index}]"),
                "enum values must be strings",
            )
        })?;
        if seen.insert(as_str.to_string()) {
            variants.push(as_str.to_string());
        }
    }
    if variants.is_empty() {
        return Err(CompileError::invalid(path, "enum values cannot be empty"));
    }
    Ok(TypeExpr::Enum { variants })
}

fn parse_list_type(
    inline_type: Option<&Value>,
    mut options: Map<String, Value>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    let element_value = inline_type
        .cloned()
        .or_else(|| options.remove("of"))
        .or_else(|| options.remove("item"))
        .ok_or_else(|| {
            CompileError::invalid(path, "list type requires an `of` element specification")
        })?;
    let element = parse_type_expr(&element_value, None, &format!("{path}.of"))?;
    let mut constraints = CollectionConstraints::default();
    apply_collection_constraints(&mut options, &mut constraints, path)?;
    Ok(TypeExpr::List {
        of: Box::new(element),
        constraints,
    })
}

fn parse_dict_type(
    inline_type: Option<&Value>,
    mut options: Map<String, Value>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    let value_value = inline_type
        .cloned()
        .or_else(|| options.remove("values"))
        .or_else(|| options.remove("value"))
        .ok_or_else(|| {
            CompileError::invalid(path, "dict type requires a `values` specification")
        })?;
    let value = parse_type_expr(&value_value, None, &format!("{path}.values"))?;
    let mut constraints = CollectionConstraints::default();
    apply_collection_constraints(&mut options, &mut constraints, path)?;
    Ok(TypeExpr::Dict {
        value: Box::new(value),
        constraints,
    })
}

fn parse_optional_type(
    inline_type: Option<&Value>,
    mut options: Map<String, Value>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    let target_value = inline_type
        .cloned()
        .or_else(|| options.remove("of"))
        .ok_or_else(|| CompileError::invalid(path, "optional type requires an inner type"))?;
    let inner = parse_type_expr(&target_value, None, &format!("{path}.of"))?;
    Ok(TypeExpr::Optional(Box::new(inner)))
}

fn parse_struct_type(
    inline_type: Option<&Value>,
    mut options: Map<String, Value>,
    path: &str,
) -> Result<TypeExpr, CompileError> {
    if inline_type.is_some() {
        if let Some(object) = inline_type.and_then(|v| v.as_object()) {
            for (key, value) in object {
                options.insert(key.clone(), value.clone());
            }
        }
    }
    let fields_value = options
        .remove("fields")
        .ok_or_else(|| CompileError::invalid(path, "struct type requires a `fields` array"))?;
    let extra_value = options.remove("extra");
    let extra = parse_extra(extra_value.as_ref(), path)?;
    let fields = parse_fields_array(&fields_value, path)?;
    Ok(TypeExpr::Struct { fields, extra })
}

fn parse_fields_array(value: &Value, path: &str) -> Result<Vec<FieldExpr>, CompileError> {
    let array = value
        .as_array()
        .ok_or_else(|| CompileError::invalid(path, "struct `fields` must be an array"))?;
    let mut fields = Vec::with_capacity(array.len());
    for (index, entry) in array.iter().enumerate() {
        fields.push(parse_field_entry(
            entry,
            &format!("{path}.fields[{index}]"),
        )?);
    }
    Ok(fields)
}

fn parse_field_entry(value: &Value, path: &str) -> Result<FieldExpr, CompileError> {
    let array = value
        .as_array()
        .ok_or_else(|| CompileError::invalid(path, "field entry must be an array"))?;
    if array.len() < 3 {
        return Err(CompileError::invalid(
            path,
            "field entry must contain presence, name, and type",
        ));
    }

    let presence_raw = array[0]
        .as_str()
        .ok_or_else(|| CompileError::invalid(path, "field presence must be a string"))?;
    let presence = match presence_raw {
        "required" => StructFieldPresence::Required,
        "optional" => StructFieldPresence::Optional,
        other => {
            return Err(CompileError::invalid(
                path,
                format!("unsupported field presence `{other}`"),
            ));
        }
    };

    let name = array[1]
        .as_str()
        .ok_or_else(|| CompileError::invalid(path, "field name must be a string"))?
        .to_string();

    let type_value = &array[2];
    let options_map = array.get(3).and_then(|v| v.as_object());

    let ty = parse_type_expr(type_value, options_map, &format!("{path}.type"))?;
    Ok(FieldExpr { name, presence, ty })
}

fn parse_extra(value: Option<&Value>, path: &str) -> Result<ExtraBehavior, CompileError> {
    match value {
        None => Ok(ExtraBehavior::Forbid),
        Some(Value::String(raw)) => match raw.as_str() {
            "forbid" => Ok(ExtraBehavior::Forbid),
            "ignore" => Ok(ExtraBehavior::Ignore),
            "allow" => Ok(ExtraBehavior::Allow),
            other => Err(CompileError::invalid(
                path,
                format!("unknown struct extra behavior `{other}`"),
            )),
        },
        Some(other) => Err(CompileError::invalid(
            path,
            format!("struct `extra` must be a string, got {other:?}"),
        )),
    }
}

fn apply_int_constraints(
    options: &mut Map<String, Value>,
    constraints: &mut IntConstraints,
    path: &str,
) -> Result<(), CompileError> {
    if let Some(value) = options.remove("min") {
        constraints.min = Some(parse_i64(&value, &format!("{path}.min"))?);
    }
    if let Some(value) = options.remove("max") {
        constraints.max = Some(parse_i64(&value, &format!("{path}.max"))?);
    }
    Ok(())
}

fn apply_float_constraints(
    options: &mut Map<String, Value>,
    constraints: &mut FloatConstraints,
    path: &str,
) -> Result<(), CompileError> {
    if let Some(value) = options.remove("min") {
        constraints.min = Some(OrderedFloat(parse_f64(&value, &format!("{path}.min"))?));
    }
    if let Some(value) = options.remove("max") {
        constraints.max = Some(OrderedFloat(parse_f64(&value, &format!("{path}.max"))?));
    }
    Ok(())
}

fn apply_string_constraints(
    options: &mut Map<String, Value>,
    constraints: &mut StringConstraints,
    path: &str,
) -> Result<(), CompileError> {
    if let Some(value) = options.remove("min_size") {
        constraints.min_size = Some(parse_usize(&value, &format!("{path}.min_size"))?);
    }
    if let Some(value) = options.remove("max_size") {
        constraints.max_size = Some(parse_usize(&value, &format!("{path}.max_size"))?);
    }
    if let Some(value) = options.remove("format") {
        let format_value = value.as_str().ok_or_else(|| {
            CompileError::invalid(
                format!("{path}.format"),
                "format must be specified as a string",
            )
        })?;
        constraints.format = Some(match format_value {
            "email" => super::ir::StringFormat::Email,
            "uuid" => super::ir::StringFormat::Uuid,
            "url" => super::ir::StringFormat::Url,
            other => {
                return Err(CompileError::invalid(
                    format!("{path}.format"),
                    format!("unsupported string format `{other}`"),
                ));
            }
        });
    }
    Ok(())
}

fn apply_collection_constraints(
    options: &mut Map<String, Value>,
    constraints: &mut CollectionConstraints,
    path: &str,
) -> Result<(), CompileError> {
    if let Some(value) = options.remove("min_size") {
        constraints.min_size = Some(parse_usize(&value, &format!("{path}.min_size"))?);
    }
    if let Some(value) = options.remove("max_size") {
        constraints.max_size = Some(parse_usize(&value, &format!("{path}.max_size"))?);
    }
    Ok(())
}

fn parse_i64(value: &Value, path: &str) -> Result<i64, CompileError> {
    value
        .as_i64()
        .ok_or_else(|| CompileError::invalid(path, format!("expected integer, got {value:?}")))
}

fn parse_f64(value: &Value, path: &str) -> Result<f64, CompileError> {
    value.as_f64().ok_or_else(|| {
        CompileError::invalid(
            path,
            format!("expected float-compatible number, got {value:?}"),
        )
    })
}

fn parse_usize(value: &Value, path: &str) -> Result<usize, CompileError> {
    value.as_u64().map(|v| v as usize).ok_or_else(|| {
        CompileError::invalid(path, format!("expected positive integer, got {value:?}"))
    })
}

fn inline_map_without(map: &Map<String, Value>, excluded: &[&str]) -> Map<String, Value> {
    let mut result = Map::new();
    for (key, value) in map {
        if excluded.iter().any(|excluded_key| key == excluded_key) {
            continue;
        }
        result.insert(key.clone(), value.clone());
    }
    result
}

fn merge_options(
    inline: Option<&Map<String, Value>>,
    outer: Option<&Map<String, Value>>,
) -> Map<String, Value> {
    let mut merged = Map::new();
    if let Some(outer_map) = outer {
        for (key, value) in outer_map {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let Some(inline_map) = inline {
        for (key, value) in inline_map {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ir::{
        ExtraBehavior, Node, PrimitiveType, SchemaIr, StringFormat, StructFieldPresence,
    };
    use serde_json::json;

    #[test]
    fn parses_struct_schema_into_ir() {
        let value = json!({
            "type": "struct",
            "fields": [
                ["required", "email", "string", {"format": "email"}],
                ["optional", "age", "int", {"min": 0}]
            ],
            "extra": "ignore"
        });

        let document = SchemaDocument::from_value(&value).expect("document");
        let ir = build_ir(&document).expect("ir");
        assert_struct_ir(&ir);
    }

    fn assert_struct_ir(ir: &SchemaIr) {
        let root = ir.root();
        match ir.arena().get(root) {
            Node::Struct { fields, extra } => {
                assert_eq!(*extra, ExtraBehavior::Ignore);
                assert_eq!(fields.len(), 2);

                let first = &fields[0];
                assert_eq!(first.name, "email");
                assert_eq!(first.presence, StructFieldPresence::Required);
                match ir.arena().get(first.node) {
                    Node::Primitive(PrimitiveType::String(constraints)) => {
                        assert_eq!(constraints.format, Some(StringFormat::Email));
                    }
                    other => panic!("unexpected node for email field: {other:?}"),
                }

                let second = &fields[1];
                assert_eq!(second.name, "age");
                assert_eq!(second.presence, StructFieldPresence::Optional);
                match ir.arena().get(second.node) {
                    Node::Primitive(PrimitiveType::Int(_)) => {}
                    other => panic!("unexpected node for age field: {other:?}"),
                }
            }
            other => panic!("unexpected root node: {other:?}"),
        }
    }
}
