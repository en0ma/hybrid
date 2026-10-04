#![no_main]

use hybrid_engine::{spot_price_x64, Q64};
use hybrid_state::{
    validate_ask_chain, validate_boundary_chain, AskEntry, AskPage, BoundaryEntry, BoundaryPage,
    PageLinks, ASK_PAGE_BYTES, BOUNDARY_PAGE_BYTES,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (u64, u64, u64, u64, u64, u64)| {
    let (a, b, c, d, e, f) = data;

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    let candidates = [
        (a, b, c),
        (d, e, f),
        (b, c, d),
        (e, f, a),
    ];
    for (seed, qty_seed, seq_seed) in candidates {
        let sqrt_price_x64 = Q64.saturating_add(u128::from(seed % 100_000));
        if let Ok(price_x64) = spot_price_x64(sqrt_price_x64) {
            let entry = AskEntry {
                price_x64,
                sqrt_price_x64,
                base_qty: qty_seed.max(1),
                sequence: seq_seed.max(1),
            };
            let _ = asks.insert(entry);
            asks.validate_order().unwrap();
        }
    }

    if !asks.is_empty() {
        let first = asks.as_slice()[0];
        let _ = asks.set_quantity(first.sequence, b.max(1));
        asks.validate_order().unwrap();

        if c & 1 == 1 {
            let _ = asks.remove_by_sequence(first.sequence);
            asks.validate_order().unwrap();
        }
    }

    let mut ask_bytes = [0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_bytes).unwrap();
    let decoded_asks = AskPage::decode_from(&ask_bytes).unwrap();
    assert_eq!(decoded_asks, asks);

    let mut boundaries = BoundaryPage::default();
    boundaries.set_links(PageLinks::new(0, None, None));
    for (seed, liquidity_seed) in [(a, b), (c, d), (e, f)] {
        let entry = BoundaryEntry {
            sqrt_price_x64: Q64.saturating_add(1 + u128::from(seed % 100_000)),
            liquidity_after: u128::from(liquidity_seed).max(1),
        };
        let _ = boundaries.insert(entry);
        boundaries.validate_order().unwrap();
    }

    if !boundaries.is_empty() {
        let first = boundaries.as_slice()[0];
        let _ = boundaries.set_liquidity(first.sqrt_price_x64, u128::from(a).max(1));
        boundaries.validate_order().unwrap();

        if d & 1 == 1 {
            let _ = boundaries.remove(first.sqrt_price_x64);
            boundaries.validate_order().unwrap();
        }
    }

    let mut boundary_bytes = [0u8; BOUNDARY_PAGE_BYTES];
    boundaries.encode_into(&mut boundary_bytes).unwrap();
    let decoded_boundaries = BoundaryPage::decode_from(&boundary_bytes).unwrap();
    assert_eq!(decoded_boundaries, boundaries);

    assert!(validate_ask_chain(core::slice::from_ref(&asks)).is_ok());
    assert!(validate_boundary_chain(core::slice::from_ref(&boundaries)).is_ok());

    let mut bad_asks = asks;
    bad_asks.set_links(PageLinks::new(7, None, None));
    assert!(validate_ask_chain(core::slice::from_ref(&bad_asks)).is_err());

    let mut bad_boundaries = boundaries;
    bad_boundaries.set_links(PageLinks::new(9, None, None));
    assert!(validate_boundary_chain(core::slice::from_ref(&bad_boundaries)).is_err());
});
