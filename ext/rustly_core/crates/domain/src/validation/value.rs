use super::arena::{Arena, BVec};
use serde_json::Value;
use std::mem;

pub type OwnedList<'arena> = BVec<'arena, OwnedValue<'arena>>;
pub type OwnedDict<'arena> = BVec<'arena, (&'arena str, OwnedValue<'arena>)>;

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedStruct<'arena> {
    pub names: BVec<'arena, &'arena str>,
    pub fields: BVec<'arena, Option<OwnedValue<'arena>>>,
    pub extras: Option<OwnedDict<'arena>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OwnedValue<'arena> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(&'arena str),
    Symbol(&'arena str),
    List(OwnedList<'arena>),
    Dict(OwnedDict<'arena>),
    Struct(OwnedStruct<'arena>),
}

impl<'arena> OwnedValue<'arena> {
    pub fn type_name(&self) -> &'static str {
        match self {
            OwnedValue::Null => "null",
            OwnedValue::Bool(_) => "bool",
            OwnedValue::Int(_) => "int",
            OwnedValue::Float(_) => "float",
            OwnedValue::String(_) => "str",
            OwnedValue::Symbol(_) => "sym",
            OwnedValue::List(_) => "list",
            OwnedValue::Dict(_) => "dict",
            OwnedValue::Struct(_) => "struct",
        }
    }

    pub fn from_json(value: Value, arena: &'arena Arena) -> Self {
        match value {
            Value::Null => OwnedValue::Null,
            Value::Bool(b) => OwnedValue::Bool(b),
            Value::Number(num) => {
                if let Some(i) = num.as_i64() {
                    OwnedValue::Int(i)
                } else {
                    OwnedValue::Float(num.as_f64().unwrap_or_default())
                }
            }
            Value::String(s) => {
                let copied = arena.alloc_str_from_bytes(s.as_bytes());
                OwnedValue::String(copied)
            }
            Value::Array(items) => {
                let mut converted = arena.bump_vec_with_capacity(items.len());
                for item in items {
                    converted.push(OwnedValue::from_json(item, arena));
                }
                OwnedValue::List(converted)
            }
            Value::Object(map) => {
                let mut converted = arena.bump_vec_with_capacity(map.len());
                for (key, value) in map {
                    let key_ref = arena.alloc_str_from_bytes(key.as_bytes());
                    let value_ref = OwnedValue::from_json(value, arena);
                    converted.push((key_ref, value_ref));
                }
                OwnedValue::Dict(converted)
            }
        }
    }
}

pub fn extend_value_lifetime(value: OwnedValue<'_>) -> OwnedValue<'static> {
    unsafe { mem::transmute::<OwnedValue<'_>, OwnedValue<'static>>(value) }
}

pub fn extend_list_lifetime(list: OwnedList<'_>) -> OwnedList<'static> {
    unsafe { mem::transmute::<OwnedList<'_>, OwnedList<'static>>(list) }
}

pub fn extend_dict_lifetime(dict: OwnedDict<'_>) -> OwnedDict<'static> {
    unsafe { mem::transmute::<OwnedDict<'_>, OwnedDict<'static>>(dict) }
}

pub fn extend_struct_lifetime(structure: OwnedStruct<'_>) -> OwnedStruct<'static> {
    unsafe { mem::transmute::<OwnedStruct<'_>, OwnedStruct<'static>>(structure) }
}

pub fn extend_str_lifetime(value: &str) -> &'static str {
    unsafe { mem::transmute::<&str, &'static str>(value) }
}

unsafe impl<'arena> Send for OwnedStruct<'arena> {}

unsafe impl<'arena> Send for OwnedValue<'arena> {}
