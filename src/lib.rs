// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! A standalone reproducer for an ownership-induced split in a replicated state machine.
//!
//! A store returns a row's payload as a `Cow<[u8]>`: borrowed when the row is resident in memory, owned when the row was released and has to be reloaded. Residency is local to each replica. It follows that replica's memory pressure and history, not the replicated log.
//!
//! A meter that charges retained bytes only for an owned copy therefore charges the same committed read differently on two replicas. At any budget between the two charges, one replica admits the read and the other refuses it. The refusal is a replicated outcome, so the replicas disagree about history while agreeing on the log.
//!
//! The corrected meter charges the payload's logical extent before acquiring it, whatever the residency. Every replica then reaches the same decision at every budget.
//!
//! The crate uses only `std` and contains no code from any production system.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod budget;
mod digest;
mod read;
mod replica;
mod store;

pub use budget::{Budget, Event, Refusal};
pub use read::{PAYLOAD_BYTE, Pricing, ROW_OVERHEAD, read_row};
pub use replica::{Command, Outcome, Replica};
pub use store::{Residency, RowId, Store};
