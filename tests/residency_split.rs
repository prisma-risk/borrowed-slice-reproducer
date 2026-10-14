// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! Two replicas apply the same committed log. One has room to keep every row in memory; the other is under memory pressure and has released the row the last command reads.
//!
//! `cargo test` builds the corrected meter as the default pricing, and every test passes. `cargo test --features owned-copy-meter` builds the defective meter as the default pricing: the two regression tests then fail and print the budgets at which the replicas split.

use borrowed_slice_reproducer::{
    Command, Event, Outcome, PAYLOAD_BYTE, Pricing, ROW_OVERHEAD, Refusal, Replica, Residency,
    RowId,
};

const READ_ROW: RowId = RowId(1);
const OTHER_ROW: RowId = RowId(2);
const PAYLOAD_LEN: u64 = 264;
const OTHER_LEN: u64 = 200;

/// Holds both rows in memory.
const ROOMY_CACHE: usize = 1 << 20;
/// Holds one row at a time, so writing the second releases the first.
const TIGHT_CACHE: usize = 300;

/// What the read costs when its payload is charged: the logical price.
const LOGICAL_COST: u64 = ROW_OVERHEAD + PAYLOAD_LEN * PAYLOAD_BYTE;

fn log(budget: u64) -> Vec<Command> {
    vec![
        Command::Write {
            row: READ_ROW,
            payload: (0..PAYLOAD_LEN).map(|i| i as u8).collect(),
        },
        Command::Write {
            row: OTHER_ROW,
            payload: vec![0xAB; OTHER_LEN as usize],
        },
        Command::Read {
            row: READ_ROW,
            budget,
        },
    ]
}

fn replay(cache_limit: usize, pricing: Pricing, budget: u64) -> Replica {
    let mut replica = Replica::with_pricing(cache_limit, pricing);
    replica.apply_all(&log(budget));
    replica
}

/// Every budget from zero to twice the logical cost at which the two replicas end with different replicated state.
fn split_budgets(pricing: Pricing) -> Vec<u64> {
    let roomy = replay(ROOMY_CACHE, pricing, 0);
    let tight = replay(TIGHT_CACHE, pricing, 0);
    assert_eq!(roomy.residency(READ_ROW), Some(Residency::Resident));
    assert_eq!(tight.residency(READ_ROW), Some(Residency::Released));
    (0..=2 * LOGICAL_COST)
        .filter(|budget| {
            replay(ROOMY_CACHE, pricing, *budget).state_digest()
                != replay(TIGHT_CACHE, pricing, *budget).state_digest()
        })
        .collect()
}

fn describe(budgets: &[u64]) -> String {
    match (budgets.first(), budgets.last()) {
        (Some(first), Some(last)) => format!(
            "replicas split at {} budgets, from {first} to {last}",
            budgets.len()
        ),
        _ => "no split".to_owned(),
    }
}

/// The regression: replicas under different memory pressure reach the same replicated state at every budget. Fails under `--features owned-copy-meter`.
#[test]
fn replicas_under_different_memory_pressure_agree_at_every_budget() {
    let splits = split_budgets(Pricing::DEFAULT);
    assert!(splits.is_empty(), "{}", describe(&splits));
}

#[derive(Debug, Clone, Copy)]
enum State {
    Resident,
    Released,
    /// Released, then reloaded into the cache.
    Hydrated,
}

fn read_in_state(state: State, pricing: Pricing, budget: u64) -> Outcome {
    let mut replica = Replica::with_pricing(ROOMY_CACHE, pricing);
    let log = log(budget);
    replica.apply_all(&log[..2]);
    match state {
        State::Resident => {}
        State::Released => replica.release(READ_ROW),
        State::Hydrated => {
            replica.release(READ_ROW);
            replica.hydrate(READ_ROW);
        }
    }
    replica.apply(&log[2])
}

/// The smallest admitting budget, and the outcome one unit below it.
fn boundary(state: State, pricing: Pricing) -> (u64, Outcome) {
    let admit = (0..=2 * LOGICAL_COST)
        .find(|budget| matches!(read_in_state(state, pricing, *budget), Outcome::Read(_)))
        .expect("the read admits at some budget");
    (admit, read_in_state(state, pricing, admit - 1))
}

/// The regression in one replica: the same read has one price and one refusal in every residency state. Fails under `--features owned-copy-meter`.
#[test]
fn one_read_has_one_boundary_in_every_residency_state() {
    let expected = (
        LOGICAL_COST,
        Outcome::Refused(Refusal {
            event: Event::Payload,
            limit: LOGICAL_COST - 1,
            spent: ROW_OVERHEAD,
        }),
    );
    for state in [State::Resident, State::Released, State::Hydrated] {
        assert_eq!(
            boundary(state, Pricing::DEFAULT),
            expected,
            "{state:?}: admit at the logical cost, refuse one unit below at the payload event"
        );
    }
}

/// The defect, shown explicitly in every build: charging only an owned copy splits the replicas at exactly the budgets between the resident charge and the released charge.
#[test]
fn owned_copy_meter_splits_replicas_between_the_two_charges() {
    let expected: Vec<u64> = (ROW_OVERHEAD..LOGICAL_COST).collect();
    assert_eq!(split_budgets(Pricing::OwnedCopyOnly), expected);
    assert_eq!(
        boundary(State::Resident, Pricing::OwnedCopyOnly).0,
        ROW_OVERHEAD
    );
    assert_eq!(
        boundary(State::Hydrated, Pricing::OwnedCopyOnly).0,
        ROW_OVERHEAD
    );
    assert_eq!(
        boundary(State::Released, Pricing::OwnedCopyOnly).0,
        LOGICAL_COST
    );
}

/// The correction, shown explicitly in every build.
#[test]
fn logical_extent_meter_never_splits() {
    assert_eq!(split_budgets(Pricing::LogicalExtent), Vec::<u64>::new());
}
