// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! The metered read and its two pricings.

use std::borrow::Cow;

use crate::budget::{Budget, Event, Refusal};
use crate::store::{RowId, Store};

/// Retained units for the decoded row structure a read builds, whatever the payload. Synthetic: any fixed price shows the same split.
pub const ROW_OVERHEAD: u64 = 1_000;

/// Retained units per retained payload byte.
pub const PAYLOAD_BYTE: u64 = 1;

/// How a read prices the payload it retains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pricing {
    /// The defect. Retained bytes are charged only when the store hands back an owned copy, after the copy exists. A borrow of a resident row is free. Physically accurate, and replica-dependent.
    OwnedCopyOnly,
    /// The correction. The payload's logical extent is charged before acquisition, whether the store will borrow or copy.
    LogicalExtent,
}

impl Pricing {
    /// The pricing a replica uses unless a test picks one: the correction, or the defect when built with the `owned-copy-meter` feature.
    pub const DEFAULT: Pricing = if cfg!(feature = "owned-copy-meter") {
        Pricing::OwnedCopyOnly
    } else {
        Pricing::LogicalExtent
    };
}

impl Default for Pricing {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Reads one row under `budget` and returns a checksum of its payload, or the refusal. A missing row returns `Ok(None)` and charges nothing.
pub fn read_row(
    store: &Store,
    row: RowId,
    budget: &mut Budget,
    pricing: Pricing,
) -> Result<Option<u64>, Refusal> {
    let Some(extent) = store.extent(row) else {
        return Ok(None);
    };
    budget.charge(Event::RowOverhead, ROW_OVERHEAD)?;
    let bytes = match pricing {
        Pricing::OwnedCopyOnly => {
            // Before: only a released row's copy is charged.
            let bytes = store.payload(row).expect("extent implies a payload");
            if let Cow::Owned(bytes) = &bytes {
                budget.charge(
                    Event::Payload,
                    (bytes.len() as u64).saturating_mul(PAYLOAD_BYTE),
                )?;
            }
            bytes
        }
        Pricing::LogicalExtent => {
            // After: the logical extent is charged before acquisition, whether the row is resident or not.
            budget.charge(Event::Payload, extent.saturating_mul(PAYLOAD_BYTE))?;
            store.payload(row).expect("extent implies a payload")
        }
    };
    Ok(Some(checksum(&bytes)))
}

fn checksum(bytes: &[u8]) -> u64 {
    crate::digest::fnv1a(bytes)
}
