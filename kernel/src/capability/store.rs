//! Capability store — the central in-memory registry for all live capability tokens.
//!
//! Uses FixedMap<StoredToken> for O(1) amortized lookup by ID (TD-008).
//! Each StoredToken wraps the wire CapabilityToken with kernel-private fields
//! (HMAC, budget config, creation tick) and intrusive sibling pointers for
//! the parent→children relationship.
//!
//! Phase 6: Added parent→children index for O(n) cascading revocation (TD-004).
//! TD-008: Replaced BTreeMap with alloc-free FixedMap + intrusive child list.

#![allow(dead_code)]

use alloc::vec::Vec;
use fabric_types::{CapabilityToken, CapabilityId, ResourceId, ProcessId, Perm, Budget};
use crate::capability::hmac_engine;
use crate::capability::budget::BudgetTracker;
use crate::capability::nonce::NonceTracker;
use super::slab::FixedMap;

/// Maximum tokens in the store (75% of FixedMap capacity = 12288).
const MAX_TOKENS: usize = 12288;

/// Kernel-private token wrapper. Extends the wire CapabilityToken with
/// fields that never leave Ring 0.
///
/// Includes intrusive linked-list pointers for parent→children relationships,
/// eliminating the separate BTreeMap<u64, Vec<u64>> children index.
pub struct StoredToken {
    pub token: CapabilityToken,
    pub hmac: [u8; 32],
    pub budget: Option<Budget>,
    pub created_at: u64,
    /// First child capability ID (intrusive linked list head), 0 = no children.
    pub first_child: u64,
    /// Next sibling capability ID (intrusive linked list), 0 = end of list.
    pub next_sibling: u64,
}

/// Error types for capability operations.
#[derive(Debug)]
#[must_use]
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
    tokens: FixedMap<StoredToken>,
    next_id: u64,
    budget_tracker: BudgetTracker,
    nonce_tracker: NonceTracker,
    current_tick: u64,
}

impl CapabilityStore {
    pub const fn new() -> Self {
        Self {
            tokens: FixedMap::new(),
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
    #[must_use]
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
            first_child: 0,
            next_sibling: 0,
        });

        Ok(CapabilityId::new(id))
    }

    /// Delegate: create a child capability from a parent.
    /// Parent must have GRANT permission. Child permissions must be a subset of parent's.
    #[must_use]
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
        let old_first_child = parent.first_child;
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

        // Insert new child with next_sibling pointing to parent's old first_child
        self.tokens.insert(id, StoredToken {
            token,
            hmac,
            budget,
            created_at: self.current_tick,
            first_child: 0,
            next_sibling: old_first_child,
        });

        // Update parent's first_child to point to new child (prepend)
        if let Some(parent) = self.tokens.get_mut(&parent_id) {
            parent.first_child = id;
        }

        Ok(CapabilityId::new(id))
    }

    /// Validate a token for a specific operation.
    /// Checks: exists, HMAC integrity, not expired, permissions sufficient,
    /// nonce valid, budget not exhausted.
    #[must_use]
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

    /// Unlink a child from its parent's intrusive child list.
    fn unlink_child(&mut self, parent_id: u64, child_id: u64) {
        // Read child's next_sibling before modifying anything
        let child_next = self.tokens.get(&child_id)
            .map(|s| s.next_sibling)
            .unwrap_or(0);

        let parent_first = self.tokens.get(&parent_id)
            .map(|s| s.first_child)
            .unwrap_or(0);

        if parent_first == child_id {
            // Child is head of list — advance parent's first_child
            if let Some(parent) = self.tokens.get_mut(&parent_id) {
                parent.first_child = child_next;
            }
        } else {
            // Walk to find predecessor in sibling chain
            let mut prev_id = parent_first;
            while prev_id != 0 {
                let next = self.tokens.get(&prev_id)
                    .map(|s| s.next_sibling)
                    .unwrap_or(0);
                if next == child_id {
                    if let Some(prev) = self.tokens.get_mut(&prev_id) {
                        prev.next_sibling = child_next;
                    }
                    break;
                }
                prev_id = next;
            }
        }
    }

    /// Collect children IDs by walking the intrusive linked list.
    fn collect_children(&self, parent_id: u64) -> Vec<u64> {
        let mut children = Vec::new();
        let mut child_id = self.tokens.get(&parent_id)
            .map(|s| s.first_child)
            .unwrap_or(0);
        while child_id != 0 {
            children.push(child_id);
            child_id = self.tokens.get(&child_id)
                .map(|s| s.next_sibling)
                .unwrap_or(0);
        }
        children
    }

    /// Revoke a token and all its descendants (cascading revocation).
    /// Returns the count of tokens removed.
    ///
    /// TD-004: Uses intrusive child list for O(n) tree walk.
    /// TD-008: No separate children BTreeMap — uses first_child/next_sibling.
    #[must_use]
    pub fn revoke(&mut self, token_id: u64) -> Result<usize, CapabilityError> {
        // Get parent ID before we start removing
        let parent_id = self.tokens.get(&token_id)
            .ok_or(CapabilityError::NotFound)?
            .token.delegated_from;

        // Unlink from parent's child list (if delegated)
        if parent_id != 0 {
            self.unlink_child(parent_id, token_id);
        }

        // DFS via intrusive child list — O(n) where n = number of descendants
        let mut stack: Vec<u64> = Vec::new();
        stack.push(token_id);
        let mut count = 0;

        while let Some(id) = stack.pop() {
            // Collect children before removing the token
            let children = self.collect_children(id);
            stack.extend(children);

            // Remove the token itself
            if self.tokens.remove(&id).is_some() {
                self.budget_tracker.remove(id);
                self.nonce_tracker.remove(id);
                count += 1;
            }
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
