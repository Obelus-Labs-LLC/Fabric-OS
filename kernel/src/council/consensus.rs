//! Pluggable consensus trait — zero-cost abstraction for single vs multi-node.
//!
//! Single-node: LocalState (in-memory, no coordination overhead)
//! Multi-node: Inject RaftState or HotStuffState in Phase 8+

#![allow(dead_code)]

use fabric_types::governance::PolicyVerdict;

/// A proposed state change from the Council.
#[derive(Clone, Copy, Debug)]
pub struct Proposal {
    pub action: PolicyVerdict,
    pub tier: u8,
    pub tick: u64,
}

/// Pluggable consensus for Council decisions.
pub trait StateAgreement {
    fn propose(&mut self, proposal: Proposal) -> bool;
    fn commit(&mut self, proposal: Proposal) -> bool;
    fn is_leader(&self) -> bool;
    fn cluster_size(&self) -> usize;
}

/// Single-node consensus — all proposals immediately committed. Zero overhead.
pub struct LocalState;

impl LocalState {
    pub const fn new() -> Self { Self }
}

impl StateAgreement for LocalState {
    #[inline(always)]
    fn propose(&mut self, _proposal: Proposal) -> bool { true }
    #[inline(always)]
    fn commit(&mut self, _proposal: Proposal) -> bool { true }
    #[inline(always)]
    fn is_leader(&self) -> bool { true }
    #[inline(always)]
    fn cluster_size(&self) -> usize { 1 }
}
