#![no_main]

use hybrid_engine::ActiveFill;
use hybrid_settlement::{settle_buy_active_fills, MakerBalance, SettlementError};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: [u64; 8]| {
    let [locked_a, locked_b, fill_a, fill_b, quote_a, quote_b, free_quote_a, free_quote_b] = data;
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
    let original = [
        MakerBalance {
            base_free: 0,
            base_locked: locked_a,
            quote_free: free_quote_a,
            quote_locked: 0,
        },
        MakerBalance {
            base_free: 0,
            base_locked: locked_b,
            quote_free: free_quote_b,
            quote_locked: 0,
        },
    ];
    let mut balances = original;

    let expected = if locked_a < fill_a {
        Err(SettlementError::InsufficientBase)
    } else if free_quote_a.checked_add(quote_a).is_none() {
        Err(SettlementError::Overflow)
    } else if locked_b < fill_b {
        Err(SettlementError::InsufficientBase)
    } else if free_quote_b.checked_add(quote_b).is_none()
        || fill_a.checked_add(fill_b).is_none()
        || quote_a.checked_add(quote_b).is_none()
    {
        Err(SettlementError::Overflow)
    } else {
        Ok(())
    };

    let result = settle_buy_active_fills(&fills, &owners, &mut balances);
    match (expected, result) {
        (Ok(()), Ok(totals)) => {
            assert_eq!(balances[0].base_locked, locked_a - fill_a);
            assert_eq!(balances[1].base_locked, locked_b - fill_b);
            assert_eq!(balances[0].quote_free, free_quote_a + quote_a);
            assert_eq!(balances[1].quote_free, free_quote_b + quote_b);
            assert_eq!(totals.base_to_taker, fill_a + fill_b);
            assert_eq!(totals.quote_from_taker, quote_a + quote_b);
        }
        (Err(expected_error), Err(actual_error)) => {
            assert_eq!(actual_error, expected_error);
            assert_eq!(balances, original);
        }
        (expected, actual) => {
            panic!("settlement result mismatch: expected {expected:?}, got {actual:?}");
        }
    }
});
