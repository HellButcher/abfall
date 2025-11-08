//! Tri-color marking system for garbage collection
//!
//! This module implements the color states used in the tri-color marking algorithm:
//! - White: Objects that are potentially unreachable
//! - Gray: Objects that are reachable but not yet scanned
//! - Black: Objects that are reachable and fully scanned

use std::sync::atomic::{AtomicU8, Ordering};

/// The color of an object in the tri-color marking algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    /// Potentially unreachable, candidate for collection
    White = 0,
    /// Reachable but not yet scanned
    Gray = 1,
    /// Reachable and fully scanned
    Black = 2,
}

impl From<u8> for Color {
    fn from(value: u8) -> Self {
        match value {
            0 => Color::White,
            1 => Color::Gray,
            2 => Color::Black,
            _ => Color::White,
        }
    }
}

/// Thread-safe atomic color storage
pub struct AtomicColor {
    inner: AtomicU8,
}

impl AtomicColor {
    pub fn new(color: Color) -> Self {
        Self {
            inner: AtomicU8::new(color as u8),
        }
    }

    #[allow(dead_code)]
    pub fn load(&self, ordering: Ordering) -> Color {
        Color::from(self.inner.load(ordering))
    }

    pub fn store(&self, color: Color, ordering: Ordering) {
        self.inner.store(color as u8, ordering);
    }

    #[allow(dead_code)]
    pub fn compare_exchange(
        &self,
        current: Color,
        new: Color,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Color, Color> {
        self.inner
            .compare_exchange(current as u8, new as u8, success, failure)
            .map(Color::from)
            .map_err(Color::from)
    }
}
