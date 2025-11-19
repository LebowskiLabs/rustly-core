use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue};
use rb_sys::VALUE;
use rustly_core_domain::{CompiledSchema, ErrorSet};
use rustly_core_domain::validation::{RawInput, Arena, OwnedStruct, InputMode};
use rustly_core_domain::validation::value::OwnedValue;
use serde_json::{Value as JsonValue, Map};
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::hint::unreachable_unchecked;
use std::os::raw::{c_char, c_long, c_void, c_int};
use std::ptr;

thread_local! {
    static SKIP_GVL: Cell<bool> = const { Cell::new(false) };
}

pub struct TypeDescriptor(pub rb_data_type_t);

unsafe impl Sync for TypeDescriptor {}

pub const COMPILED_SCHEMA_NAME: &CStr = c"Rustly::Core::CompiledSchema";
pub const ERROR_SET_NAME: &CStr = c"Rustly::Core::ErrorSet";

pub static COMPILED_SCHEMA_TYPE: TypeDescriptor = TypeDescriptor(rb_data_type_t {
    wrap_struct_name: COMPILED_SCHEMA_NAME.as_ptr(),
    function: rb_data_type_struct__bindgen_ty_1 {
        dmark: Some(noop_mark),
        dfree: Some(compiled_schema_free),
        dsize: Some(compiled_schema_memsize),
        dcompact: None,
        reserved: [ptr::null_mut(); 1],
    },
    parent: ptr::null(),
    data: ptr::null_mut(),
    flags: 0,
});

pub static ERROR_SET_TYPE: TypeDescriptor = TypeDescriptor(rb_data_type_t {
    wrap_struct_name: ERROR_SET_NAME.as_ptr(),
    function: rb_data_type_struct__bindgen_ty_1 {
        dmark: Some(noop_mark),
        dfree: Some(error_set_free),
        dsize: Some(error_set_memsize),
        dcompact: None,
        reserved: [ptr::null_mut(); 1],
    },
    parent: ptr::null(),
    data: ptr::null_mut(),
    flags: 0,
});

/// # Safety
/// Called from Ruby's GC with a raw pointer; pointer may be null.
pub unsafe extern "C" fn noop_mark(_ptr: *mut c_void) {}

unsafe extern "C" fn compiled_schema_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    drop(Box::from_raw(ptr as *mut CompiledSchema));
}

unsafe extern "C" fn compiled_schema_memsize(ptr: *const c_void) -> size_t {
    if ptr.is_null() {
        return 0;
    }
    std::mem::size_of::<CompiledSchema>() as size_t
}

unsafe extern "C" fn error_set_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    drop(Box::from_raw(ptr as *mut ErrorSet));
}

unsafe extern "C" fn error_set_memsize(ptr: *const c_void) -> size_t {
    if ptr.is_null() {
        return 0;
    }
    std::mem::size_of::<ErrorSet>() as size_t
}

pub fn wrap_compiled_schema(schema: CompiledSchema, klass: VALUE) -> VALUE {
    unsafe {
        let boxed = Box::new(schema);
        let ptr = Box::into_raw(boxed) as *mut c_void;
        rb_data_typed_object_wrap(klass, ptr, &COMPILED_SCHEMA_TYPE.0)
    }
}

pub fn wrap_error_set(set: ErrorSet, klass: VALUE) -> VALUE {
    unsafe {
        let boxed = Box::new(set);
        let ptr = Box::into_raw(boxed) as *mut c_void;
        rb_data_typed_object_wrap(klass, ptr, &ERROR_SET_TYPE.0)
    }
}

pub fn schema_from_value(value: VALUE) -> *mut CompiledSchema {
    unsafe { rb_check_typeddata(value, &COMPILED_SCHEMA_TYPE.0) as *mut CompiledSchema }
}

pub fn error_set_from_value(value: VALUE) -> *mut ErrorSet {
    unsafe { rb_check_typeddata(value, &ERROR_SET_TYPE.0) as *mut ErrorSet }
}

pub fn ensure_symbol(id: &str) -> VALUE {
    unsafe {
        let cstr = CString::new(id).expect("symbol name");
        let id_value = rb_intern2(cstr.as_ptr(), cstr.as_bytes().len() as c_long);
        rb_id2sym(id_value)
    }
}

pub fn truthy(value: VALUE) -> bool {
    let qfalse: VALUE = Qfalse.into();
    let qnil: VALUE = Qnil.into();
    value != qfalse && value != qnil
}

pub fn hash_lookup(hash: VALUE, key: VALUE) -> Option<VALUE> {
    unsafe {
        if !truthy(rb_obj_is_kind_of(hash, rb_cHash)) {
            return None;
        }
        let default: VALUE = Qnil.into();
        let result = rb_hash_lookup2(hash, key, default);
        if result == default {
            None
        } else {
            Some(result)
        }
    }
}

pub fn value_to_bool(value: VALUE) -> Option<bool> {
    let qtrue: VALUE = Qtrue.into();
    let qfalse: VALUE = Qfalse.into();
    if value == qtrue {
        Some(true)
    } else if value == qfalse {
        Some(false)
    } else {
        None
    }
}

pub fn string_to_owned(value: VALUE) -> Option<String> {
    unsafe {
        if !truthy(rb_obj_is_kind_of(value, rb_cString)) {
            return None;
        }
        let mut str_value = value;
        let ptr = rb_string_value_cstr(&mut str_value);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

pub fn symbol_to_owned(value: VALUE) -> Option<String> {
    unsafe {
        if !truthy(rb_obj_is_kind_of(value, rb_cSymbol)) {
            return None;
        }
        let str_value = rb_sym2str(value);
        string_to_owned(str_value)
    }
}

pub fn json_value_to_ruby(value: &JsonValue) -> VALUE {
    match value {
        JsonValue::Null => Qnil.into(),
        JsonValue::Bool(true) => Qtrue.into(),
        JsonValue::Bool(false) => Qfalse.into(),
        JsonValue::Number(num) => {
            if let Some(i) = num.as_i64() {
                unsafe { rb_ll2inum(i) }
            } else if let Some(u) = num.as_u64() {
                unsafe { rb_ull2inum(u) }
            } else if let Some(f) = num.as_f64() {
                unsafe { rb_float_new(f) }
            } else {
                Qnil.into()
            }
        }
        JsonValue::String(s) => unsafe {
            rb_utf8_str_new(s.as_ptr() as *const c_char, s.len() as c_long)
        },
        JsonValue::Array(items) => {
            let array = unsafe { rb_ary_new_capa(items.len() as c_long) };
            for item in items {
                let ruby_value = json_value_to_ruby(item);
                unsafe {
                    rb_ary_push(array, ruby_value);
                }
            }
            array
        }
        JsonValue::Object(map) => map_to_ruby(map),
    }
}

pub fn map_to_ruby(map: &serde_json::Map<String, JsonValue>) -> VALUE {
    let hash = unsafe { rb_hash_new() };
    for (key, value) in map.iter() {
        let key_value =
            unsafe { rb_utf8_str_new(key.as_ptr() as *const c_char, key.len() as c_long) };
        let ruby_value = json_value_to_ruby(value);
        unsafe {
            rb_hash_aset(hash, key_value, ruby_value);
        }
    }
    hash
}

pub fn owned_value_to_ruby(value: &OwnedValue<'_>) -> VALUE {
    match value {
        OwnedValue::Null => Qnil.into(),
        OwnedValue::Bool(true) => Qtrue.into(),
        OwnedValue::Bool(false) => Qfalse.into(),
        OwnedValue::Int(i) => unsafe { rb_ll2inum(*i) },
        OwnedValue::Float(f) => unsafe { rb_float_new(*f) },
        OwnedValue::String(s) => {
            unsafe { rb_utf8_str_new(s.as_ptr() as *const c_char, s.len() as c_long) }
        }
        OwnedValue::Symbol(s) => ensure_symbol(s),
        OwnedValue::List(items) => {
            let array = unsafe { rb_ary_new_capa(items.len() as c_long) };
            for item in items {
                let ruby_value = owned_value_to_ruby(item);
                unsafe {
                    rb_ary_push(array, ruby_value);
                }
            }
            array
        }
        OwnedValue::Dict(entries) => {
            let hash = unsafe { rb_hash_new() };
            for (key, value) in entries {
                let key_value = unsafe {
                    rb_utf8_str_new(key.as_ptr() as *const c_char, key.len() as c_long)
                };
                let ruby_value = owned_value_to_ruby(value);
                unsafe {
                    rb_hash_aset(hash, key_value, ruby_value);
                }
            }
            hash
        }
        OwnedValue::Struct(structure) => {
            let hash = unsafe { rb_hash_new() };

            for (idx, name) in structure.names.iter().enumerate() {
                let key_value = unsafe {
                    rb_utf8_str_new(name.as_ptr() as *const c_char, name.len() as c_long)
                };
                let ruby_value = structure
                    .fields
                    .get(idx)
                    .and_then(|val| val.as_ref())
                    .map(owned_value_to_ruby)
                    .unwrap_or_else(|| Qnil.into());
                unsafe {
                    rb_hash_aset(hash, key_value, ruby_value);
                }
            }

            if let Some(extras) = &structure.extras {
                for (key, value) in extras {
                    let key_value = unsafe {
                        rb_utf8_str_new(key.as_ptr() as *const c_char, key.len() as c_long)
                    };
                    let ruby_value = owned_value_to_ruby(value);
                    unsafe {
                        rb_hash_aset(hash, key_value, ruby_value);
                    }
                }
            }

            hash
        }
    }
}

pub fn class_name(value: VALUE) -> String {
    unsafe {
        let klass = rb_obj_class(value);
        let name_ptr = rb_class2name(klass);
        if name_ptr.is_null() {
            "Unknown".to_string()
        } else {
            CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
        }
    }
}

/// # Safety
/// Runs `func` without the GVL; the closure must not touch Ruby VM state directly.
pub unsafe fn call_without_gvl<F, R>(func: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    if SKIP_GVL.with(|flag| flag.get()) {
        return func();
    }

    struct WithoutGvlContext<F, R>
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        func: Option<F>,
        result: Option<R>,
    }

    unsafe extern "C" fn executor<F, R>(ptr: *mut c_void) -> *mut c_void
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let ctx = &mut *(ptr as *mut WithoutGvlContext<F, R>);
        if let Some(func) = ctx.func.take() {
            ctx.result = Some(func());
        }
        std::ptr::null_mut()
    }

    unsafe extern "C" fn ubf(_ptr: *mut c_void) {}

    let mut ctx = WithoutGvlContext {
        func: Some(func),
        result: None,
    };

    unsafe {
        rb_thread_call_without_gvl(
            Some(executor::<F, R>),
            &mut ctx as *mut _ as *mut c_void,
            Some(ubf),
            std::ptr::null_mut(),
        );
    }

    ctx.result
        .expect("rb_thread_call_without_gvl did not execute the function")
}

pub fn execute_with_gvl<F, R>(func: F) -> R
where
    F: FnOnce() -> R,
{
    SKIP_GVL.with(|flag| {
        let previous = flag.replace(true);
        let result = func();
        flag.set(previous);
        result
    })
}

#[allow(unreachable_code)]
pub fn raise_argument_error(message: &str) -> ! {
    let sanitized = message.replace('\0', "\\0");
    let cstr =
        CString::new(sanitized).unwrap_or_else(|_| CString::new("invalid argument").unwrap());
    unsafe {
        rb_raise(rb_eArgError, c"%s".as_ptr(), cstr.as_ptr());
        unreachable_unchecked()
    }
}

#[macro_export]
macro_rules! ruby_method {
    ($func:expr $(, $param:ty )* $(,)?) => {{
        Some(std::mem::transmute::<
            unsafe extern "C" fn(VALUE $(, $param)*) -> VALUE,
            unsafe extern "C" fn() -> VALUE,
        >($func))
    }};
}

pub fn value_to_json_value(value: VALUE) -> Result<JsonValue, String> {
    unsafe {
        let qnil: VALUE = Qnil.into();
        let qtrue: VALUE = Qtrue.into();
        let qfalse: VALUE = Qfalse.into();

        if value == qnil {
            return Ok(JsonValue::Null);
        } else if value == qtrue {
            return Ok(JsonValue::Bool(true));
        } else if value == qfalse {
            return Ok(JsonValue::Bool(false));
        }

        if truthy(rb_obj_is_kind_of(value, rb_cInteger)) {
            let result = rb_num2ll(value);
            return Ok(JsonValue::Number(result.into()));
        }

        if truthy(rb_obj_is_kind_of(value, rb_cFloat)) {
            let result = rb_num2dbl(value);
            return Ok(JsonValue::Number(
                serde_json::Number::from_f64(result).unwrap_or(serde_json::Number::from(0)),
            ));
        }

        if truthy(rb_obj_is_kind_of(value, rb_cString)) {
            if let Some(s) = string_to_owned(value) {
                return Ok(JsonValue::String(s));
            }
        }

        if truthy(rb_obj_is_kind_of(value, rb_cSymbol)) {
            if let Some(s) = symbol_to_owned(value) {
                return Ok(JsonValue::String(s));
            }
        }

        if truthy(rb_obj_is_kind_of(value, rb_cArray)) {
            let length_result = rb_funcall(value, rb_intern(c"length".as_ptr()), 0);
            let len = rb_num2long(length_result) as usize;
            let mut items = Vec::with_capacity(len);
            for i in 0..len {
                let item = rb_ary_entry(value, i as c_long);
                items.push(value_to_json_value(item)?);
            }
            return Ok(JsonValue::Array(items));
        }

        if truthy(rb_obj_is_kind_of(value, rb_cHash)) {
            struct HashContext {
                map: Map<String, JsonValue>,
                error: bool,
            }

            unsafe extern "C" fn hash_foreach_callback(
                key: VALUE,
                value: VALUE,
                data: VALUE,
            ) -> c_int {
                let context: &mut HashContext = &mut *(data as *mut HashContext);

                let key_str = if truthy(rb_obj_is_kind_of(key, rb_cString)) {
                    string_to_owned(key)
                } else if truthy(rb_obj_is_kind_of(key, rb_cSymbol)) {
                    symbol_to_owned(key)
                } else {
                    string_to_owned(rb_funcall(key, rb_intern(c"to_s".as_ptr()), 0))
                };

                if let Some(key_str) = key_str {
                    match value_to_json_value(value) {
                        Ok(json_value) => {
                            context.map.insert(key_str, json_value);
                            0 // ST_CONTINUE
                        }
                        Err(_) => {
                            context.error = true;
                            1 // ST_STOP on error
                        }
                    }
                } else {
                    context.error = true;
                    1 // ST_STOP on error
                }
            }

            let mut context = HashContext {
                map: Map::new(),
                error: false,
            };

            rb_hash_foreach(
                value,
                Some(hash_foreach_callback),
                &mut context as *mut HashContext as VALUE,
            );

            if !context.error {
                return Ok(JsonValue::Object(context.map));
            } else {
                return Err("Failed to convert hash to JSON".to_string());
            }
        }

        let to_s_result = rb_funcall(value, rb_intern(c"to_s".as_ptr()), 0);
        if let Some(s) = string_to_owned(to_s_result) {
            Ok(JsonValue::String(s))
        } else {
            Err(format!("Cannot convert Ruby value to JSON: {}", class_name(value)))
        }
    }
}

/// Convert a Ruby VALUE to RawInput for validation
pub fn value_to_raw_input(value: VALUE, mode: InputMode) -> Result<RawInput, String> {
    unsafe {
        if truthy(rb_obj_is_kind_of(value, rb_cString)) {
            if let Some(s) = string_to_owned(value) {
                if mode == InputMode::Json {
                    return Ok(RawInput::Json(s));
                }

                if mode == InputMode::Auto
                    && serde_json::from_str::<JsonValue>(&s).is_ok() {
                        return Ok(RawInput::Json(s));
                    }
            }
        }

        let json_value = value_to_json_value(value)?;
        let arena = Arena::new();
        let owned_value = convert_json_to_owned(&json_value, &arena);
        Ok(RawInput::from_owned(arena.clone(), owned_value))
    }
}

/// Convert serde_json::Value to OwnedValue
fn convert_json_to_owned<'a>(value: &'a JsonValue, arena: &'a Arena) -> OwnedValue<'a> {
    match value {
        JsonValue::Null => OwnedValue::Null,
        JsonValue::Bool(b) => OwnedValue::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                OwnedValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                OwnedValue::Float(f)
            } else if let Some(u) = n.as_u64() {
                OwnedValue::Int(u as i64)
            } else {
                OwnedValue::Null
            }
        }
        JsonValue::String(s) => {
            let copied = arena.alloc_str_from_bytes(s.as_bytes());
            OwnedValue::String(copied)
        },
        JsonValue::Array(arr) => {
            let mut items = arena.bump_vec_with_capacity(arr.len());
            for item in arr {
                items.push(convert_json_to_owned(item, arena));
            }
            OwnedValue::List(items)
        }
        JsonValue::Object(map) => {
            let mut names = arena.bump_vec_with_capacity(map.len());
            let mut fields = arena.bump_vec_with_capacity(map.len());
            for (key, value) in map {
                let key_ref = arena.alloc_str_from_bytes(key.as_bytes());
                names.push(key_ref);
                fields.push(Some(convert_json_to_owned(value, arena)));
            }
            OwnedValue::Struct(OwnedStruct {
                names,
                fields,
                extras: None,
            })
        }
    }
}
