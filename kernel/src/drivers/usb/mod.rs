//! USB Subsystem — Phase 21a+.
//!
//! Host controller drivers for USB. Currently implements xHCI (USB 3.0).
//! Future phases will add EHCI (USB 2.0 fallback), hub enumeration,
//! device class drivers (HID, mass storage, etc.).

#![allow(dead_code)]

pub mod xhci;
