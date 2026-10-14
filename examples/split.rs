// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! Prints, for each pricing, what two replicas decide about the same committed read at every budget.
//!
//! Run with `cargo run --example split`.

use borrowed_slice_reproducer::{Command, Outcome, Pricing, Replica, Residency, RowId};

fn main() {
    let read_row = RowId(1);
    let log = |budget| {
        vec![
            Command::Write {
                row: read_row,
                payload: (0..264u32).map(|i| i as u8).collect(),
            },
            Command::Write {
                row: RowId(2),
                payload: vec![0xAB; 200],
            },
            Command::Read {
                row: read_row,
                budget,
            },
        ]
    };
    let decide = |cache_limit, pricing, budget| {
        let mut replica = Replica::with_pricing(cache_limit, pricing);
        replica.apply_all(&log(budget));
        let admitted = matches!(replica.outcomes().last(), Some(Outcome::Read(_)));
        (
            admitted,
            replica.residency(read_row),
            replica.state_digest(),
        )
    };

    for pricing in [Pricing::OwnedCopyOnly, Pricing::LogicalExtent] {
        let mut first_admit = [None, None];
        let mut splits = Vec::new();
        for budget in 0..=2_000u64 {
            let roomy = decide(1 << 20, pricing, budget);
            let tight = decide(300, pricing, budget);
            debug_assert_eq!(roomy.1, Some(Residency::Resident));
            debug_assert_eq!(tight.1, Some(Residency::Released));
            for (slot, (admitted, ..)) in first_admit.iter_mut().zip([roomy, tight]) {
                if admitted && slot.is_none() {
                    *slot = Some(budget);
                }
            }
            if roomy.2 != tight.2 {
                splits.push(budget);
            }
        }
        println!("{pricing:?}");
        println!(
            "  replica with the row resident admits from budget {}",
            first_admit[0].unwrap_or_default()
        );
        println!(
            "  replica with the row released admits from budget {}",
            first_admit[1].unwrap_or_default()
        );
        match (splits.first(), splits.last()) {
            (Some(first), Some(last)) => println!(
                "  replicated state differs at {} budgets, {first} to {last}",
                splits.len()
            ),
            _ => println!("  replicated state is identical at every budget"),
        }
    }
}
