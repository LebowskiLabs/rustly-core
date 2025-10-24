use super::arena::Arena;
use super::options::InputMode;
use super::value::{OwnedDict, OwnedValue, extend_value_lifetime};
use crate::ruby_helpers::class_name;
use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::macros::{RARRAY_LEN, RB_TYPE_P, RSTRING_LEN, RSTRING_PTR};
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue};
use rb_sys::{
    RUBY_T_ARRAY, RUBY_T_BIGNUM, RUBY_T_FIXNUM, RUBY_T_FLOAT, RUBY_T_HASH, RUBY_T_STRING,
    RUBY_T_SYMBOL,
};
use smallvec::SmallVec;
use std::ffi::CStr;
use std::os::raw::{c_int, c_long};
use thiserror::Error;

#[derive(Debug)]
pub enum PreparedInput {
    Owned(PreparedOwned),
    Json(String),
}

#[derive(Debug, Clone)]
pub struct PreparedOwned {
    value: OwnedValue<'static>,
    arena: Arena,
}

impl PreparedOwned {
    pub fn new(arena: Arena, value: OwnedValue<'static>) -> Self {
        Self { value, arena }
    }

    pub fn value(&self) -> &OwnedValue<'static> {
        &self.value
    }

    pub fn arena(&self) -> &Arena {
        &self.arena
    }
}

#[derive(Debug, Error)]
pub enum InputError {
    #[error("expected a String for JSON input, got {actual}")]
    ExpectedString { actual: String },
    #[error("unsupported Ruby type {actual} at path {path}")]
    UnsupportedType { path: String, actual: String },
    #[error("hash keys must be String or Symbol at path {path}, got {actual}")]
    InvalidHashKey { path: String, actual: String },
}

type Path<'arena> = SmallVec<[PathSegment<'arena>; 8]>;

enum PathSegment<'arena> {
    Key(&'arena str),
    Index(usize),
}

pub unsafe fn prepare_input(value: VALUE, mode: InputMode) -> Result<PreparedInput, InputError> {
    match mode {
        InputMode::Json => {
            let string = string_to_owned(value).ok_or_else(|| InputError::ExpectedString {
                actual: class_name(value),
            })?;
            Ok(PreparedInput::Json(string))
        }
        InputMode::Ruby => convert_owned(value),
        InputMode::Auto => {
            if ruby_type_is(value, RUBY_T_STRING) {
                let string = string_to_owned(value).ok_or_else(|| InputError::ExpectedString {
                    actual: class_name(value),
                })?;
                Ok(PreparedInput::Json(string))
            } else {
                convert_owned(value)
            }
        }
    }
}

fn convert_owned(value: VALUE) -> Result<PreparedInput, InputError> {
    let arena = Arena::new();
    let converted = {
        let mut path: Path = SmallVec::new();
        let converted = convert_ruby_value(value, &mut path, &arena)?;
        extend_value_lifetime(converted)
    };
    Ok(PreparedInput::Owned(PreparedOwned::new(arena, converted)))
}

fn convert_ruby_value<'arena>(
    value: VALUE,
    path: &mut Path<'arena>,
    arena: &'arena Arena,
) -> Result<OwnedValue<'arena>, InputError> {
    let qnil: VALUE = Qnil.into();
    let qtrue: VALUE = Qtrue.into();
    let qfalse: VALUE = Qfalse.into();

    if value == qnil {
        return Ok(OwnedValue::Null);
    }
    if value == qtrue {
        return Ok(OwnedValue::Bool(true));
    }
    if value == qfalse {
        return Ok(OwnedValue::Bool(false));
    }

    if ruby_type_is(value, RUBY_T_FIXNUM) || ruby_type_is(value, RUBY_T_BIGNUM) {
        return Ok(OwnedValue::Int(unsafe { rb_num2ll(value) }));
    }
    if ruby_type_is(value, RUBY_T_FLOAT) {
        return Ok(OwnedValue::Float(unsafe { rb_num2dbl(value) }));
    }
    if ruby_type_is(value, RUBY_T_STRING) {
        return copy_ruby_string(value, arena)
            .map(OwnedValue::String)
            .ok_or_else(|| unsupported(value, path));
    }
    if ruby_type_is(value, RUBY_T_SYMBOL) {
        return copy_ruby_symbol(value, arena)
            .map(OwnedValue::Symbol)
            .ok_or_else(|| unsupported(value, path));
    }
    if ruby_type_is(value, RUBY_T_ARRAY) {
        return convert_array(value, path, arena);
    }
    if ruby_type_is(value, RUBY_T_HASH) {
        return convert_hash(value, path, arena);
    }

    Err(unsupported(value, path))
}

fn convert_array<'arena>(
    value: VALUE,
    path: &mut Path<'arena>,
    arena: &'arena Arena,
) -> Result<OwnedValue<'arena>, InputError> {
    unsafe {
        let length = array_length(value);
        let mut items = arena.bump_vec_with_capacity(length);
        for index in 0..length {
            let element = rb_ary_entry(value, index as c_long);
            path.push(PathSegment::Index(index));
            let converted = convert_ruby_value(element, path, arena)?;
            path.pop();
            items.push(converted);
        }
        Ok(OwnedValue::List(items))
    }
}

fn convert_hash<'arena>(
    value: VALUE,
    path: &mut Path<'arena>,
    arena: &'arena Arena,
) -> Result<OwnedValue<'arena>, InputError> {
    unsafe {
        let capacity = rb_hash_size_num(value) as usize;
        let mut entries = arena.bump_vec_with_capacity(capacity);
        let mut error = None;
        let mut state = HashConversionState {
            entries: &mut entries as *mut OwnedDict<'arena>,
            path: path as *mut Path<'arena>,
            error: &mut error as *mut Option<InputError>,
            arena,
        };

        rb_hash_foreach(
            value,
            Some(hash_foreach_convert),
            (&mut state as *mut HashConversionState<'arena>) as VALUE,
        );

        if let Some(err) = error {
            Err(err)
        } else {
            Ok(OwnedValue::Dict(entries))
        }
    }
}

fn copy_ruby_string(value: VALUE, arena: &Arena) -> Option<&str> {
    unsafe {
        if !ruby_type_is(value, RUBY_T_STRING) {
            return None;
        }
        let len = RSTRING_LEN(value) as usize;
        let ptr = RSTRING_PTR(value) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        Some(arena.alloc_str_from_bytes(slice))
    }
}

fn copy_ruby_symbol(value: VALUE, arena: &Arena) -> Option<&str> {
    unsafe {
        if !ruby_type_is(value, RUBY_T_SYMBOL) {
            return None;
        }
        let string_value = rb_sym2str(value);
        copy_ruby_string(string_value, arena)
    }
}

fn extract_hash_key<'arena>(
    value: VALUE,
    path: &mut Path<'arena>,
    arena: &'arena Arena,
) -> Result<&'arena str, InputError> {
    if let Some(string) = copy_ruby_string(value, arena) {
        return Ok(string);
    }
    if let Some(symbol) = copy_ruby_symbol(value, arena) {
        return Ok(symbol);
    }
    Err(InputError::InvalidHashKey {
        path: format_path(path),
        actual: class_name(value),
    })
}

fn unsupported(value: VALUE, path: &mut Path<'_>) -> InputError {
    InputError::UnsupportedType {
        path: format_path(path),
        actual: class_name(value),
    }
}

fn format_path(path: &[PathSegment<'_>]) -> String {
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

unsafe fn array_length(value: VALUE) -> usize {
    unsafe { RARRAY_LEN(value) as usize }
}

struct HashConversionState<'arena> {
    entries: *mut OwnedDict<'arena>,
    path: *mut Path<'arena>,
    error: *mut Option<InputError>,
    arena: &'arena Arena,
}

unsafe extern "C" fn hash_foreach_convert(key: VALUE, value: VALUE, data: VALUE) -> c_int {
    const ST_CONTINUE: c_int = 0;
    const ST_STOP: c_int = 1;

    let state = unsafe { &mut *(data as *mut HashConversionState<'static>) };
    let error_slot = unsafe { &mut *state.error };
    if error_slot.is_some() {
        return ST_STOP;
    }

    let path = unsafe { &mut *state.path };
    let entries = unsafe { &mut *state.entries };

    let key_ref = match extract_hash_key(key, path, state.arena) {
        Ok(key) => key,
        Err(err) => {
            *error_slot = Some(err);
            return ST_STOP;
        }
    };

    path.push(PathSegment::Key(key_ref));
    let converted = match convert_ruby_value(value, path, state.arena) {
        Ok(converted) => converted,
        Err(err) => {
            path.pop();
            *error_slot = Some(err);
            return ST_STOP;
        }
    };
    path.pop();

    entries.push((key_ref, converted));
    ST_CONTINUE
}

fn string_to_owned(value: VALUE) -> Option<String> {
    unsafe {
        if !ruby_type_is(value, RUBY_T_STRING) {
            return None;
        }
        let mut string = value;
        let ptr = rb_string_value_cstr(&mut string);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

#[inline]
fn ruby_type_is(value: VALUE, ty: ruby_value_type) -> bool {
    unsafe { RB_TYPE_P(value, ty) }
}
