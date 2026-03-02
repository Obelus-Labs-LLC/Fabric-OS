//! Capability store — the central in-memory registry for all live capability tokens.
//!
//! Uses BTreeMap<u64, StoredToken> for O(log n) lookup by ID. Each StoredToken
//! wraps the wire CapabilityToken with kernel-private fields (HMAC, budget config,
//! creation tick).

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use fabric_types::{CapabilityToken, CapabilityId, ResourceId, ProcessId, Perm, Budget};
use crate::capability::hmac_engine;
use crate::capability::budget::BudgetTracker;
use crate::capability::nonce::NonceTracker;

/// Maximum tokens in the store (prevents unbounded heap growth).
const MAX_TOKENS: usize = 65536;

/// Kernel-private token wrapper. Extends the wire CapabilityToken with
/// fields that never leave Ring 0.
pub struct StoredToken {
    pub token: CapabilityToken,
    pub hmac: [u8; 32],
    pub budget: Option<Budget>,
    pub created_at: u64,
}

/// Error types for capability operations.
#[derive(Debug)]
pub enum CapabilityError {
    NotFound,
    InvalidHmac,
    Expired,
    InsufficientPermission,
    BudgetExhausted,
    NonceReplay,
    DelegationDenied,
    PermissionEscalation,
    StoreFull,
}

/// The capability store holding all live tokens and associated tracking state.
pub struct CapabilityStore {
    tokens: BTreeMap<u64, StoredToken>,
    next_id: u64,
    budget_tracker: BudgetTracker,
    nonce_tracker: NonceTracker,
    current_tick: u64,
}

impl CapabilityStore {
    pub const fn new() -> Self {
        Self {
            tokens: BTreeMap::new(),
            next_id: 1,
            budget_tracker: BudgetTracker::new(),
            nonce_tracker: NonceTracker::new(),
            current_tick: 0,
        }
    }

    /// Allocate the next unique capability ID.
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Create a new root capability (not delegated).
    pub fn create(
        &mut self,
        resource: ResourceId,
        permissions: Perm,
        owner: ProcessId,
        expires: Option<u32>,
        budget: Option<Budget>,
    ) -> Result<CapabilityId, CapabilityError> {
        if self.tokens.len() >= MAX_TOKENS {
            return Err(CapabilityError::StoreFull);
        }

        let id = self.alloc_id();

        let mut token = CapabilityToken::zeroed();
        token.version = CapabilityToken::VERSION;
        token.permissions = permissions;
        token.owner = owner;
        token.id = CapabilityId::new(id);
        token.resource = resource;
        token.delegated_from = 0;
        token.nonce = 0;
        token.expires = expires.unwrap_or(0);

        let hmac = hmac_engine::sign(&token.active_bytes());

        self.tokens.insert(id, StoredToken {
            token,
            hmac,
            budget,
            created_at: self.current_tick,
        });

        Ok(CapabilityId::new(id))
    }

    /// Delegate: create a child capability from a parent.
    /// Parent must have GRANT permission. Child permissions must be a subset of parent's.
    pub fn delegate(
        &mut self,
        parent_id: u64,
        new_owner: ProcessId,
        permissions: Perm,
        expires: Option<u32>,
        budget: Option<Budget>,
    ) -> Result<CapabilityId, CapabilityError> {
        if self.tokens.len() >= MAX_TOKENS {
            return Err(CapabilityError::StoreFull);
        }

        // Validate parent exists and has GRANT
        let parent = self.tokens.get(&parent_id)
            .ok_or(CapabilityError::NotFound)?;

        if !parent.token.permissions.contains(Perm::GRANT) {
            return Err(CapabilityError::DelegationDenied);
        }

        // Child permissions must be subset of parent (no escalation)
        if !permissions.is_subset_of(parent.token.permissions) {
            return Err(CapabilityError::PermissionEscalation);
        }

        // Child expiration must not exceed parent's
        let parent_expires = parent.token.expires;
        let child_expires = match (expires, parent_expires) {
            (Some(e), pe) if pe > 0 && e > pe => pe,
            (Some(e), _) => e,
            (None, pe) => pe,
        };

        let resource = parent.token.resource;
        let id = self.alloc_id();

        let mut token = CapabilityToken::zeroed();
        token.version = CapabilityToken::VERSION;
        token.permissions = permissions;
        token.owner = new_owner;
        token.id = CapabilityId::new(id);
        token.resource = resource;
        token.delegated_from = parent_id;
        token.nonce = 0;
        token.expires = child_expires;

        let hmac = hmac_engine::sign(&token.active_bytes());

        self.tokens.insert(id, StoredToken {
            token,
            hmac,
            budget,
            created_at: self.current_tick,
        });

        Ok(CapabilityId::new(id))
    }

    /// Validate a token for a specific operation.
    /// Checks: exists, HMAC integrity, not expired, permissions sufficient,
    /// nonce valid, budget not exhausted.
    pub fn validate(
        &mut self,
        token_id: u64,
        required_perm: Perm,
        presented_nonce: u32,
    ) -> Result<(), CapabilityError> {
        // 1. Lookup
        let stored = self.tokens.get(&token_id)
            .ok_or(CapabilityError::NotFound)?;

        // 2. HMAC integrity
        if !hmac_engine::verify(&stored.token.active_bytes(), &stored.hmac) {
            return Err(CapabilityError::InvalidHmac);
        }

        // 3. Expiration
        if stored.token.expires > 0 {
            let deadline = stored.created_at + stored.token.expires as u64;
            if self.current_tick >= deadline {
                return Err(CapabilityError::Expired);
            }
        }

        // 4. Permissions
        if !stored.token.permissions.contains(required_perm) {
            return Err(CapabilityError::InsufficientPermission);
        }

        // 5. Nonce replay prevention
        if !self.nonce_tracker.check_and_advance(token_id, presented_nonce) {
            return Err(CapabilityError::NonceReplay);
        }

        // 6. Budget enforcement
        let budget = stored.budget;
        if let Some(ref b) = budget {
            if !self.budget_tracker.check_and_consume(token_id, b, self.current_tick) {
                return Err(CapabilityError::BudgetExhausted);
            }
        }

        Ok(())
    }

    /// Revoke a token and all its descendants (cascading revocation).
    /// Returns the count of tokens removed.
    pub fn revoke(&mut self, token_id: u64) -> Result<usize, CapabilityError> {
        if !self.tokens.contains_key(&token_id) {
            return Err(CapabilityError::NotFound);
        }

        // BFS to find all descendants
        let mut to_revoke: Vec<u64> = Vec::new();
        to_revoke.push(token_id);
        let mut i = 0;

        while i < to_revoke.len() {
            let parent = to_revoke[i];
            for (&id, stored) in self.tokens.iter() {
                if stored.token.delegated_from == parent && !to_revoke.contains(&id) {
                    to_revoke.push(id);
                }
            }
            i += 1;
        }

        let count = to_revoke.len();
        for id in &to_revoke {
            self.tokens.remove(id);
            self.budget_tracker.remove(*id);
            self.nonce_tracker.remove(*id);
        }

        Ok(count)
    }

    /// Get a stored token by ID (no validation, just lookup).
    pub fn get(&self, token_id: u64) -> Option<&StoredToken> {
        self.tokens.get(&token_id)
    }

    /// Get the owner and resource of a capability (for bus router sender verification).
    pub fn get_token_info(&self, token_id: u64) -> Option<(ProcessId, ResourceId)> {
        self.tokens.get(&token_id).map(|s| (s.token.owner, s.token.resource))
    }

    /// Advance the monotonic tick counter.
    pub fn tick(&mut self) {
        self.current_tick += 1;
    }

    /// Advance the tick counter by a specific amount.
    pub fn advance_ticks(&mut self, n: u64) {
        self.current_tick += n;
    }

    /// Get the current tick.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Number of live tokens.
    pub fn count(&self) -> usize {
        self.tokens.len()
    }

    /// Clear the entire store (for testing).
    pub fn clear(&mut self) {
        self.tokens.clear();
        self.budget_tracker.clear();
        self.nonce_tracker.clear();
        self.next_id = 1;
        self.current_tick = 0;
    }
}
