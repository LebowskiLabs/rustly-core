use crate::{CompiledSchema, ErrorSet};
use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qnil};
use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::{c_long, c_void};
use std::ptr;

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
    value != Qfalse.into() && value != Qnil.into()
}

pub(crate) fn hash_flag(hash: VALUE, key: VALUE) -> bool {
    unsafe {
        if !truthy(rb_obj_is_kind_of(hash, rb_cHash)) {
            return false;
        }
        truthy(rb_hash_aref(hash, key))
    }
}

pub(crate) fn method_arity1(
    func: unsafe extern "C" fn(VALUE) -> VALUE,
) -> Option<unsafe extern "C" fn() -> VALUE> {
    unsafe {
        Some(mem::transmute::<
            unsafe extern "C" fn(VALUE) -> VALUE,
            unsafe extern "C" fn() -> VALUE,
        >(func))
    }
}

pub(crate) fn method_arity2(
    func: unsafe extern "C" fn(VALUE, VALUE) -> VALUE,
) -> Option<unsafe extern "C" fn() -> VALUE> {
    unsafe {
        Some(mem::transmute::<
            unsafe extern "C" fn(VALUE, VALUE) -> VALUE,
            unsafe extern "C" fn() -> VALUE,
        >(func))
    }
}

pub(crate) fn method_arity5(
    func: unsafe extern "C" fn(VALUE, VALUE, VALUE, VALUE, VALUE) -> VALUE,
) -> Option<unsafe extern "C" fn() -> VALUE> {
    unsafe {
        Some(mem::transmute::<
            unsafe extern "C" fn(VALUE, VALUE, VALUE, VALUE, VALUE) -> VALUE,
            unsafe extern "C" fn() -> VALUE,
        >(func))
    }
}
