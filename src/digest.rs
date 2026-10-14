// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 Prisma Risk

//! A stable 64-bit FNV-1a digest. `std`'s default hasher is seeded per process, so it cannot compare state across replicas.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over `bytes`.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    extend(OFFSET, bytes)
}

/// Continues an FNV-1a digest with `bytes`.
pub(crate) fn extend(mut state: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(PRIME);
    }
    state
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_the_published_fnv1a_vector() {
        assert_eq!(super::fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
