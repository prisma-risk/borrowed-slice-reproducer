// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! A replica: a store, a pricing, and the replicated record of every applied command's outcome.

use crate::budget::{Budget, Refusal};
use crate::digest;
use crate::read::{Pricing, read_row};
use crate::store::{Residency, RowId, Store};

/// A committed log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Store a row.
    Write {
        /// The row.
        row: RowId,
        /// Its payload.
        payload: Vec<u8>,
    },
    /// Read a row under a retained-unit budget. The budget is part of the command, so every replica applies the same one.
    Read {
        /// The row.
        row: RowId,
        /// The retained-unit budget.
        budget: u64,
    },
}

/// The replicated outcome of one applied command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// A write was stored.
    Written,
    /// A read was admitted; the checksum of what it read.
    Read(u64),
    /// A read named a row that does not exist.
    Missing,
    /// A read was refused.
    Refused(Refusal),
}

/// One replica of the state machine.
#[derive(Debug)]
pub struct Replica {
    store: Store,
    pricing: Pricing,
    outcomes: Vec<Outcome>,
}

impl Replica {
    /// A replica whose cache holds at most `cache_limit` payload bytes, using [`Pricing::DEFAULT`].
    pub fn new(cache_limit: usize) -> Self {
        Self::with_pricing(cache_limit, Pricing::DEFAULT)
    }

    /// A replica with an explicit pricing.
    pub fn with_pricing(cache_limit: usize, pricing: Pricing) -> Self {
        Self {
            store: Store::new(cache_limit),
            pricing,
            outcomes: Vec::new(),
        }
    }

    /// Applies one committed command and records its outcome. Admission happens before any effect, so a refused read changes nothing but its own record.
    pub fn apply(&mut self, command: &Command) -> Outcome {
        let outcome = match command {
            Command::Write { row, payload } => {
                self.store.write(*row, payload.clone());
                Outcome::Written
            }
            Command::Read { row, budget } => {
                let mut budget = Budget::new(*budget);
                match read_row(&self.store, *row, &mut budget, self.pricing) {
                    Ok(Some(checksum)) => Outcome::Read(checksum),
                    Ok(None) => Outcome::Missing,
                    Err(refusal) => Outcome::Refused(refusal),
                }
            }
        };
        self.outcomes.push(outcome);
        outcome
    }

    /// Applies every command in order.
    pub fn apply_all<'a>(&mut self, log: impl IntoIterator<Item = &'a Command>) {
        for command in log {
            self.apply(command);
        }
    }

    /// Releases a row from this replica's cache. Local: no command does this.
    pub fn release(&mut self, row: RowId) {
        self.store.release(row);
    }

    /// Reloads a released row into this replica's cache. Local: no command does this.
    pub fn hydrate(&mut self, row: RowId) {
        self.store.hydrate(row);
    }

    /// The row's residency on this replica.
    pub fn residency(&self, row: RowId) -> Option<Residency> {
        self.store.residency(row)
    }

    /// The recorded outcomes, in log order.
    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// A digest of replicated state: the durable rows and every recorded outcome. Residency and cache contents are deliberately excluded.
    pub fn state_digest(&self) -> u64 {
        let mut state = digest::fnv1a(b"replicated-state");
        for (row, payload) in self.store.durable_rows() {
            state = digest::extend(state, &row.0.to_le_bytes());
            state = digest::extend(state, &(payload.len() as u64).to_le_bytes());
            state = digest::extend(state, payload);
        }
        for outcome in &self.outcomes {
            state = match outcome {
                Outcome::Written => digest::extend(state, &[0]),
                Outcome::Read(checksum) => {
                    digest::extend(digest::extend(state, &[1]), &checksum.to_le_bytes())
                }
                Outcome::Missing => digest::extend(state, &[2]),
                Outcome::Refused(refusal) => {
                    let event = [3, refusal.event as u8];
                    let state = digest::extend(state, &event);
                    let state = digest::extend(state, &refusal.limit.to_le_bytes());
                    digest::extend(state, &refusal.spent.to_le_bytes())
                }
            };
        }
        state
    }
}
