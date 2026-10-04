#![no_main]

use hybrid_engine::{quote_quote_in_for_base_out, PassiveState};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (u64, u64, u64)| {
    let (sqrt_hi, liquidity_hi, amount_in) = data;
    let sqrt = (u128::from(sqrt_hi).max(1)) << 32;
    let liquidity = (u128::from(liquidity_hi).max(1)) << 64;
    let state = PassiveState {
        sqrt_price_x64: sqrt,
        liquidity,
    };
    if let Ok(q) = quote_quote_in_for_base_out(state, amount_in) {
        assert!(q.next_sqrt_price_x64 >= state.sqrt_price_x64);
        assert_eq!(q.amount_in, amount_in);
    }
});
