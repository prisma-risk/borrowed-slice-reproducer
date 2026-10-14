// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! A one-dimensional retained-unit budget with event-ordered refusal.
//!
//! A real meter tracks several dimensions (visits, work, retained units, encoded bytes). One dimension is enough to show the split, because the defect lives entirely in the retained-unit charge.

/// The logical event a charge pays for. A refusal names the event it stopped at, so two replicas that refuse at different events record different outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    /// The decoded row structure every read builds, whatever the payload.
    RowOverhead,
    /// The payload bytes the reader retains.
    Payload,
}

/// A refused charge. It is recorded as the read's replicated outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Refusal {
    /// The event whose charge did not fit.
    pub event: Event,
    /// The budget's limit.
    pub limit: u64,
    /// Units already spent before the refused event.
    pub spent: u64,
}

/// Retained units a single read may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    limit: u64,
    spent: u64,
}

impl Budget {
    /// A budget that admits charges up to `limit` units in total.
    pub fn new(limit: u64) -> Self {
        Self { limit, spent: 0 }
    }

    /// Charges `units` for `event` before the event's effect happens. Checked addition: an overflow counts as a failed check, never wraps.
    pub fn charge(&mut self, event: Event, units: u64) -> Result<(), Refusal> {
        match self.spent.checked_add(units) {
            Some(total) if total <= self.limit => {
                self.spent = total;
                Ok(())
            }
            _ => Err(Refusal {
                event,
                limit: self.limit,
                spent: self.spent,
            }),
        }
    }

    /// Units spent so far.
    pub fn spent(&self) -> u64 {
        self.spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charge_that_reaches_the_limit_exactly_is_admitted() {
        let mut budget = Budget::new(10);
        assert_eq!(budget.charge(Event::RowOverhead, 10), Ok(()));
        assert_eq!(budget.spent(), 10);
    }

    #[test]
    fn a_refused_charge_spends_nothing_and_names_its_event() {
        let mut budget = Budget::new(10);
        budget.charge(Event::RowOverhead, 4).unwrap();
        assert_eq!(
            budget.charge(Event::Payload, 7),
            Err(Refusal {
                event: Event::Payload,
                limit: 10,
                spent: 4
            })
        );
        assert_eq!(budget.spent(), 4);
    }

    #[test]
    fn overflow_refuses_instead_of_wrapping() {
        let mut budget = Budget::new(u64::MAX - 1);
        budget.charge(Event::RowOverhead, 2).unwrap();
        assert!(budget.charge(Event::Payload, u64::MAX).is_err());
    }
}
