//! The core's 128-bit hash is xxh3-128 (PRD 5.3), pinned to values from the
//! reference C implementation, the first of them its published value for
//! empty input.

use reify_eval_core::Hash128;

#[test]
fn hash128_of_bytes_is_xxh3_128() {
    assert_eq!(
        Hash128::of_bytes(b""),
        Hash128::from_u128(0x99aa_06d3_0147_98d8_6001_c324_468d_497f)
    );
    assert_eq!(
        Hash128::of_bytes(b"reify").as_u128(),
        0x0c32_cfa8_d7d6_cffb_0a60_0f44_ce8b_a0cf
    );
}
