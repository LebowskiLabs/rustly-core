//! Rust implementation of the `rustly-core` native extension.

mod errors;
mod materialize;
mod ruby_helpers;
mod schema;
mod validation;

use crate::errors::{ErrorEntry, ErrorSet};
use crate::materialize::{MaterializeError, materialize_instance};
use crate::ruby_helpers::{
    ensure_symbol, error_set_from_value, execute_with_gvl, hash_lookup, map_to_ruby,
    raise_argument_error, ruby_method, schema_from_value, symbol_to_owned, truthy, value_to_bool,
    wrap_compiled_schema, wrap_error_set,
};
use crate::schema::ir::ExtraBehavior;
use crate::schema::{CompiledSchema, SchemaCompiler};
use crate::validation::{
    FreezeMode, InputError, InputMode, ValidationIssue, ValidationOptions, ValidationResult,
    prepare_input, validate_no_gvl,
};
use rb_sys::VALUE;
use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::ffi::CString;
use std::hint::unreachable_unchecked;
use std::os::raw::{c_char, c_long, c_void};
use std::ptr;

const VERSION: &str = env!("CARGO_PKG_VERSION");

static mut CORE_MODULE: VALUE = 0;
static mut COMPILED_SCHEMA_CLASS: VALUE = 0;
static mut ERROR_SET_CLASS: VALUE = 0;
static mut ATTR_IVAR_NAME: *const c_char = ptr::null();
static mut STRICT_SYMBOL: VALUE = 0;
static mut EXTRA_SYMBOL: VALUE = 0;
static mut INPUT_MODE_SYMBOL: VALUE = 0;
static mut FREEZE_SYMBOL: VALUE = 0;
static mut STORE_ATTRIBUTES_SYMBOL: VALUE = 0;
static mut VALIDATION_ERROR_CLASS: VALUE = 0;

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
    let id = unsafe { (&*schema).id() };
    unsafe { rb_ull2inum(id as u64) }
}

unsafe extern "C" fn compiled_schema_summary(self_value: VALUE) -> VALUE {
    let schema = schema_from_value(self_value);
    unsafe {
        let summary = (&*schema).summary();
        rb_utf8_str_new(summary.as_ptr() as *const c_char, summary.len() as c_long)
    }
}

unsafe extern "C" fn error_set_messages(self_value: VALUE) -> VALUE {
    let set = error_set_from_value(self_value);
    unsafe {
        let entries = (&*set).entries();
        let array = rb_ary_new_capa(entries.len() as c_long);
        let path_sym = ensure_symbol("path");
        let code_sym = ensure_symbol("code");
        let meta_sym = ensure_symbol("meta");
        for entry in entries {
            let hash = rb_hash_new();
            let path_value = rb_utf8_str_new(
                entry.path.as_ptr() as *const c_char,
                entry.path.len() as c_long,
            );
            rb_hash_aset(hash, path_sym, path_value);
            let code_value = ensure_symbol(entry.code);
            rb_hash_aset(hash, code_sym, code_value);
            let meta_hash = map_to_ruby(&entry.meta);
            rb_hash_aset(hash, meta_sym, meta_hash);
            rb_ary_push(array, hash);
        }
        array
    }
}

unsafe fn parse_options(opts_value: VALUE) -> Result<ValidationOptions, String> {
    let mut options = ValidationOptions::default();
    let qnil: VALUE = Qnil.into();

    if opts_value == qnil {
        return Ok(options);
    }

    if !truthy(unsafe { rb_obj_is_kind_of(opts_value, rb_cHash) }) {
        return Err("opts must be a Hash".into());
    }

    if let Some(strict_value) = hash_lookup(opts_value, unsafe { STRICT_SYMBOL }) {
        if strict_value == qnil {
            // ignore nil
        } else if let Some(boolean) = value_to_bool(strict_value) {
            options.strict = boolean;
        } else {
            return Err("opts[:strict] must be true or false".into());
        }
    }

    if let Some(mode_value) = hash_lookup(opts_value, unsafe { INPUT_MODE_SYMBOL }) {
        if mode_value == qnil {
            // ignore
        } else {
            let symbol = symbol_to_owned(mode_value)
                .ok_or_else(|| "opts[:input_mode] must be a Symbol".to_string())?;
            options.input_mode = map_input_mode(&symbol)?;
        }
    }

    if let Some(extra_value) = hash_lookup(opts_value, unsafe { EXTRA_SYMBOL }) {
        if extra_value == qnil {
            // ignore
        } else {
            let symbol = symbol_to_owned(extra_value)
                .ok_or_else(|| "opts[:extra] must be a Symbol".to_string())?;
            options.extra_behavior = map_extra_behavior(&symbol)?;
        }
    }

    if let Some(freeze_value) = hash_lookup(opts_value, unsafe { FREEZE_SYMBOL }) {
        if freeze_value == qnil {
            // ignore
        } else if let Some(boolean) = value_to_bool(freeze_value) {
            options.freeze = if boolean {
                FreezeMode::Deep
            } else {
                FreezeMode::None
            };
        } else {
            let symbol = symbol_to_owned(freeze_value)
                .ok_or_else(|| "opts[:freeze] must be false or a Symbol".to_string())?;
            options.freeze = map_freeze_mode(&symbol)?;
        }
    }

    if let Some(store_value) = hash_lookup(opts_value, unsafe { STORE_ATTRIBUTES_SYMBOL }) {
        if store_value == qnil {
            // ignore
        } else if let Some(boolean) = value_to_bool(store_value) {
            options.store_attributes = boolean;
        } else {
            return Err("opts[:store_attributes] must be true or false".into());
        }
    }

    Ok(options)
}

fn map_input_mode(symbol: &str) -> Result<InputMode, String> {
    match symbol {
        "auto" => Ok(InputMode::Auto),
        "json" => Ok(InputMode::Json),
        "ruby" => Ok(InputMode::Ruby),
        other => Err(format!("unknown input_mode :{other}")),
    }
}

fn map_extra_behavior(symbol: &str) -> Result<ExtraBehavior, String> {
    match symbol {
        "forbid" => Ok(ExtraBehavior::Forbid),
        "ignore" => Ok(ExtraBehavior::Ignore),
        "allow" => Ok(ExtraBehavior::Allow),
        other => Err(format!("unknown extra policy :{other}")),
    }
}

fn map_freeze_mode(symbol: &str) -> Result<FreezeMode, String> {
    match symbol {
        "deep" => Ok(FreezeMode::Deep),
        "shallow" => Ok(FreezeMode::Shallow),
        "none" => Ok(FreezeMode::None),
        other => Err(format!("unknown freeze mode :{other}")),
    }
}

fn error_set_from_input_error(error: InputError) -> ErrorSet {
    let mut meta = JsonMap::new();
    let (path, code) = match error {
        InputError::ExpectedString { actual } => {
            meta.insert("expected".into(), JsonValue::String("string".into()));
            meta.insert("actual".into(), JsonValue::String(actual));
            (String::new(), "type_mismatch")
        }
        InputError::UnsupportedType { path, actual } => {
            meta.insert("expected".into(), JsonValue::String("json_value".into()));
            meta.insert("actual".into(), JsonValue::String(actual));
            (path, "type_mismatch")
        }
        InputError::InvalidHashKey { path, actual } => {
            meta.insert(
                "expected".into(),
                JsonValue::String("string_or_symbol".into()),
            );
            meta.insert("actual".into(), JsonValue::String(actual));
            (path, "type_mismatch")
        }
    };
    ErrorSet::from_entries(vec![ErrorEntry::new(path, code, meta)])
}

fn error_set_from_materialize_error(error: MaterializeError) -> ErrorSet {
    let mut meta = JsonMap::new();
    let (code, path) = match error {
        MaterializeError::UnsupportedRoot(actual) => {
            meta.insert("expected".into(), JsonValue::String("struct".into()));
            meta.insert("actual".into(), JsonValue::String(actual.into()));
            ("type_mismatch", String::new())
        }
        MaterializeError::InvalidSymbol(name) => {
            meta.insert("from".into(), JsonValue::String("sym".into()));
            meta.insert("value".into(), JsonValue::String(name));
            ("coerce_failed", String::new())
        }
        MaterializeError::InvalidKey(name) => {
            meta.insert("from".into(), JsonValue::String("str".into()));
            meta.insert("value".into(), JsonValue::String(name));
            ("coerce_failed", String::new())
        }
    };
    ErrorSet::from_entries(vec![ErrorEntry::new(path, code, meta)])
}

#[allow(unreachable_code)]
fn raise_validation_error(error_set: ErrorSet) -> ! {
    unsafe {
        let error_value = wrap_error_set(error_set, ERROR_SET_CLASS);
        let exception = rb_funcall(
            VALIDATION_ERROR_CLASS,
            rb_intern(c"new".as_ptr()),
            1,
            error_value,
        );
        rb_exc_raise(exception);
        unreachable_unchecked()
    }
}

fn error_set_from_issues(issues: &[ValidationIssue]) -> ErrorSet {
    let entries = issues
        .iter()
        .map(|issue| ErrorEntry::new(issue.path.clone(), issue.code, issue.meta.clone()))
        .collect();
    ErrorSet::from_entries(entries)
}

unsafe extern "C" fn version(_self: VALUE) -> VALUE {
    unsafe {
        let cstr = CString::new(VERSION).expect("static version");
        rb_utf8_str_new(cstr.as_ptr(), cstr.as_bytes().len() as c_long)
    }
}

unsafe extern "C" fn compile(_self: VALUE, schema_ast: VALUE, opts: VALUE) -> VALUE {
    let options = match unsafe { parse_options(opts) } {
        Ok(parsed) => parsed,
        Err(message) => {
            raise_argument_error(&message);
        }
    };

    match unsafe { SchemaCompiler::compile(schema_ast) } {
        Ok(mut compiled) => {
            *compiled.options_mut() = options;
            unsafe { wrap_compiled_schema(compiled, COMPILED_SCHEMA_CLASS) }
        }
        Err(error) => {
            raise_argument_error(&error.to_string());
        }
    }
}

unsafe fn build_internal(compiled: VALUE, input: VALUE, klass: VALUE, release_gvl: bool) -> VALUE {
    let schema_ptr = schema_from_value(compiled);
    let schema = unsafe { &*schema_ptr };
    let options = *schema.options();

    let prepared_input = match unsafe { prepare_input(input, options.input_mode) } {
        Ok(prepared) => prepared,
        Err(error) => {
            raise_validation_error(error_set_from_input_error(error));
        }
    };

    let schema_ir = schema.ir_arc();
    let result = if release_gvl {
        validate_no_gvl(schema_ir, prepared_input, options)
    } else {
        execute_with_gvl(|| validate_no_gvl(schema_ir, prepared_input, options))
    };

    let ValidationResult { output, issues } = result;

    if !issues.is_empty() {
        raise_validation_error(error_set_from_issues(&issues));
    }

    let prepared = output.expect("validation succeeded but output missing");
    let attr_name = unsafe { ATTR_IVAR_NAME };
    let plan = schema.plan();

    match materialize_instance(
        klass,
        attr_name,
        prepared.value(),
        plan,
        options.freeze,
        options.store_attributes,
    ) {
        Ok(instance) => instance,
        Err(error) => {
            raise_validation_error(error_set_from_materialize_error(error));
        }
    }
}

unsafe extern "C" fn build(_self: VALUE, compiled: VALUE, input: VALUE, klass: VALUE) -> VALUE {
    unsafe { build_internal(compiled, input, klass, true) }
}

unsafe extern "C" fn build_synced(
    _self: VALUE,
    compiled: VALUE,
    input: VALUE,
    klass: VALUE,
) -> VALUE {
    unsafe { build_internal(compiled, input, klass, false) }
}

#[allow(unused_unsafe, unsafe_op_in_unsafe_fn)]
unsafe extern "C" fn validate(_self: VALUE, compiled: VALUE, input: VALUE, opts: VALUE) -> VALUE {
    unsafe {
        let schema_ptr = schema_from_value(compiled);
        let mut options = *(&*schema_ptr).options();
        let nil_value: VALUE = Qnil.into();
        if opts != nil_value {
            options = match unsafe { parse_options(opts) } {
                Ok(parsed) => parsed,
                Err(message) => {
                    raise_argument_error(&message);
                }
            };
        }

        let prepared_input = match prepare_input(input, options.input_mode) {
            Ok(prepared) => prepared,
            Err(error) => {
                let error_set = error_set_from_input_error(error);
                let error_value = wrap_error_set(error_set, ERROR_SET_CLASS);
                let values = [Qfalse.into(), error_value];
                return rb_ary_new_from_values(2, values.as_ptr());
            }
        };

        let schema_ir = (&*schema_ptr).ir_arc();
        let result = validate_no_gvl(schema_ir, prepared_input, options);

        if result.is_valid() {
            let values = [Qtrue.into(), Qnil.into()];
            rb_ary_new_from_values(2, values.as_ptr())
        } else {
            let error_set = error_set_from_issues(&result.issues);
            let error_value = wrap_error_set(error_set, ERROR_SET_CLASS);
            let values = [Qfalse.into(), error_value];
            rb_ary_new_from_values(2, values.as_ptr())
        }
    }
}

unsafe extern "C" fn bench_prepare(_self: VALUE, input: VALUE, mode: VALUE) -> VALUE {
    let mode_string = symbol_to_owned(mode)
        .unwrap_or_else(|| raise_argument_error("Rustly::Core::Bench.prepare expects a Symbol"));
    let input_mode = map_input_mode(&mode_string).unwrap_or_else(|msg| raise_argument_error(&msg));

    match unsafe { prepare_input(input, input_mode) } {
        Ok(_) => {
            let values = [Qtrue.into(), Qnil.into()];
            unsafe { rb_ary_new_from_values(2, values.as_ptr()) }
        }
        Err(error) => {
            let error_set = error_set_from_input_error(error);
            let error_value = unsafe { wrap_error_set(error_set, ERROR_SET_CLASS) };
            let values = [Qfalse.into(), error_value];
            unsafe { rb_ary_new_from_values(2, values.as_ptr()) }
        }
    }
}

unsafe extern "C" fn bench_validate(
    _self: VALUE,
    compiled: VALUE,
    input: VALUE,
    opts: VALUE,
) -> VALUE {
    let schema_ptr = schema_from_value(compiled);
    let options = match unsafe { parse_options(opts) } {
        Ok(parsed) => parsed,
        Err(message) => {
            raise_argument_error(&message);
        }
    };

    let prepared_input = match unsafe { prepare_input(input, options.input_mode) } {
        Ok(prepared) => prepared,
        Err(error) => {
            let error_set = error_set_from_input_error(error);
            let error_value = unsafe { wrap_error_set(error_set, ERROR_SET_CLASS) };
            let values = [Qfalse.into(), error_value];
            return unsafe { rb_ary_new_from_values(2, values.as_ptr()) };
        }
    };

    let schema_ir = unsafe { (&*schema_ptr).ir_arc() };
    let result = validate_no_gvl(schema_ir, prepared_input, options);
    let ValidationResult { output: _, issues } = result;

    if issues.is_empty() {
        let values = [Qtrue.into(), Qnil.into()];
        unsafe { rb_ary_new_from_values(2, values.as_ptr()) }
    } else {
        let error_set = error_set_from_issues(&issues);
        let error_value = unsafe { wrap_error_set(error_set, ERROR_SET_CLASS) };
        let values = [Qfalse.into(), error_value];
        unsafe { rb_ary_new_from_values(2, values.as_ptr()) }
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
            ruby_method!(compiled_schema_id),
            0,
        );
        rb_define_method(
            COMPILED_SCHEMA_CLASS,
            c"summary".as_ptr(),
            ruby_method!(compiled_schema_summary),
            0,
        );

        ERROR_SET_CLASS = rb_define_class_under(CORE_MODULE, c"ErrorSet".as_ptr(), rb_cObject);
        rb_define_alloc_func(ERROR_SET_CLASS, Some(error_set_alloc));
        rb_define_method(
            ERROR_SET_CLASS,
            c"messages".as_ptr(),
            ruby_method!(error_set_messages),
            0,
        );
        rb_define_method(
            ERROR_SET_CLASS,
            c"to_a".as_ptr(),
            ruby_method!(error_set_messages),
            0,
        );

        rb_define_singleton_method(CORE_MODULE, c"version".as_ptr(), ruby_method!(version), 0);
        rb_define_singleton_method(
            CORE_MODULE,
            c"compile".as_ptr(),
            ruby_method!(compile, VALUE, VALUE),
            2,
        );
        rb_define_singleton_method(
            CORE_MODULE,
            c"validate".as_ptr(),
            ruby_method!(validate, VALUE, VALUE, VALUE),
            3,
        );
        rb_define_singleton_method(
            CORE_MODULE,
            c"build".as_ptr(),
            ruby_method!(build, VALUE, VALUE, VALUE),
            3,
        );
        rb_define_singleton_method(
            CORE_MODULE,
            c"build_synced".as_ptr(),
            ruby_method!(build_synced, VALUE, VALUE, VALUE),
            3,
        );

        VALIDATION_ERROR_CLASS =
            rb_define_class_under(CORE_MODULE, c"ValidationError".as_ptr(), rb_eStandardError);

        let bench_module = rb_define_module_under(CORE_MODULE, c"Bench".as_ptr());
        rb_define_singleton_method(
            bench_module,
            c"prepare".as_ptr(),
            ruby_method!(bench_prepare, VALUE, VALUE),
            2,
        );
        rb_define_singleton_method(
            bench_module,
            c"validate".as_ptr(),
            ruby_method!(bench_validate, VALUE, VALUE, VALUE),
            3,
        );

        STRICT_SYMBOL = ensure_symbol("strict");
        EXTRA_SYMBOL = ensure_symbol("extra");
        INPUT_MODE_SYMBOL = ensure_symbol("input_mode");
        FREEZE_SYMBOL = ensure_symbol("freeze");
        STORE_ATTRIBUTES_SYMBOL = ensure_symbol("store_attributes");
        ATTR_IVAR_NAME = c"@attributes".as_ptr();
    }
}

#[cfg(test)]
mod tests {
    use super::errors::{ErrorEntry, ErrorSet};
    use super::schema::CompiledSchema;
    use super::schema::ir::{ExtraBehavior, SchemaIr};
    use serde_json::{Map as JsonMap, Value as JsonValue};

    #[test]
    fn compiled_schema_stores_summary() {
        use crate::schema::MaterializePlan;
        let schema = CompiledSchema::new(
            "example".to_string(),
            SchemaIr::empty(),
            MaterializePlan::empty(),
        );
        assert_eq!(schema.summary(), "example");
        assert!(schema.id() > 0);
    }

    #[test]
    fn error_set_holds_entries() {
        let mut set = ErrorSet::new();
        let mut meta = JsonMap::new();
        meta.insert("expected".into(), JsonValue::String("int".into()));
        set.push(ErrorEntry::new("field".into(), "type_mismatch", meta));
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        let entry = &set.entries()[0];
        assert_eq!(entry.code, "type_mismatch");
        assert_eq!(entry.path, "field");
    }

    #[test]
    fn compiled_schema_options_are_mutable() {
        let mut schema = CompiledSchema::default();
        schema.options_mut().extra_behavior = ExtraBehavior::Allow;
        assert_eq!(schema.options().extra_behavior, ExtraBehavior::Allow);
    }
}
