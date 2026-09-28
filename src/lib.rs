//! Lueur — ultra-light RGB controller for Windows.
//!
//! Effects run inside the lighting controllers themselves: Lueur writes the
//! settings once, then sleeps until something changes (new setting, resume
//! from sleep). No render loop, no polling.

pub mod autostart;
pub mod config;
pub mod devices;
pub mod effect;
pub mod hid;
pub mod pawnio;
pub mod smbus;
pub mod util;
