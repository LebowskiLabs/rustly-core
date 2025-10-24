use bumpalo::{Bump, collections::Vec as BumpVec};
use std::sync::Arc;

#[derive(Debug)]
struct ArenaInner {
    bump: Bump,
}

impl ArenaInner {
    fn new() -> Self {
        Self { bump: Bump::new() }
    }

    fn with_capacity(initial_bytes: usize) -> Self {
        Self {
            bump: Bump::with_capacity(initial_bytes),
        }
    }
}

/// Arena wrapper that centralizes bump allocation for transient validation data.
#[allow(clippy::arc_with_non_send_sync)]
#[derive(Clone, Debug)]
pub struct Arena {
    inner: Arc<ArenaInner>,
}

impl Arena {
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ArenaInner::new()),
        }
    }

    #[allow(dead_code)]
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn with_capacity(initial_bytes: usize) -> Self {
        Self {
            inner: Arc::new(ArenaInner::with_capacity(initial_bytes)),
        }
    }

    #[inline]
    pub fn alloc_str_from_bytes<'arena>(&'arena self, bytes: &[u8]) -> &'arena str {
        let copied = self.inner.bump.alloc_slice_copy(bytes);
        // SAFETY: callers responsible for providing valid UTF-8 when allocating strings.
        unsafe { std::str::from_utf8_unchecked(copied) }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn bump_vec<'arena, T>(&'arena self) -> BumpVec<'arena, T> {
        BumpVec::new_in(&self.inner.bump)
    }

    #[inline]
    pub fn bump_vec_with_capacity<'arena, T>(&'arena self, capacity: usize) -> BumpVec<'arena, T> {
        let mut vec = BumpVec::new_in(&self.inner.bump);
        vec.reserve(capacity);
        vec
    }

    #[inline]
    #[allow(dead_code)]
    pub fn allocator(&self) -> &Bump {
        &self.inner.bump
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

pub type BVec<'arena, T> = BumpVec<'arena, T>;

// SAFETY: `Arena` is only transferred across threads after validation completes and never used
// concurrently. `Bump` permits thread-local use, and we do not implement `Sync`.
unsafe impl Send for Arena {}
