#![no_main]

use hybrid_engine::{
    quote_buy_exact_in, quote_buy_exact_in_levels, quote_quote_in_for_base_out, spot_price_x64,
    HybridMarket, LimitAsk, PassiveBoundary, PassiveState, Q64,
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

    let step = Q64 / 10_000;
    let ask0_sqrt = Q64;
    let ask1_sqrt = Q64 + step * 2;
    let asks = [
        LimitAsk {
            price_x64: Q64,
            sqrt_price_x64: ask0_sqrt,
            base_qty: ask_qty / 2,
        },
        LimitAsk {
            price_x64: spot_price_x64(ask1_sqrt).unwrap(),
            sqrt_price_x64: ask1_sqrt,
            base_qty: ask_qty.saturating_sub(ask_qty / 2),
        },
    ];
    let boundaries = [PassiveBoundary {
        sqrt_price_x64: Q64 + step,
        liquidity_after: u128::from(liquidity_hi).max(1) << 48,
    }];
    if let Ok(q) = quote_buy_exact_in_levels(
        PassiveState {
            sqrt_price_x64: Q64,
            liquidity: u128::from(liquidity_hi).max(1) << 48,
        },
        &asks,
        &boundaries,
        amount_in,
    ) {
        assert!(q.amount_in <= amount_in);
        assert_eq!(
            q.amount_out,
            q.active_base_out.saturating_add(q.passive_base_out)
        );
        assert!(q.next_sqrt_price_x64 >= Q64);
        assert!(q.fully_consumed_asks <= asks.len() as u32);
        assert!(q.crossed_boundaries <= boundaries.len() as u32);
    }
});
