//! Reference driver implementations for Fabric OS HAL.
//!
//! Each driver implements the Driver trait and communicates via the
//! typed message bus. Drivers are "pure" — they never touch bus or
//! capability locks directly.

pub mod serial_driver;
pub mod timer_driver;
pub mod ramdisk_driver;
pub mod framebuffer_driver;
