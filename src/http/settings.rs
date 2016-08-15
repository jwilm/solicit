//! State management for HTTP/2 connection settings
//!
//! This module defines a `Settings` trait which is used by other solicit APIs. For convenience, a
//! type that implements the `Settings` trait is also provided.
//!
//! Note that these types do not cause settings frames to be sent; they are simply for recording
//! state.

/// Manage HTTP/2 connection settings
pub trait Settings {
    /// Record the current max concurrent streams
    fn set_max_concurrent_streams(&mut self, u32);

    /// Get the maximum concurrent streams setting
    fn max_concurrent_streams(&self) -> u32;
}

/// Connection settings state
///
/// This type implements the `Settings` trait and may be used anywhere it is accepted.
pub struct SettingsState {
    max_concurrent_streams: u32,
}

impl SettingsState {
    pub fn new(max_concurrent_streams: u32) -> SettingsState {
        SettingsState {
            max_concurrent_streams: max_concurrent_streams,
        }
    }
}

impl Default for SettingsState {
    fn default() -> SettingsState {
        SettingsState {
            // All clients and servers will support at least one concurrent stream.
            max_concurrent_streams: 1,
        }
    }
}

impl Settings for SettingsState {
    #[inline]
    fn set_max_concurrent_streams(&mut self, max_streams: u32) {
        self.max_concurrent_streams = max_streams;
    }

    #[inline]
    fn max_concurrent_streams(&self) -> u32 {
        self.max_concurrent_streams
    }
}

