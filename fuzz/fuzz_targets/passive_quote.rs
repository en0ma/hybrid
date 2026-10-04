#![no_main]

use hybrid_engine::{
    quote_buy_exact_in, quote_quote_in_for_base_out, spot_price_x64, HybridMarket, LimitAsk,
    PassiveState,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (u64, u64, u64, u64)| {
    let (sqrt_hi, liquidity_hi, amount_in, ask_qty) = data;
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

    if let Ok(price_x64) = spot_price_x64(sqrt) {
        let market = HybridMarket {
            passive: state,
            best_ask: Some(LimitAsk {
                price_x64,
                sqrt_price_x64: sqrt,
                base_qty: ask_qty,
            }),
        };
        if let Ok(q) = quote_buy_exact_in(market, amount_in) {
            assert!(q.amount_in <= amount_in);
            assert_eq!(
                q.amount_out,
                q.active_base_out.saturating_add(q.passive_base_out)
            );
        }
    }
});
