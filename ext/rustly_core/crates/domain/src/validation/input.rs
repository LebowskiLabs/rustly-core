use super::arena::Arena;
use super::options::InputMode;
use super::value::{extend_value_lifetime, OwnedValue};
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

/// Raw validation input provided by the gateway layer.
#[derive(Debug, Clone)]
pub enum RawInput {
    Owned {
        arena: Arena,
        value: OwnedValue<'static>,
    },
    Json(String),
}

impl RawInput {
    pub fn from_owned(arena: Arena, value: OwnedValue<'_>) -> Self {
        RawInput::Owned {
            arena,
            value: extend_value_lifetime(value),
        }
    }
}

#[derive(Debug, Error)]
pub enum InputError {
    #[error("expected JSON input but received structured data")]
    ExpectedJson,
    #[error("expected structured input but received JSON")]
    ExpectedOwned,
}

pub fn prepare_input(input: RawInput, mode: InputMode) -> Result<PreparedInput, InputError> {
    match mode {
        InputMode::Json => match input {
            RawInput::Json(source) => Ok(PreparedInput::Json(source)),
            RawInput::Owned { .. } => Err(InputError::ExpectedJson),
        },
        InputMode::Ruby | InputMode::Auto => match input {
            RawInput::Owned { arena, value } => {
                Ok(PreparedInput::Owned(PreparedOwned::new(arena, value)))
            }
            RawInput::Json(source) => {
                if mode == InputMode::Auto {
                    Ok(PreparedInput::Json(source))
                } else {
                    Err(InputError::ExpectedOwned)
                }
            }
        },
    }
}
