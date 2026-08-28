//! Writing patterns into memory and reading them back.
//!
//! Bad memory is the single most misdiagnosed hardware fault there is. It does
//! not announce itself. It corrupts one bit, somewhere, occasionally, and the
//! machine goes on running -- so the user gets a browser that crashes on a
//! different tab each time, a game that fails to launch once a week, a
//! photograph that opens with a band of noise through it, and an operating
//! system that gets reinstalled for nothing.
//!
//! The test is old and simple: write a known value into every cell, read every
//! cell back, and see whether it is still there. What makes it worth doing
//! carefully is the choice of patterns, because different patterns catch
//! different physical faults, and a test that only writes zeroes catches
//! almost nothing.
//!
//! **What this cannot do.** It tests the memory that the operating system is
//! willing to hand this process, which is not all of it -- the kernel's own
//! memory, and anything already in use, is untouchable from here. A clean
//! result from this narrows the problem; it does not clear the memory. That
//! sentence is in the report as well as in this comment, because somebody
//! deciding whether to replace a memory module deserves to know which of the
//! two they have been told.

use serde::{Deserialize, Serialize};

/// How much memory to work on at once.
///
/// Small enough that the run notices a cancellation promptly and that a
/// machine short on memory can still allocate *something*; large enough that
/// the cost of the bookkeeping disappears against the cost of the work.
pub const CHUNK_BYTES: u64 = 64 * 1024 * 1024;

/// Memory never taken, however much is free.
///
/// Whatever the user asked for, taking the machine down to nothing means the
/// operating system starts paging -- at which point this is a slow, damaging
/// test of the disk rather than a test of the memory, and the machine it is
/// meant to be diagnosing becomes unusable while it runs. On Linux it also
/// invites the kernel to shoot a process, and there is no guarantee it picks
/// this one.
pub const RESERVED_BYTES: u64 = 1024 * 1024 * 1024;

/// The share of free memory to take when nobody says otherwise.
pub const DEFAULT_SHARE: f64 = 0.6;

/// Below this there is not enough to be worth the disruption.
pub const MINIMUM_USEFUL_BYTES: u64 = 256 * 1024 * 1024;

/// What to write into memory.
///
/// Each of these catches a different physical fault, which is why the test
/// cycles through all of them rather than picking one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pattern {
    /// Every bit low. Catches cells stuck high.
    Zeros,
    /// Every bit high. Catches cells stuck low, and is the pattern that draws
    /// the most current -- so it is also the one most likely to expose a power
    /// delivery problem rather than a storage one.
    Ones,
    /// Alternating bits, inverted between neighbouring cells. Catches cells
    /// that are disturbed by what is written next to them, which is the
    /// dominant failure mode in dense modern memory.
    Checkerboard,
    /// Each cell holds its own address. Catches faults in the *addressing*
    /// rather than the storage: two addresses that land on one physical cell
    /// look perfect to every other pattern, because both reads return
    /// something that was legitimately written.
    OwnAddress,
    /// A pseudorandom walk. Catches what a regular pattern cannot, because a
    /// fault that happens to agree with the pattern being written is invisible
    /// to that pattern.
    Noise,
}

impl Pattern {
    /// Every pattern, in the order a pass runs them.
    pub const ALL: [Pattern; 5] = [
        Pattern::Zeros,
        Pattern::Ones,
        Pattern::Checkerboard,
        Pattern::OwnAddress,
        Pattern::Noise,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Pattern::Zeros => "zeros",
            Pattern::Ones => "ones",
            Pattern::Checkerboard => "checkerboard",
            Pattern::OwnAddress => "own-address",
            Pattern::Noise => "noise",
        }
    }

    /// What cell `index` should contain under this pattern.
    ///
    /// `index` is the cell's position across the whole test, not within a
    /// chunk, so that `OwnAddress` means something.
    pub fn cell(self, index: u64) -> u64 {
        match self {
            Pattern::Zeros => 0,
            Pattern::Ones => u64::MAX,
            Pattern::Checkerboard => {
                if index % 2 == 0 {
                    0x5555_5555_5555_5555
                } else {
                    0xAAAA_AAAA_AAAA_AAAA
                }
            }
            Pattern::OwnAddress => index,
            Pattern::Noise => {
                // A cheap reversible mix, so the expected value for any cell
                // can be recomputed without keeping a copy of the data --
                // which matters, because a second copy would halve how much
                // memory the test can cover.
                let mut value = index.wrapping_add(0x9E37_79B9_7F4A_7C15);
                value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                value ^ (value >> 31)
            }
        }
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How much memory to test, or why not to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Budget {
    Test {
        bytes: u64,
    },
    /// Not enough spare memory to test without pushing the machine into swap.
    /// A refusal, said out loud -- not a silent skip and not a smaller test
    /// pretending to be a real one.
    NotEnoughSpare {
        available: u64,
        reserved: u64,
    },
}

/// Decide how much memory to take.
///
/// Kept as a pure function of three numbers so the rule that protects the
/// machine can be tested against the awkward cases -- a laptop with 700 MB
/// free, a share of 1.0, a machine already swapping -- rather than only
/// against whatever this developer's computer happened to have free.
pub fn budget(available_bytes: u64, share: f64) -> Budget {
    let share = share.clamp(0.05, 0.95);
    let spare = available_bytes.saturating_sub(RESERVED_BYTES);
    let wanted = (available_bytes as f64 * share) as u64;
    let bytes = wanted.min(spare);

    if bytes < MINIMUM_USEFUL_BYTES {
        return Budget::NotEnoughSpare {
            available: available_bytes,
            reserved: RESERVED_BYTES,
        };
    }
    // Whole chunks, so the last one is not a stub.
    let bytes = (bytes / CHUNK_BYTES) * CHUNK_BYTES;
    if bytes < MINIMUM_USEFUL_BYTES {
        return Budget::NotEnoughSpare {
            available: available_bytes,
            reserved: RESERVED_BYTES,
        };
    }
    Budget::Test { bytes }
}

/// A cell that did not hold what was put in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub pattern: Pattern,
    /// Byte offset within the region under test. Not a physical address -- the
    /// operating system does not tell us that -- and the report says so.
    pub offset_bytes: u64,
    pub expected: u64,
    pub found: u64,
}

impl Mismatch {
    /// The bits that changed. Usually one, which is the signature of a memory
    /// fault rather than of something having overwritten the buffer.
    pub fn flipped_bits(&self) -> u32 {
        (self.expected ^ self.found).count_ones()
    }

    pub fn describe(&self) -> String {
        format!(
            "{} bytes into the tested region, under the {} pattern: expected {:#018x}, \
             found {:#018x} -- {} bit{} changed",
            self.offset_bytes,
            self.pattern,
            self.expected,
            self.found,
            self.flipped_bits(),
            if self.flipped_bits() == 1 { "" } else { "s" }
        )
    }
}

/// Fill `chunk` with `pattern`, where the chunk starts at cell `first_cell`.
pub fn write(chunk: &mut [u64], pattern: Pattern, first_cell: u64) {
    for (offset, cell) in chunk.iter_mut().enumerate() {
        *cell = pattern.cell(first_cell + offset as u64);
    }
}

/// Read `chunk` back, returning the first cell that is wrong.
///
/// First rather than all of them: one is enough to condemn the memory, and a
/// module that has genuinely failed can produce millions, which would fill the
/// report, the log, and eventually the disk.
pub fn verify(chunk: &[u64], pattern: Pattern, first_cell: u64) -> Option<Mismatch> {
    for (offset, cell) in chunk.iter().enumerate() {
        let index = first_cell + offset as u64;
        let expected = pattern.cell(index);
        if *cell != expected {
            return Some(Mismatch {
                pattern,
                offset_bytes: index * 8,
                expected,
                found: *cell,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn a_round_trip_through_memory_is_clean() {
        // The baseline the whole test rests on: on working memory, every
        // pattern comes back exactly as it went in.
        for pattern in Pattern::ALL {
            let mut chunk = vec![0u64; 4096];
            write(&mut chunk, pattern, 1_000);
            assert_eq!(verify(&chunk, pattern, 1_000), None, "{pattern} round trip");
        }
    }

    #[test]
    fn a_single_flipped_bit_is_caught() {
        // What a failing memory module actually does. If this test can be made
        // to pass with the check removed, the check is worthless -- so it
        // asserts on the specific cell, not merely on "something was wrong".
        for pattern in Pattern::ALL {
            let mut chunk = vec![0u64; 4096];
            write(&mut chunk, pattern, 0);
            chunk[2_000] ^= 1 << 17;
            let caught = verify(&chunk, pattern, 0).expect("{pattern}: flipped bit missed");
            assert_eq!(caught.offset_bytes, 2_000 * 8);
            assert_eq!(caught.flipped_bits(), 1);
            assert_eq!(caught.pattern, pattern);
        }
    }

    #[test]
    fn the_reported_offset_is_across_the_whole_region_not_the_chunk() {
        // Otherwise every chunk reports a fault "near the start", and the one
        // number that would let somebody correlate two runs is meaningless.
        let mut chunk = vec![0u64; 1024];
        write(&mut chunk, Pattern::Noise, 500_000);
        chunk[10] = 0;
        let caught = verify(&chunk, Pattern::Noise, 500_000).unwrap();
        assert_eq!(caught.offset_bytes, (500_000 + 10) * 8);
    }

    #[test]
    fn the_address_pattern_would_catch_two_addresses_sharing_one_cell() {
        // The fault it exists for: an address line stuck so that cell N and
        // cell N + 2^k are physically the same. Every other pattern reads back
        // something that was legitimately written and sees nothing wrong.
        let mut chunk = vec![0u64; 8192];
        write(&mut chunk, Pattern::OwnAddress, 0);
        // The aliased write lands in both places; the second read finds the
        // other address's value.
        chunk[1_000] = chunk[1_000 + 4_096];
        assert!(verify(&chunk, Pattern::OwnAddress, 0).is_some());

        // The same aliasing under a uniform pattern is genuinely invisible,
        // which is why the test does not rely on one.
        let mut uniform = vec![0u64; 8192];
        write(&mut uniform, Pattern::Ones, 0);
        uniform[1_000] = uniform[1_000 + 4_096];
        assert!(verify(&uniform, Pattern::Ones, 0).is_none());
    }

    #[test]
    fn neighbouring_cells_are_inverses_under_the_checkerboard() {
        // If they were not, the pattern would not be exercising the
        // disturbance between adjacent cells that it exists to exercise.
        for index in 0..64 {
            assert_eq!(
                Pattern::Checkerboard.cell(index) ^ Pattern::Checkerboard.cell(index + 1),
                u64::MAX
            );
        }
    }

    #[test]
    fn the_noise_pattern_is_recomputable_and_not_a_pattern() {
        // Recomputable, so no second copy of the data is needed; and spread
        // out, so it is genuinely a different test from the others.
        assert_eq!(Pattern::Noise.cell(99), Pattern::Noise.cell(99));
        let values: std::collections::HashSet<u64> =
            (0..1000).map(|index| Pattern::Noise.cell(index)).collect();
        assert_eq!(values.len(), 1000, "noise repeated itself");
    }

    #[test]
    fn a_machine_with_little_free_memory_is_left_alone() {
        // The case that matters most: taking 60% of what is free on a machine
        // with 900 MB free would put it into swap, and this tool exists to fix
        // machines, not to wedge them.
        assert!(matches!(
            budget(900 * 1024 * 1024, 0.6),
            Budget::NotEnoughSpare { .. }
        ));
        assert!(matches!(budget(0, 0.9), Budget::NotEnoughSpare { .. }));
    }

    #[test]
    fn a_gigabyte_is_always_left_for_the_machine_to_run_in() {
        // Even when asked for everything.
        let available = 8 * GB;
        let Budget::Test { bytes } = budget(available, 1.0) else {
            panic!("should have tested something");
        };
        assert!(
            bytes <= available - RESERVED_BYTES,
            "took {bytes} of {available}, leaving less than the reserve"
        );
    }

    #[test]
    fn the_share_is_respected_when_there_is_room_for_it() {
        let Budget::Test { bytes } = budget(32 * GB, 0.5) else {
            panic!("should have tested something");
        };
        // Rounded down to whole chunks, so within one chunk of half.
        assert!(bytes <= 16 * GB && bytes > 16 * GB - CHUNK_BYTES, "{bytes}");
    }

    #[test]
    fn the_region_is_a_whole_number_of_chunks() {
        for available in [4 * GB, 5 * GB + 12345, 64 * GB] {
            if let Budget::Test { bytes } = budget(available, 0.6) {
                assert_eq!(bytes % CHUNK_BYTES, 0, "{available}");
            }
        }
    }

    #[test]
    fn a_nonsense_share_cannot_take_the_whole_machine() {
        // Comes from a command line and from a text field, so it arrives
        // however somebody typed it.
        for share in [1.0, 5.0, f64::INFINITY, -1.0, f64::NAN] {
            match budget(16 * GB, share) {
                Budget::Test { bytes } => assert!(bytes <= 16 * GB - RESERVED_BYTES, "{share}"),
                Budget::NotEnoughSpare { .. } => {}
            }
        }
    }
}
