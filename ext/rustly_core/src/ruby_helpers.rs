use crate::errors::ErrorSet;
use crate::schema::CompiledSchema;
use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue, Qundef};
use serde_json::Value as JsonValue;
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::hint::unreachable_unchecked;
use std::os::raw::{c_char, c_long, c_void};
use std::ptr;

thread_local! {
    static SKIP_GVL: Cell<bool> = const { Cell::new(false) };
}

pub(crate) struct TypeDescriptor(pub rb_data_type_t);

unsafe impl Sync for TypeDescriptor {}

pub(crate) const COMPILED_SCHEMA_NAME: &CStr = c"Rustly::Core::CompiledSchema";
pub(crate) const ERROR_SET_NAME: &CStr = c"Rustly::Core::ErrorSet";

pub(crate) static COMPILED_SCHEMA_TYPE: TypeDescriptor = TypeDescriptor(rb_data_type_t {
    wrap_struct_name: COMPILED_SCHEMA_NAME.as_ptr(),
    function: rb_data_type_struct__bindgen_ty_1 {
        dmark: Some(crate::noop_mark),
        dfree: Some(crate::compiled_schema_free),
        dsize: Some(crate::compiled_schema_memsize),
        dcompact: None,
        reserved: [ptr::null_mut(); 1],
    },
    parent: ptr::null(),
    data: ptr::null_mut(),
    flags: 0,
});

pub(crate) static ERROR_SET_TYPE: TypeDescriptor = TypeDescriptor(rb_data_type_t {
    wrap_struct_name: ERROR_SET_NAME.as_ptr(),
    function: rb_data_type_struct__bindgen_ty_1 {
        dmark: Some(crate::noop_mark),
        dfree: Some(crate::error_set_free),
        dsize: Some(crate::error_set_memsize),
        dcompact: None,
        reserved: [ptr::null_mut(); 1],
    },
    parent: ptr::null(),
    data: ptr::null_mut(),
    flags: 0,
});

pub(crate) fn wrap_compiled_schema(schema: CompiledSchema, klass: VALUE) -> VALUE {
    unsafe {
        let boxed = Box::new(schema);
        let ptr = Box::into_raw(boxed) as *mut c_void;
        rb_data_typed_object_wrap(klass, ptr, &COMPILED_SCHEMA_TYPE.0)
    }
}

pub(crate) fn wrap_error_set(set: ErrorSet, klass: VALUE) -> VALUE {
    unsafe {
        let boxed = Box::new(set);
        let ptr = Box::into_raw(boxed) as *mut c_void;
        rb_data_typed_object_wrap(klass, ptr, &ERROR_SET_TYPE.0)
    }
}

pub(crate) fn schema_from_value(value: VALUE) -> *mut CompiledSchema {
    unsafe { rb_check_typeddata(value, &COMPILED_SCHEMA_TYPE.0) as *mut CompiledSchema }
}

pub(crate) fn error_set_from_value(value: VALUE) -> *mut ErrorSet {
    unsafe { rb_check_typeddata(value, &ERROR_SET_TYPE.0) as *mut ErrorSet }
}

pub(crate) fn ensure_symbol(id: &str) -> VALUE {
    unsafe {
        let cstr = CString::new(id).expect("symbol name");
        let id_value = rb_intern2(cstr.as_ptr(), cstr.as_bytes().len() as c_long);
        rb_id2sym(id_value)
    }
}

pub(crate) fn truthy(value: VALUE) -> bool {
    let qfalse: VALUE = Qfalse.into();
    let qnil: VALUE = Qnil.into();
    value != qfalse && value != qnil
}

pub(crate) fn hash_lookup(hash: VALUE, key: VALUE) -> Option<VALUE> {
    unsafe {
        if !truthy(rb_obj_is_kind_of(hash, rb_cHash)) {
            return None;
        }
        let default: VALUE = Qundef.into();
        let result = rb_hash_lookup2(hash, key, default);
        if result == default {
            None
        } else {
            Some(result)
        }
    }
}

pub(crate) fn value_to_bool(value: VALUE) -> Option<bool> {
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

pub(crate) fn string_to_owned(value: VALUE) -> Option<String> {
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

pub(crate) fn symbol_to_owned(value: VALUE) -> Option<String> {
    unsafe {
        if !truthy(rb_obj_is_kind_of(value, rb_cSymbol)) {
            return None;
        }
        let str_value = rb_sym2str(value);
        string_to_owned(str_value)
    }
}

pub(crate) fn json_value_to_ruby(value: &JsonValue) -> VALUE {
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

pub(crate) fn map_to_ruby(map: &serde_json::Map<String, JsonValue>) -> VALUE {
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

pub(crate) fn class_name(value: VALUE) -> String {
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

pub(crate) unsafe fn call_without_gvl<F, R>(func: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    if SKIP_GVL.with(|flag| flag.get()) {
        return func();
    }

    // Safety: In a multi-threaded Ruby environment, we need to be careful about
    // the context we pass to rb_thread_call_without_gvl. The context must be
    // valid for the duration of the call and not be accessed by other threads.
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
        // SAFETY: This function is called from Ruby's C API with a valid pointer
        // that points to a WithoutGvlContext allocated on the stack in call_without_gvl.
        let ctx = unsafe { &mut *(ptr as *mut WithoutGvlContext<F, R>) };
        // Take the function to avoid double execution in case of reentrancy
        if let Some(func) = ctx.func.take() {
            ctx.result = Some(func());
        }
        std::ptr::null_mut()
    }

    unsafe extern "C" fn ubf(_ptr: *mut c_void) {
        // Unblock function - called when GVL is acquired during interruption
    }

    let mut ctx = WithoutGvlContext {
        func: Some(func),
        result: None,
    };

    // Execute the function without holding the GVL
    unsafe {
        rb_thread_call_without_gvl(
            Some(executor::<F, R>),
            &mut ctx as *mut _ as *mut c_void,
            Some(ubf),
            std::ptr::null_mut(),
        );
    }

    // Return the result, which must have been set by the executor
    ctx.result
        .expect("rb_thread_call_without_gvl did not execute the function")
}

pub(crate) fn execute_with_gvl<F, R>(func: F) -> R
where
    F: FnOnce() -> R,
{
    // This is used when we need to execute Ruby code from a thread that has
    // released the GVL. We set the SKIP_GVL flag to avoid recursive calls
    // to rb_thread_call_without_gvl.
    SKIP_GVL.with(|flag| {
        let previous = flag.replace(true);
        let result = func();
        flag.set(previous);
        result
    })
}

/// Deprecated: Use `execute_with_gvl` instead
#[allow(dead_code)]
pub(crate) fn with_gvl_lock<F, R>(func: F) -> R
where
    F: FnOnce() -> R,
{
    execute_with_gvl(func)
}

#[allow(unreachable_code)]
pub(crate) fn raise_argument_error(message: &str) -> ! {
    let sanitized = message.replace('\0', "\\0");
    let cstr =
        CString::new(sanitized).unwrap_or_else(|_| CString::new("invalid argument").unwrap());
    unsafe {
        rb_raise(rb_eArgError, c"%s".as_ptr(), cstr.as_ptr());
        unreachable_unchecked()
    }
}

macro_rules! ruby_method {
    ($func:expr $(, $param:ty )* $(,)?) => {{
        Some(std::mem::transmute::<
            unsafe extern "C" fn(VALUE $(, $param)*) -> VALUE,
            unsafe extern "C" fn() -> VALUE,
        >($func))
    }};
}

pub(crate) use ruby_method;
