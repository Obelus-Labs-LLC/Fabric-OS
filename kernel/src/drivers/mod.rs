//! Hardware PCI Drivers — Phase 20A+.
//!
//! Contains drivers for real hardware devices. Each driver implements
//! the NicDriver trait (for NICs) or other device traits as needed.

#![allow(dead_code)]

pub mod e1000e;
