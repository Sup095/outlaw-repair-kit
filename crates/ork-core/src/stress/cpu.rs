//! Working every core hard, and checking that it got the right answer.
//!
//! The point is not the load. Anything can load a processor -- a `while true`
//! loop will do it. The point is that the work has a *known correct result*,
//! so that a core which quietly returns the wrong number is caught, and caught
//! with a core number attached.
//!
//! That is the failure this exists to find. A processor that is degrading, or
//! one pushed past what its voltage can sustain, does not usually stop; it
//! computes. It just computes wrongly, occasionally, and the machine that
//! results is one where builds fail at random places, archives fail their
//! checksums, and nothing ever reproduces. Every one of those looks like a
//! software problem right up until somebody runs something like this.
//!
//! **Where the correct answer comes from.** It is computed at the start of the
//! run, on the same core that will then be checked against it. That is
//! deliberate, and it is not circular:
//!
//! * A hardcoded constant would be wrong. Floating point is deterministic for
//!   a given sequence of instructions, but the sequence is chosen by the
//!   compiler and differs between processors, architectures, and releases of
//!   this tool. A constant would fail everywhere it had not been generated.
//! * We are not asking whether this processor agrees with some reference
//!   machine. We are asking whether it agrees *with itself* -- whether the
//!   same instructions on the same data give the same answer this second as
//!   last second. A processor that does not is broken, and no reference is
//!   needed to say so.
//! * A core faulty enough to compute the reference wrongly will then disagree
//!   with almost every block that follows, because the fault will not repeat
//!   identically. It is caught either way.

use std::hint::black_box;

/// How much work is in one block.
///
/// Sized so a block takes a few milliseconds on a current processor. Small
/// enough that cancelling is immediate -- the run only checks whether it has
/// been stopped between blocks, and nobody should have to wait to stop
/// something that is heating their computer.
pub const BLOCK_ITERATIONS: u64 = 1 << 16;

/// The mixing constant from xorshift64*. Nothing sacred about it; it is a
/// large odd number that mixes well.
const MULTIPLIER: u64 = 0x2545_F491_4F6C_DD1D;

/// One block of verifiable work.
///
/// Deliberately mixed: an integer path that keeps the shifters and the
/// multiplier busy, a floating-point path built on fused multiply-add, and a
/// dependency between the two so neither can be dropped or reordered away.
/// The chain is serial on purpose -- each step needs the previous one -- which
/// is what makes a single wrong bit anywhere show up in the result rather than
/// being averaged out.
pub fn block(seed: u64) -> u64 {
    // Through `black_box` so that a constant seed at a call site cannot let
    // the whole loop be folded away at compile time.
    //
    // Zero is the one state an xorshift cannot leave, so it is the one state
    // that has to be substituted -- and only that one. Forcing the low bit
    // instead, which is the shorter way to write this, would mean the lowest
    // bit of the seed never reached the arithmetic, and a check whose whole
    // job is to notice one wrong bit would have been ignoring one.
    let mut state = black_box(seed);
    if state == 0 {
        state = 1;
    }
    let mut float = 1.000_000_1_f64;
    let mut accumulator = 0u64;

    for step in 0..BLOCK_ITERATIONS {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let mixed = state.wrapping_mul(MULTIPLIER);

        // Fused multiply-add is one instruction and one rounding, which makes
        // it repeatable; the addend is taken from the integer path so the two
        // cannot be computed independently.
        float = float.mul_add(1.000_000_000_1, (mixed >> 40) as f64 * 1e-12);
        // Kept in a sane range without a branch on the value's exact bits, so
        // the sequence stays identical run to run.
        if !float.is_finite() || float > 1e9 {
            float = 1.000_000_1;
        }

        accumulator = accumulator
            .rotate_left(7)
            .wrapping_add(mixed ^ float.to_bits())
            ^ step;
    }

    accumulator
}

/// What one core did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreWork {
    pub core: usize,
    pub blocks: u64,
    /// Blocks that came back with the wrong answer. Any number above zero is
    /// a hardware fault.
    pub wrong: u64,
}

/// The seed for a given core.
///
/// Different per core so the cores are not all grinding through byte-identical
/// data, which would leave whole regions of the arithmetic untouched on every
/// one of them.
pub fn seed_for(core: usize) -> u64 {
    0x0BAD_C0DE_DEAD_BEEF ^ ((core as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_answer() {
        // If this ever fails on a real machine, that machine has the fault
        // this module exists to find.
        let once = block(seed_for(0));
        let again = block(seed_for(0));
        assert_eq!(once, again);
    }

    #[test]
    fn different_cores_do_different_work() {
        // Otherwise every core grinds through byte-identical data and a fault
        // that only shows up on one bit pattern is invisible on all but one.
        let a = block(seed_for(0));
        let b = block(seed_for(1));
        let c = block(seed_for(7));
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn the_work_is_not_optimised_away() {
        // A block that compiled down to a constant would load nothing and
        // detect nothing, and would look exactly like a passing test. Two
        // different seeds producing two different non-trivial answers is the
        // cheapest evidence that the loop actually ran.
        let answer = block(black_box(12_345));
        assert_ne!(answer, 0);
        assert_ne!(answer, block(black_box(12_346)));
    }

    #[test]
    fn one_wrong_bit_anywhere_changes_the_answer() {
        // The property the whole check rests on: a fault does not have to be
        // large to be caught. Flipping the lowest bit of the seed is the
        // smallest possible difference in the input.
        let clean = block(seed_for(3));
        let nudged = block(seed_for(3) ^ 1);
        assert_ne!(clean, nudged);
    }

    #[test]
    fn every_core_gets_its_own_seed() {
        let seeds: std::collections::HashSet<u64> = (0..256).map(seed_for).collect();
        assert_eq!(seeds.len(), 256, "two cores would have done identical work");
    }
}
