use crate::ruby_helpers::truthy;
use crate::schema::MaterializePlan;
use crate::validation::{FreezeMode, OwnedValue};
use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::macros::RARRAY_LEN;
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long};
use thiserror::Error;

/// RAII guard that keeps a Ruby VALUE registered with the GC while it is in scope.
struct ValueGuard {
    ptr: *mut VALUE,
}

impl ValueGuard {
    unsafe fn register(value: &mut VALUE) -> Self {
        let ptr = value as *mut VALUE;
        unsafe {
            rb_gc_register_address(ptr);
        }
        Self { ptr }
    }
}

impl Drop for ValueGuard {
    fn drop(&mut self) {
        unsafe {
            rb_gc_unregister_address(self.ptr);
        }
    }
}

#[derive(Debug, Error)]
pub enum MaterializeError {
    #[error("materialization expects a struct result, got {0}")]
    UnsupportedRoot(&'static str),
    #[error("invalid symbol name `{0}`")]
    InvalidSymbol(String),
    #[error("invalid attribute key `{0}`")]
    InvalidKey(String),
}

pub fn materialize_instance(
    klass: VALUE,
    attr_ivar: *const c_char,
    root: &OwnedValue,
    plan: &MaterializePlan,
    freeze_mode: FreezeMode,
    store_attributes: bool,
) -> Result<VALUE, MaterializeError> {
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

    let mut instance = unsafe { rb_obj_alloc(klass) };
    let _instance_guard = unsafe { ValueGuard::register(&mut instance) };

    let mut attributes: VALUE = Qnil.into();
    let mut attributes_guard: Option<ValueGuard> = None;
    if store_attributes {
        let extras_len = extras.map(|m| m.len()).unwrap_or(0);
        attributes = unsafe { rb_hash_new_capa((field_values.len() + extras_len) as c_long) };
        attributes_guard = Some(unsafe { ValueGuard::register(&mut attributes) });
    }
    let has_attributes = attributes_guard.is_some();

    for entry in plan.entries() {
        if let Some(&idx) = field_lookup.get(entry.name.as_str()) {
            if idx >= field_values.len() {
                continue;
            }
            if let Some(value) = &field_values[idx] {
                let ruby_value = owned_value_to_ruby(value)?;
                if freeze_mode == FreezeMode::Deep {
                    deep_freeze_value(ruby_value);
                }

                let symbol = unsafe { rb_id2sym(entry.symbol_id) };
                unsafe {
                    if has_attributes {
                        rb_hash_aset(attributes, symbol, ruby_value);
                    }
                    rb_ivar_set(instance, entry.ivar_id, ruby_value);
                }
            }
        }
    }

    if let Some(extras_map) = extras {
        for (key, value) in extras_map {
            let ruby_value = owned_value_to_ruby(value)?;
            if freeze_mode == FreezeMode::Deep {
                deep_freeze_value(ruby_value);
            }
            if has_attributes {
                let key_value = attribute_key_value(key)?;
                unsafe {
                    rb_hash_aset(attributes, key_value, ruby_value);
                }
            }
        }
    }

    if has_attributes && !attr_ivar.is_null() {
        unsafe {
            rb_iv_set(instance, attr_ivar, attributes);
        }
    }

    match freeze_mode {
        FreezeMode::None => {}
        FreezeMode::Shallow => {
            if has_attributes {
                freeze(attributes);
            }
            freeze(instance);
        }
        FreezeMode::Deep => {
            if has_attributes {
                deep_freeze_value(attributes);
            }
            deep_freeze_value(instance);
        }
    }

    Ok(instance)
}

fn owned_value_to_ruby(value: &OwnedValue) -> Result<VALUE, MaterializeError> {
    match value {
        OwnedValue::Null => Ok(Qnil.into()),
        OwnedValue::Bool(true) => Ok(Qtrue.into()),
        OwnedValue::Bool(false) => Ok(Qfalse.into()),
        OwnedValue::Int(i) => Ok(unsafe { rb_ll2inum(*i) }),
        OwnedValue::Float(f) => Ok(unsafe { rb_float_new(*f) }),
        OwnedValue::String(s) => {
            Ok(unsafe { rb_utf8_str_new(s.as_ptr() as *const c_char, s.len() as c_long) })
        }
        OwnedValue::Symbol(s) => symbol_to_value(s),
        OwnedValue::List(items) => {
            let mut array = unsafe { rb_ary_new_capa(items.len() as c_long) };
            let _array_guard = unsafe { ValueGuard::register(&mut array) };
            for item in items.iter() {
                let ruby_value = owned_value_to_ruby(item)?;
                unsafe {
                    rb_ary_push(array, ruby_value);
                }
            }
            Ok(array)
        }
        OwnedValue::Dict(map) => build_hash(map.as_slice()),
        OwnedValue::Struct(structure) => {
            let mut capacity = structure.names.len();
            if let Some(extras) = &structure.extras {
                capacity += extras.len();
            }
            let mut hash = unsafe { rb_hash_new_capa(capacity as c_long) };
            let _hash_guard = unsafe { ValueGuard::register(&mut hash) };
            for (name, maybe_value) in structure.names.iter().zip(structure.fields.iter()) {
                if let Some(value) = maybe_value {
                    let ruby_value = owned_value_to_ruby(value)?;
                    unsafe {
                        let key = symbol_from_key(name)?;
                        rb_hash_aset(hash, key, ruby_value);
                    }
                }
            }
            if let Some(extras) = &structure.extras {
                for (key, value) in extras {
                    let ruby_value = owned_value_to_ruby(value)?;
                    unsafe {
                        let key_value = attribute_key_value(key)?;
                        rb_hash_aset(hash, key_value, ruby_value);
                    }
                }
            }
            Ok(hash)
        }
    }
}

fn build_hash(map: &[(&str, OwnedValue<'_>)]) -> Result<VALUE, MaterializeError> {
    let mut hash = unsafe { rb_hash_new() };
    let _hash_guard = unsafe { ValueGuard::register(&mut hash) };
    for (key, value) in map.iter() {
        let symbol = symbol_from_key(key)?;
        let ruby_value = owned_value_to_ruby(value)?;
        unsafe {
            rb_hash_aset(hash, symbol, ruby_value);
        }
    }
    Ok(hash)
}

fn symbol_from_key(key: &str) -> Result<VALUE, MaterializeError> {
    symbol_to_value(key)
}

fn symbol_to_value(name: &str) -> Result<VALUE, MaterializeError> {
    let cstr = CString::new(name).map_err(|_| MaterializeError::InvalidSymbol(name.into()))?;
    Ok(unsafe {
        let id = rb_intern2(cstr.as_ptr(), name.len() as c_long);
        rb_id2sym(id)
    })
}

fn attribute_key_value(name: &str) -> Result<VALUE, MaterializeError> {
    let cstr = CString::new(name).map_err(|_| MaterializeError::InvalidKey(name.into()))?;
    Ok(unsafe { rb_utf8_str_new(cstr.as_ptr(), name.len() as c_long) })
}

fn freeze(value: VALUE) {
    unsafe {
        rb_obj_freeze(value);
    }
}

fn deep_freeze_value(value: VALUE) {
    freeze(value);
    unsafe {
        if truthy(rb_obj_is_kind_of(value, rb_cArray)) {
            let length = array_length(value);
            for index in 0..length {
                let element = rb_ary_entry(value, index as c_long);
                deep_freeze_value(element);
            }
        } else if truthy(rb_obj_is_kind_of(value, rb_cHash)) {
            rb_hash_foreach(value, Some(deep_freeze_hash_callback), 0);
        }
    }
}

fn array_length(value: VALUE) -> usize {
    unsafe { RARRAY_LEN(value) as usize }
}

unsafe extern "C" fn deep_freeze_hash_callback(key: VALUE, val: VALUE, _data: VALUE) -> c_int {
    const ST_CONTINUE: c_int = 0;
    deep_freeze_value(key);
    deep_freeze_value(val);
    ST_CONTINUE
}
