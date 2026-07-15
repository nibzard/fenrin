const ONSETS: [char; 8] = ['b', 'd', 'f', 'g', 'k', 'm', 'n', 'r'];
const VOWELS: [char; 4] = ['a', 'e', 'o', 'u'];
const MEDIALS: [char; 8] = ['d', 'f', 'g', 'k', 'l', 'm', 'n', 's'];
const CODAS: [char; 8] = ['f', 'k', 'm', 'n', 'p', 'r', 's', 'z'];

pub(crate) const VERSION: &str = "fenrin-sas-v1";
pub(crate) const SAS_BITS: usize = 40;
pub(crate) const SAS_BYTES: usize = SAS_BITS / 8;

pub(crate) fn encode(bytes: [u8; SAS_BYTES]) -> String {
    let value = u64::from_be_bytes([0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]]);
    let mut phrase = String::with_capacity(23);

    for (position, shift) in [30, 20, 10, 0].into_iter().enumerate() {
        if position > 0 {
            phrase.push(' ');
        }
        encode_word(((value >> shift) & 0x03ff) as u16, &mut phrase);
    }

    phrase
}

fn encode_word(index: u16, output: &mut String) {
    debug_assert!(index < 1024);
    let onset = ((index >> 7) & 0x07) as usize;
    let first_vowel = ((index >> 5) & 0x03) as usize;
    let medial = ((index >> 2) & 0x07) as usize;
    let second_vowel = (index & 0x03) as usize;
    let parity = (onset + first_vowel + 3 * medial + 5 * second_vowel) % CODAS.len();

    output.push(ONSETS[onset]);
    output.push(VOWELS[first_vowel]);
    output.push(MEDIALS[medial]);
    output.push(VOWELS[second_vowel]);
    output.push(CODAS[parity]);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn zero_has_a_stable_encoding() {
        assert_eq!(encode([0; SAS_BYTES]), "badaf badaf badaf badaf");
    }

    #[test]
    fn golden_vectors_freeze_version_one() {
        assert_eq!(VERSION, "fenrin-sas-v1");
        assert_eq!(encode([0xff; SAS_BYTES]), "rusus rusus rusus rusus");
        assert_eq!(encode([0, 0, 0, 0, 1]), "badaf badaf badaf bader");
        assert_eq!(
            encode([0x00, 0x40, 0x10, 0x04, 0x01]),
            "bader bader bader bader"
        );
    }

    #[test]
    fn every_ten_bit_word_is_unique_and_printable() {
        let mut words = HashSet::new();

        for index in 0..1024 {
            let mut word = String::new();
            encode_word(index, &mut word);
            assert_eq!(word.len(), 5);
            assert!(word.bytes().all(|byte| byte.is_ascii_lowercase()));
            assert!(words.insert(word));
        }

        assert_eq!(words.len(), 1024);
    }

    #[test]
    fn changing_one_core_symbol_also_changes_its_parity_coda() {
        for index in 0_u16..1024 {
            let digits = [
                (index >> 7) & 0x07,
                (index >> 5) & 0x03,
                (index >> 2) & 0x07,
                index & 0x03,
            ];
            let radices = [8_u16, 4, 8, 4];
            let original_parity = (digits[0] + digits[1] + 3 * digits[2] + 5 * digits[3]) % 8;

            for position in 0..digits.len() {
                for replacement in 0..radices[position] {
                    if replacement == digits[position] {
                        continue;
                    }
                    let mut changed = digits;
                    changed[position] = replacement;
                    let changed_parity =
                        (changed[0] + changed[1] + 3 * changed[2] + 5 * changed[3]) % 8;
                    assert_ne!(changed_parity, original_parity);
                }
            }
        }
    }

    #[test]
    fn all_forty_input_bits_affect_the_phrase() {
        let baseline = encode([0; SAS_BYTES]);

        for bit in 0..SAS_BITS {
            let mut changed = [0_u8; SAS_BYTES];
            changed[bit / 8] = 1 << (7 - bit % 8);
            assert_ne!(encode(changed), baseline, "bit {bit} had no effect");
        }
    }
}
