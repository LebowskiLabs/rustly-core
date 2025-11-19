use crate::schema::ir::ExtraBehavior;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Auto,
    Json,
    Ruby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeMode {
    Deep,
    Shallow,
    None,
}

#[derive(Debug, Clone, Copy)]
pub struct ValidationOptions {
    pub strict: bool,
    pub input_mode: InputMode,
    pub extra_behavior: ExtraBehavior,
    pub freeze: FreezeMode,
    pub store_attributes: bool,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            strict: false,
            input_mode: InputMode::Auto,
            extra_behavior: ExtraBehavior::Forbid,
            freeze: FreezeMode::None,
            store_attributes: false,
        }
    }
}
