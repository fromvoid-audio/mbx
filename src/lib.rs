//! MBX multiband dynamics.

pub mod dynamics;
pub mod parameters;

#[cfg(all(feature = "plugin", target_os = "macos"))]
mod vst3;
