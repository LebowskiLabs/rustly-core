//! Rust implementation of the `rustly-core` native extension.

mod ruby_helpers;

use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qtrue};
use ruby_helpers::{
    ensure_symbol, error_set_from_value, hash_flag, method_arity1, method_arity2, method_arity5,
    schema_from_value, wrap_compiled_schema, wrap_error_set,
};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_long, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

const VERSION: &str = env!("CARGO_PKG_VERSION");

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);
static mut CORE_MODULE: VALUE = 0;
static mut COMPILED_SCHEMA_CLASS: VALUE = 0;
static mut ERROR_SET_CLASS: VALUE = 0;
static mut FAIL_SYMBOL: VALUE = 0;
static mut FORCE_ERROR_SYMBOL: VALUE = 0;
static mut ATTR_IVAR_NAME: *const c_char = ptr::null();

#[derive(Default)]
struct CompiledSchema {
    id: u64,
    summary: String,
}

#[derive(Default)]
struct ErrorSet {
    messages: Vec<String>,
}

pub(crate) unsafe extern "C" fn noop_mark(_ptr: *mut c_void) {}

pub(crate) unsafe extern "C" fn compiled_schema_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ptr as *mut CompiledSchema));
    }
}

pub(crate) unsafe extern "C" fn compiled_schema_memsize(ptr: *const c_void) -> size_t {
    if ptr.is_null() {
        return 0;
    }
    std::mem::size_of::<CompiledSchema>() as size_t
}

pub(crate) unsafe extern "C" fn error_set_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ptr as *mut ErrorSet));
    }
}

pub(crate) unsafe extern "C" fn error_set_memsize(ptr: *const c_void) -> size_t {
    if ptr.is_null() {
        return 0;
    }
    std::mem::size_of::<ErrorSet>() as size_t
}

unsafe extern "C" fn compiled_schema_alloc(klass: VALUE) -> VALUE {
    wrap_compiled_schema(CompiledSchema::default(), klass)
}

unsafe extern "C" fn error_set_alloc(klass: VALUE) -> VALUE {
    wrap_error_set(ErrorSet::default(), klass)
}

unsafe extern "C" fn compiled_schema_id(self_value: VALUE) -> VALUE {
    let schema = schema_from_value(self_value);
    unsafe { rb_ull2inum((*schema).id) }
}

unsafe extern "C" fn compiled_schema_summary(self_value: VALUE) -> VALUE {
    let schema = schema_from_value(self_value);
    unsafe {
        let summary = &(*schema).summary;
        rb_utf8_str_new(summary.as_ptr() as *const c_char, summary.len() as c_long)
    }
}

unsafe extern "C" fn error_set_messages(self_value: VALUE) -> VALUE {
    let set = error_set_from_value(self_value);
    unsafe {
        let messages = &(*set).messages;
        let array = rb_ary_new_capa(messages.len() as c_long);
        for message in messages {
            let str_value =
                rb_utf8_str_new(message.as_ptr() as *const c_char, message.len() as c_long);
            rb_ary_push(array, str_value);
        }
        array
    }
}

unsafe extern "C" fn version(_self: VALUE) -> VALUE {
    unsafe {
        let cstr = CString::new(VERSION).expect("static version");
        rb_utf8_str_new(cstr.as_ptr(), cstr.as_bytes().len() as c_long)
    }
}

unsafe extern "C" fn compile(_self: VALUE, schema_ast: VALUE) -> VALUE {
    unsafe {
        let inspect_id = rb_intern(c"inspect".as_ptr());
        let mut inspected = rb_funcallv(schema_ast, inspect_id, 0, ptr::null());
        let ptr = rb_string_value_cstr(&mut inspected);
        let summary = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        let id = NEXT_SCHEMA_ID.fetch_add(1, Ordering::SeqCst);
        wrap_compiled_schema(CompiledSchema { id, summary }, COMPILED_SCHEMA_CLASS)
    }
}

unsafe extern "C" fn build(
    _self: VALUE,
    compiled: VALUE,
    input: VALUE,
    opts: VALUE,
    klass: VALUE,
) -> VALUE {
    unsafe {
        let schema_ptr = schema_from_value(compiled);
        let id = (*schema_ptr).id;

        let fail_symbol = FAIL_SYMBOL;
        let force_error_symbol = FORCE_ERROR_SYMBOL;

        let should_fail = hash_flag(input, fail_symbol) || hash_flag(opts, force_error_symbol);

        if should_fail {
            let errors = ErrorSet {
                messages: vec![format!("validation failed for schema #{id}")],
            };
            let error_value = wrap_error_set(errors, ERROR_SET_CLASS);
            let values = [Qfalse.into(), error_value];
            rb_ary_new_from_values(2, values.as_ptr())
        } else {
            let instance = rb_class_new_instance(0, ptr::null(), klass);
            if !ATTR_IVAR_NAME.is_null() {
                rb_iv_set(instance, ATTR_IVAR_NAME, input);
            }
            rb_funcallv(instance, rb_intern(c"freeze".as_ptr()), 0, ptr::null());
            let values = [Qtrue.into(), instance];
            rb_ary_new_from_values(2, values.as_ptr())
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Init_rustly_core() {
    unsafe {
        let rustly_module = rb_define_module(c"Rustly".as_ptr());
        CORE_MODULE = rb_define_module_under(rustly_module, c"Core".as_ptr());

        COMPILED_SCHEMA_CLASS =
            rb_define_class_under(CORE_MODULE, c"CompiledSchema".as_ptr(), rb_cObject);
        rb_define_alloc_func(COMPILED_SCHEMA_CLASS, Some(compiled_schema_alloc));
        rb_define_method(
            COMPILED_SCHEMA_CLASS,
            c"id".as_ptr(),
            method_arity1(compiled_schema_id),
            0,
        );
        rb_define_method(
            COMPILED_SCHEMA_CLASS,
            c"summary".as_ptr(),
            method_arity1(compiled_schema_summary),
            0,
        );

        ERROR_SET_CLASS = rb_define_class_under(CORE_MODULE, c"ErrorSet".as_ptr(), rb_cObject);
        rb_define_alloc_func(ERROR_SET_CLASS, Some(error_set_alloc));
        rb_define_method(
            ERROR_SET_CLASS,
            c"messages".as_ptr(),
            method_arity1(error_set_messages),
            0,
        );
        rb_define_method(
            ERROR_SET_CLASS,
            c"to_a".as_ptr(),
            method_arity1(error_set_messages),
            0,
        );

        rb_define_singleton_method(CORE_MODULE, c"version".as_ptr(), method_arity1(version), 0);
        rb_define_singleton_method(CORE_MODULE, c"compile".as_ptr(), method_arity2(compile), 1);
        rb_define_singleton_method(CORE_MODULE, c"build".as_ptr(), method_arity5(build), 4);

        FAIL_SYMBOL = ensure_symbol("fail");
        FORCE_ERROR_SYMBOL = ensure_symbol("force_error");
        ATTR_IVAR_NAME = c"@attributes".as_ptr();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_schema_stores_summary() {
        let schema = CompiledSchema {
            id: 42,
            summary: "example".to_string(),
        };
        assert_eq!(schema.id, 42);
        assert_eq!(schema.summary, "example");
    }

    #[test]
    fn error_set_holds_messages() {
        let set = ErrorSet {
            messages: vec!["oops".into(), "another".into()],
        };
        assert_eq!(set.messages.len(), 2);
        assert!(set.messages.iter().all(|m| !m.is_empty()));
    }
}
