use crate::schema::MaterializePlan;
use crate::validation::{extend_value_lifetime, FreezeMode, OwnedValue};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum MaterializeError {
    #[error("materialization expects a struct result, got {0}")]
    UnsupportedRoot(&'static str),
}

pub type MaterializeResult<T> = Result<T, MaterializeError>;

#[derive(Debug, Clone)]
pub struct MaterializedInstance {
    pub fields: HashMap<String, OwnedValue<'static>>,
    pub attributes: Option<HashMap<String, OwnedValue<'static>>>,
    pub freeze_mode: FreezeMode,
}

pub fn materialize_instance(
    root: &OwnedValue<'static>,
    plan: &MaterializePlan,
    freeze_mode: FreezeMode,
    store_attributes: bool,
) -> MaterializeResult<MaterializedInstance> {
    let structure = match root {
        OwnedValue::Struct(structure) => structure,
        other => return Err(MaterializeError::UnsupportedRoot(other.type_name())),
    };
    let field_names = &structure.names;
    let field_values = &structure.fields;
    let extras = structure.extras.as_ref();

    let mut field_lookup: HashMap<&str, usize> = HashMap::with_capacity(field_names.len());
    for (idx, name) in field_names.iter().enumerate() {
        field_lookup.insert(name, idx);
    }

    let mut fields: HashMap<String, OwnedValue<'static>> = HashMap::new();
    let mut attributes: Option<HashMap<String, OwnedValue<'static>>> = None;

    if store_attributes {
        let extras_len = extras.map(|m| m.len()).unwrap_or(0);
        let attr_map: HashMap<String, OwnedValue<'static>> =
            HashMap::with_capacity(field_values.len() + extras_len);
        attributes = Some(attr_map);
    }

    for entry in plan.entries() {
        if let Some(&idx) = field_lookup.get(entry.name.as_str()) {
            if let Some(value) = field_values.get(idx).and_then(|v| v.as_ref()) {
                let processed_value = match freeze_mode {
                    FreezeMode::Deep => deep_freeze_value(value.clone()),
                    _ => value.clone(),
                };

                fields.insert(entry.name.clone(), processed_value.clone());

                if let Some(attrs) = &mut attributes {
                    attrs.insert(entry.name.clone(), processed_value);
                }
            }
        }
    }

    if let Some(extras_map) = extras {
        for (key, value) in extras_map {
            let processed_value = match freeze_mode {
                FreezeMode::Deep => deep_freeze_value(value.clone()),
                _ => value.clone(),
            };

            if let Some(attrs) = &mut attributes {
                attrs.insert(key.to_string(), processed_value.clone());
            }
        }
    }

    Ok(MaterializedInstance {
        fields,
        attributes,
        freeze_mode,
    })
}

fn deep_freeze_value(value: OwnedValue<'_>) -> OwnedValue<'static> {
    extend_value_lifetime(value)
}
