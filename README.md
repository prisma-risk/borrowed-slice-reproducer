# Borrowed-slice reproducer

[![DOI](https://zenodo.org/badge/1419939792.svg)](https://doi.org/10.5281/zenodo.23381245)

A small, self-contained Rust crate that reproduces the ownership-induced split described in the paper *When a Borrowed Slice Splits Replicas: Deterministic Resource Metering in a Replicated State Machine*. It uses only the Rust standard library and contains no code from the system the paper studies.

## What it shows

A store returns a row's payload as a `Cow<[u8]>`. It is borrowed when the row is resident in memory, and an owned copy when the row was released under memory pressure and has to be reloaded. Each replica sizes its cache from its own memory, so residency is local: it is not part of the replicated log.

Each read charges a fixed overhead (1,000 retained units) and its payload (one unit per byte). The test payload is 264 bytes. Two meters are built in:

| Meter | Resident row | Released row | Replicas split at |
| --- | ---: | ---: | --- |
| `OwnedCopyOnly`: charges retained bytes only for an owned copy, after it exists | 1,000 | 1,264 | every budget from 1,000 to 1,263 |
| `LogicalExtent`: charges the payload's logical extent before acquisition | 1,264 | 1,264 | none |

Two replicas apply the same committed log: write the row, write a second row, read the first row under a budget. One replica has room for both rows. The other has room for one, so writing the second releases the first. Under the owned-copy meter, a budget between the two charges admits the read on one replica and refuses it on the other. The refusal is recorded as the read's outcome, so the replicas' replicated state digests differ. Under the logical-extent meter, both replicas reach the same decision at every budget.

The numbers are synthetic. Any fixed overhead and any payload length give the same shape: the split covers exactly the budgets from the resident charge up to one unit below the released charge.

## Run it

```bash
cargo test
```

All tests pass. The default build uses the corrected meter.

```bash
cargo test --features owned-copy-meter
```

The feature makes the defective meter the default read path. The two regression tests fail and print where the replicas split:

```
replicas split at 264 budgets, from 1000 to 1263
Resident: admit at the logical cost, refuse one unit below at the payload event
  left: (1000, Refused(Refusal { event: RowOverhead, limit: 999, spent: 0 }))
 right: (1264, Refused(Refusal { event: Payload, limit: 1263, spent: 1000 }))
```

```bash
cargo run --example split
```

The example prints both meters' decisions side by side.

No network access is needed: the crate has no dependencies. Continuous integration runs the formatting check, Clippy, the tests, the failing feature build and the example with Rust 1.96.1 and the current stable release, on Linux and macOS (`.github/workflows/ci.yml`).

## Tests

`tests/residency_split.rs`:

- `replicas_under_different_memory_pressure_agree_at_every_budget`: the regression. Two replicas with different cache sizes must end with equal replicated-state digests at every budget from 0 to 2,528. Fails under the feature.
- `one_read_has_one_boundary_in_every_residency_state`: the regression on one replica. In the resident, released and hydrated states, the read must admit at 1,264 and refuse at 1,263 with the same refusal. Fails under the feature.
- `owned_copy_meter_splits_replicas_between_the_two_charges`: the defect, shown explicitly in every build. The split is exactly 1,000 to 1,263.
- `logical_extent_meter_never_splits`: the correction, shown explicitly in every build.

Unit tests in `src/` check the premise (borrowed when resident, owned when released, released under memory pressure), the budget's checked addition and the digest.

## Layout

| File | Role |
| --- | --- |
| `src/store.rs` | Durable rows (replicated) and a bounded cache (local) |
| `src/budget.rs` | A retained-unit budget that refuses before an event, with checked addition |
| `src/read.rs` | The metered read under both pricings |
| `src/replica.rs` | Applies committed commands, records outcomes, digests replicated state only |
| `src/digest.rs` | A stable FNV-1a digest; `std`'s default hasher is seeded per process |

## What it does not show

The crate models one dimension of one meter. The studied system meters four dimensions, rechecks admission at apply time, and pins its prices with known-answer tests on several targets; none of that is reproduced here. It shows the mechanism, not the system's numbers.

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
