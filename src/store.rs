// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! A row store with a durable tier and a bounded in-memory cache.
//!
//! The durable tier is replicated state: every replica holds the same rows after applying the same log. The cache is not. Each replica sizes it from its own memory, so which rows are resident depends on local configuration and history.

use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};

/// Identifies a stored row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowId(pub u64);

/// Where a row's payload currently lives on this replica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residency {
    /// The payload is cached in memory; a read borrows it.
    Resident,
    /// The payload was released from the cache; a read reloads an owned copy from the durable tier.
    Released,
}

#[derive(Debug)]
struct Stored {
    /// Stands in for the on-disk copy. Replicated.
    durable: Vec<u8>,
    /// The in-memory copy, when resident. Replica-local.
    cached: Option<Vec<u8>>,
}

/// A row store. Its durable rows are replicated; its cache is local to one replica.
#[derive(Debug)]
pub struct Store {
    rows: BTreeMap<RowId, Stored>,
    cache_limit: usize,
    cached_bytes: usize,
    /// Cached rows from oldest to newest; the oldest is released first under pressure.
    cache_order: VecDeque<RowId>,
}

impl Store {
    /// An empty store that keeps at most `cache_limit` payload bytes in memory.
    pub fn new(cache_limit: usize) -> Self {
        Self {
            rows: BTreeMap::new(),
            cache_limit,
            cached_bytes: 0,
            cache_order: VecDeque::new(),
        }
    }

    /// Writes a row durably and caches it if the cache can hold it, releasing older rows first.
    pub fn write(&mut self, row: RowId, payload: Vec<u8>) {
        self.release(row);
        self.rows.insert(
            row,
            Stored {
                durable: payload,
                cached: None,
            },
        );
        self.hydrate(row);
    }

    /// The payload's logical length, known from the row's metadata without acquiring the payload.
    pub fn extent(&self, row: RowId) -> Option<u64> {
        self.rows
            .get(&row)
            .map(|stored| stored.durable.len() as u64)
    }

    /// The payload: borrowed when the row is resident, an owned copy reloaded from the durable tier when it was released.
    pub fn payload(&self, row: RowId) -> Option<Cow<'_, [u8]>> {
        let stored = self.rows.get(&row)?;
        Some(match &stored.cached {
            Some(bytes) => Cow::Borrowed(bytes.as_slice()),
            None => Cow::Owned(stored.durable.clone()),
        })
    }

    /// The row's residency on this replica.
    pub fn residency(&self, row: RowId) -> Option<Residency> {
        self.rows.get(&row).map(|stored| match stored.cached {
            Some(_) => Residency::Resident,
            None => Residency::Released,
        })
    }

    /// Drops the row's cached copy. Its durable copy stays.
    pub fn release(&mut self, row: RowId) {
        if let Some(bytes) = self
            .rows
            .get_mut(&row)
            .and_then(|stored| stored.cached.take())
        {
            self.cached_bytes -= bytes.len();
            self.cache_order.retain(|cached| *cached != row);
        }
    }

    /// Reloads a released row into the cache, releasing the oldest cached rows until it fits. A row larger than the whole cache stays released.
    pub fn hydrate(&mut self, row: RowId) {
        let Some(len) = self
            .rows
            .get(&row)
            .filter(|stored| stored.cached.is_none())
            .map(|stored| stored.durable.len())
        else {
            return;
        };
        if len > self.cache_limit {
            return;
        }
        while self.cached_bytes + len > self.cache_limit {
            let Some(oldest) = self.cache_order.front().copied() else {
                break;
            };
            self.release(oldest);
        }
        if let Some(stored) = self.rows.get_mut(&row) {
            stored.cached = Some(stored.durable.clone());
            self.cached_bytes += len;
            self.cache_order.push_back(row);
        }
    }

    /// The durable rows in key order: the store's replicated content.
    pub(crate) fn durable_rows(&self) -> impl Iterator<Item = (RowId, &[u8])> {
        self.rows
            .iter()
            .map(|(row, stored)| (*row, stored.durable.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_is_borrowed_when_resident_and_owned_when_released() {
        let mut store = Store::new(1_024);
        store.write(RowId(1), vec![7; 16]);
        assert!(matches!(store.payload(RowId(1)), Some(Cow::Borrowed(_))));
        store.release(RowId(1));
        assert!(matches!(store.payload(RowId(1)), Some(Cow::Owned(_))));
        store.hydrate(RowId(1));
        assert!(matches!(store.payload(RowId(1)), Some(Cow::Borrowed(_))));
    }

    #[test]
    fn memory_pressure_releases_the_oldest_cached_row() {
        let mut store = Store::new(100);
        store.write(RowId(1), vec![1; 60]);
        store.write(RowId(2), vec![2; 60]);
        assert_eq!(store.residency(RowId(1)), Some(Residency::Released));
        assert_eq!(store.residency(RowId(2)), Some(Residency::Resident));
        assert_eq!(store.extent(RowId(1)), Some(60));
    }
}
