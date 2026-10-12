//! Passive exact-out plan construction (pure math only).
//! Execution must enforce authenticated custody, max gross input, all crossed
//! boundaries and actual output floors before transferring any SPL tokens.
use crate::{quote_base_in_for_quote_out, quote_quote_in_for_base_out, PassiveState, Quote, QuoteError};

/// Smallest net quote input needed for at least `base_out` atoms.
pub fn buy_exact_out(state: PassiveState, base_out: u64) -> Result<Quote, QuoteError> {
    if base_out == 0 {
        return Ok(Quote {
            amount_in: 0,
            amount_out: 0,
            next_sqrt_price_x64: state.sqrt_price_x64,
        });
    }
    let max = quote_quote_in_for_base_out(state, u64::MAX)?;
    if max.amount_out < base_out {
        return Err(QuoteError::Overflow);
    }
    let mut low = 1u64;
    let mut high = u64::MAX;
    while low < high {
        let mid = low + (high - low) / 2;
        let quote = quote_quote_in_for_base_out(state, mid)?;
        if quote.amount_out >= base_out {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    quote_quote_in_for_base_out(state, low)
}

/// Smallest net base input needed for at least `quote_out` atoms.
pub fn sell_exact_out(state: PassiveState, quote_out: u64) -> Result<Quote, QuoteError> {
    if quote_out == 0 {
        return Ok(Quote {
            amount_in: 0,
            amount_out: 0,
            next_sqrt_price_x64: state.sqrt_price_x64,
        });
    }
    let max = quote_base_in_for_quote_out(state, u64::MAX)?;
    if max.amount_out < quote_out {
        return Err(QuoteError::Overflow);
    }
    let mut low = 1u64;
    let mut high = u64::MAX;
    while low < high {
        let mid = low + (high - low) / 2;
        let quote = quote_base_in_for_quote_out(state, mid)?;
        if quote.amount_out >= quote_out {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    quote_base_in_for_quote_out(state, low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Q64;

    #[test]
    fn exact_out_quotes_are_minimal_for_both_directions() {
        let state = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 1_000_000,
        };
        let buy = buy_exact_out(state, 100).unwrap();
        assert!(buy.amount_out >= 100);
        assert!(buy.amount_in > 0);
        assert!(quote_quote_in_for_base_out(state, buy.amount_in - 1)
            .unwrap()
            .amount_out < 100);
        let sell = sell_exact_out(state, 100).unwrap();
        assert!(sell.amount_out >= 100);
        assert!(sell.amount_in > 0);
        assert!(quote_base_in_for_quote_out(state, sell.amount_in - 1)
            .unwrap()
            .amount_out < 100);
    }
}
