//! Rust implementation of the `rustly-core` native extension.

use rb_sys::bindings::*;
use rb_sys::special_consts::{Qfalse, Qnil, Qtrue};
use rb_sys::VALUE;
use rustly_core_domain::errors::{ErrorEntry, ErrorSet};
use rustly_core_domain::materialize::{materialize_instance, MaterializeError};
use rustly_core_domain::schema::ir::ExtraBehavior;
use rustly_core_domain::schema::{CompiledSchema, SchemaCompiler};
use rustly_core_domain::validation::{
    prepare_input, validate_no_gvl, FreezeMode, InputError, InputMode, ValidationIssue,
    ValidationOptions, ValidationResult,
};
use rustly_core_gateway::ruby_helpers::{
    ensure_symbol, error_set_from_value, hash_lookup, map_to_ruby, raise_argument_error,
    schema_from_value, symbol_to_owned, truthy, value_to_bool, value_to_json_value, value_to_raw_input,
    wrap_compiled_schema, wrap_error_set, owned_value_to_ruby,
};
use rustly_core_gateway::ruby_method;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::ffi::CString;
use std::hint::unreachable_unchecked;
use std::os::raw::{c_char, c_long};
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
    let _ = schema_from_value(self_value);
    unsafe { rb_utf8_str_new(c"{}".as_ptr() as *const c_char, 2) }
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
        } else if let Some(boolean) = value_to_bool(strict_value) {
            options.strict = boolean;
        } else {
            return Err("opts[:strict] must be true or false".into());
        }
    }

    if let Some(mode_value) = hash_lookup(opts_value, unsafe { INPUT_MODE_SYMBOL }) {
        if mode_value == qnil {
        } else {
            let symbol = symbol_to_owned(mode_value)
                .ok_or_else(|| "opts[:input_mode] must be a Symbol".to_string())?;
            options.input_mode = map_input_mode(&symbol)?;
        }
    }

    if let Some(extra_value) = hash_lookup(opts_value, unsafe { EXTRA_SYMBOL }) {
        if extra_value == qnil {
        } else {
            let symbol = symbol_to_owned(extra_value)
                .ok_or_else(|| "opts[:extra] must be a Symbol".to_string())?;
            options.extra_behavior = map_extra_behavior(&symbol)?;
        }
    }

    if let Some(freeze_value) = hash_lookup(opts_value, unsafe { FREEZE_SYMBOL }) {
        if freeze_value == qnil {
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
        InputError::ExpectedJson => {
            meta.insert("expected".into(), JsonValue::String("json".into()));
            meta.insert("actual".into(), JsonValue::String("structured".into()));
            (String::new(), "type_mismatch")
        }
        InputError::ExpectedOwned => {
            meta.insert("expected".into(), JsonValue::String("structured".into()));
            meta.insert("actual".into(), JsonValue::String("json".into()));
            (String::new(), "type_mismatch")
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

    let json_value = match value_to_json_value(schema_ast) {
        Ok(value) => value,
        Err(message) => {
            raise_argument_error(&message);
        }
    };

    match SchemaCompiler::compile(&json_value) {
        Ok(mut compiled) => {
            *compiled.options_mut() = options;
            unsafe { wrap_compiled_schema(compiled, COMPILED_SCHEMA_CLASS) }
        }
        Err(error) => {
            raise_argument_error(&error.to_string());
        }
    }
}

unsafe fn build_internal(compiled: VALUE, input: VALUE, klass: VALUE) -> VALUE {
    let schema_ptr = schema_from_value(compiled);
    let schema = unsafe { &*schema_ptr };
    let options = *schema.options();

    let raw_input = match value_to_raw_input(input, options.input_mode) {
        Ok(raw) => raw,
        Err(message) => {
            raise_argument_error(&message);
        }
    };

    let prepared_input = match prepare_input(raw_input, options.input_mode) {
        Ok(prepared) => prepared,
        Err(error) => {
            raise_validation_error(error_set_from_input_error(error));
        }
    };

    let schema_ir = schema.ir_arc();
    let result = validate_no_gvl(schema_ir, prepared_input, options);

    let ValidationResult { output, issues } = result;

    if !issues.is_empty() {
        raise_validation_error(error_set_from_issues(&issues));
    }

    let prepared = output.expect("validation succeeded but output missing");
    let plan = schema.plan();

    match materialize_instance(
        prepared.value(),
        plan,
        options.freeze,
        options.store_attributes,
    ) {
        Ok(instance) => {
            let mut result = instance.fields;
            if let Some(attrs) = instance.attributes {
                result.extend(attrs);
            }
            
            let obj = unsafe { rb_obj_alloc(klass) };
            
            for (key, value) in result {
                let key_cstr = std::ffi::CString::new(key).unwrap();
                let key_id =
                    unsafe { rb_intern2(key_cstr.as_ptr(), key_cstr.as_bytes().len() as c_long) };

                let ruby_value = owned_value_to_ruby(&value);

                unsafe { rb_ivar_set(obj, key_id, ruby_value) };
            }
            
            if instance.freeze_mode != FreezeMode::None {
                unsafe {
                    rb_obj_freeze(obj);
                }
            }
            
            obj
        },
        Err(error) => {
            raise_validation_error(error_set_from_materialize_error(error));
        }
    }
}

unsafe extern "C" fn build(_self: VALUE, compiled: VALUE, input: VALUE, klass: VALUE) -> VALUE {
    unsafe { build_internal(compiled, input, klass) }
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

        let raw_input = match value_to_raw_input(input, options.input_mode) {
            Ok(raw) => raw,
            Err(message) => {
                raise_argument_error(&message);
            }
        };

        let prepared_input = match prepare_input(raw_input, options.input_mode) {
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

        VALIDATION_ERROR_CLASS =
            rb_define_class_under(CORE_MODULE, c"ValidationError".as_ptr(), rb_eStandardError);

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
    use rustly_core_domain::errors::{ErrorEntry, ErrorSet};
    use rustly_core_domain::schema::ir::{ExtraBehavior, SchemaIr};
    use rustly_core_domain::schema::CompiledSchema;
    use serde_json::{Map as JsonMap, Value as JsonValue};

    #[test]
    fn compiled_schema_stores_summary() {
        use rustly_core_domain::schema::MaterializePlan;
        let schema = CompiledSchema::new(SchemaIr::empty(), MaterializePlan::empty());
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
