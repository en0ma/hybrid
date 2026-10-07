#![no_main]

use hybrid_engine::ActiveFill;
use hybrid_settlement::{settle_buy_active_fills, MakerBalance};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (u64, u64, u64, u64)| {
    let (locked_a, locked_b, quote_a, quote_b) = data;
    let fill_a = locked_a.min(u64::from(u32::MAX));
    let fill_b = locked_b.min(u64::from(u32::MAX));
    if fill_a == 0 || fill_b == 0 || quote_a == 0 || quote_b == 0 {
        return;
    }

    let fills = [
        ActiveFill {
            ask_index: 0,
            base_qty: fill_a,
            quote_qty: quote_a,
        },
        ActiveFill {
            ask_index: 1,
            base_qty: fill_b,
            quote_qty: quote_b,
        },
    ];
    let owners = [0u16, 1u16];
    let mut balances = [
        MakerBalance {
            base_free: 0,
            base_locked: fill_a,
            quote_free: 0,
            quote_locked: 0,
        },
        MakerBalance {
            base_free: 0,
            base_locked: fill_b,
            quote_free: 0,
            quote_locked: 0,
        },
    ];

    let result = settle_buy_active_fills(&fills, &owners, &mut balances);
    if let Ok(totals) = result {
        assert_eq!(balances[0].base_locked, 0);
        assert_eq!(balances[1].base_locked, 0);
        assert_eq!(balances[0].quote_free, quote_a);
        assert_eq!(balances[1].quote_free, quote_b);
        assert_eq!(totals.base_to_taker, fill_a.checked_add(fill_b).unwrap());
        assert_eq!(totals.quote_from_taker, quote_a.checked_add(quote_b).unwrap());
    }
});
