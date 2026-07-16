# SAS protocol integration

Fenrin's stable `fenrin-sas-v1` format reads five bytes in big-endian order,
splits them into four 10-bit values, and maps each value to one algorithmic
CVCVC word. The final consonant is parity for easier comparison; it adds no
entropy. Name profiles and configuration files never affect this mapping.

Any two codewords differ in at least two of their five letters: whenever a
single core symbol changes, the parity coda changes with it. A single misread
letter therefore never turns one valid codeword into another. The test suite
proves this bound over all codeword pairs.

Paired applications should derive the five uniform bytes with a
protocol-specific KDF over their shared key-exchange secret and a canonical
transcript. The transcript should bind identities, roles, session ID, protocol
version, and ephemeral public keys. Compare all four words in order.

The active-forgery bound is approximately `q / 2^40` for `q` allowed attempts,
so the surrounding protocol must commit before revealing the phrase and limit
retries. A phrase entered on another device needs a one-shot, rate-limited PAKE
rather than direct use as key material.

Fenrin only renders the short authentication string. It does not perform the
key exchange or authenticate either application. Do not use the phrase as a
password, recovery seed, encryption key, or bearer token.
