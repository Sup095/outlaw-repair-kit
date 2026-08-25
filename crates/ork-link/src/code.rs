//! Pairing codes: short enough to read out loud, long enough to be safe.
//!
//! A pairing code carries a secret that two machines briefly share. It is
//! never used as a password and never sent over the wire -- see [`crate::pair`]
//! for what actually crosses the network. Its only job is to get the same
//! random number into two computers via a human.
//!
//! The alphabet is Crockford's base32: no `I`, `L`, `O`, or `U`, so there is
//! no way to confuse a one with an I or a zero with an O, and no accidental
//! rude words. Input is case-insensitive and forgiving about the grouping
//! dashes, because people type these by hand.

use rand::RngCore;

/// Crockford base32, in order.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many characters a code has, not counting the dashes.
///
/// Twelve characters of base32 is 60 bits. Guessing one inside its short
/// lifetime, against a host that stops listening after a handful of wrong
/// answers, is not a realistic attack.
pub const CODE_LEN: usize = 12;

/// The shared secret a pairing code carries.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// The raw characters, without dashes and already upper-cased.
    text: String,
}

impl std::fmt::Debug for PairingCode {
    /// Deliberately does not print the code. A pairing secret in a log file is
    /// a pairing secret in a log file.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PairingCode(hidden)")
    }
}

impl PairingCode {
    /// A fresh code from the operating system's random source.
    pub fn generate() -> Self {
        let mut bytes = [0u8; CODE_LEN];
        rand::rng().fill_bytes(&mut bytes);
        // Modulo bias is irrelevant here: 256 is an exact multiple of 32.
        let text = bytes.iter().map(|byte| ALPHABET[(byte % 32) as usize] as char).collect();
        Self { text }
    }

    /// Read a code somebody typed.
    ///
    /// Accepts any grouping, any case, and the letters people substitute by
    /// habit: `I` and `L` for one, `O` for zero.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let mut text = String::with_capacity(CODE_LEN);
        for character in input.chars() {
            if character == '-' || character == ' ' || character == '_' {
                continue;
            }
            let upper = character.to_ascii_uppercase();
            let normalised = match upper {
                'I' | 'L' => '1',
                'O' => '0',
                other => other,
            };
            anyhow::ensure!(
                ALPHABET.contains(&(normalised as u8)),
                "`{character}` is not part of a pairing code"
            );
            text.push(normalised);
        }

        anyhow::ensure!(
            text.len() == CODE_LEN,
            "a pairing code has {CODE_LEN} characters, but that one has {}",
            text.len()
        );
        Ok(Self { text })
    }

    /// The secret bytes, for deriving keys from.
    pub fn secret(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// How the code is shown to a person: `XXXX-XXXX-XXXX`.
    pub fn display(&self) -> String {
        self.text
            .as_bytes()
            .chunks(4)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_code_reads_back_as_itself() {
        for _ in 0..64 {
            let code = PairingCode::generate();
            assert_eq!(PairingCode::parse(&code.display()).unwrap(), code);
        }
    }

    #[test]
    fn codes_are_shown_in_groups_of_four() {
        let code = PairingCode::generate();
        let shown = code.display();
        assert_eq!(shown.len(), CODE_LEN + 2);
        assert_eq!(shown.matches('-').count(), 2);
    }

    #[test]
    fn typing_it_wrong_in_the_usual_ways_still_works() {
        let short = PairingCode::parse("K7M4-9QXP-2N").unwrap_err();
        assert!(short.to_string().contains("12 characters"), "{short}");

        let canonical = PairingCode::parse("K7M4-9QXP-2N3W").unwrap();
        for variant in ["k7m4 9qxp 2n3w", "K7M49QXP2N3W", "k7m4_9qxp_2n3w"] {
            assert_eq!(PairingCode::parse(variant).unwrap(), canonical, "{variant}");
        }
    }

    #[test]
    fn letters_people_mistake_for_digits_are_accepted_as_those_digits() {
        // Crockford's whole point: O is zero, I and L are one.
        assert_eq!(
            PairingCode::parse("O123456789AB").unwrap(),
            PairingCode::parse("0123456789AB").unwrap()
        );
        assert_eq!(
            PairingCode::parse("I123456789AB").unwrap(),
            PairingCode::parse("L123456789AB").unwrap()
        );
    }

    #[test]
    fn a_code_never_prints_itself_by_accident() {
        let code = PairingCode::generate();
        let debugged = format!("{code:?}");
        assert!(!debugged.contains(&code.display().replace('-', "")), "a debug print leaked the code");
    }

    #[test]
    fn nonsense_is_refused_rather_than_silently_trimmed() {
        assert!(PairingCode::parse("").is_err());
        assert!(PairingCode::parse("!!!!-!!!!-!!!!").is_err());
        assert!(PairingCode::parse("K7M4-9QXP-2N3W-EXTRA").is_err());
    }

    #[test]
    fn generated_codes_do_not_repeat() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            assert!(seen.insert(PairingCode::generate().display()), "a code repeated");
        }
    }
}
