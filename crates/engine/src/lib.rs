#![forbid(unsafe_code)]

pub const Q64: u128 = 1u128 << 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassiveState {
    pub sqrt_price_x64: u128,
    pub liquidity: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quote {
    pub amount_in: u64,
    pub amount_out: u64,
    pub next_sqrt_price_x64: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteError {
    ZeroLiquidity,
    Overflow,
    InvalidPrice,
}

pub fn quote_quote_in_for_base_out(
    state: PassiveState,
    quote_in: u64,
) -> Result<Quote, QuoteError> {
    if state.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if state.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }

    // CLMM segment math:
    // quote_in = L * (sqrt(P_next) - sqrt(P_now)).
    // V0 uses integer Q64 arithmetic and rounds against the taker.
    let delta_sqrt = (u128::from(quote_in))
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(state.liquidity)
        .ok_or(QuoteError::Overflow)?;
    let next = state
        .sqrt_price_x64
        .checked_add(delta_sqrt)
        .ok_or(QuoteError::Overflow)?;

    // base_out = L * (1/S_now - 1/S_next).
    // Reorder the rational expression to keep intermediate values inside u128:
    // L * dS * Q64 / (S0 * S1).
    let amount_out = state
        .liquidity
        .checked_mul(next - state.sqrt_price_x64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(state.sqrt_price_x64)
        .ok_or(QuoteError::Overflow)?
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(next)
        .ok_or(QuoteError::Overflow)?;
    let amount_out = u64::try_from(amount_out).map_err(|_| QuoteError::Overflow)?;

    Ok(Quote {
        amount_in: quote_in,
        amount_out,
        next_sqrt_price_x64: next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_moves_price_up_and_returns_base() {
        let state = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 1_000_000,
        };
        let q = quote_quote_in_for_base_out(state, 10_000).unwrap();
        assert_eq!(q.amount_in, 10_000);
        assert!(q.amount_out > 0);
        assert!(q.next_sqrt_price_x64 > state.sqrt_price_x64);
    }

    #[test]
    fn zero_liquidity_rejected() {
        let state = PassiveState { sqrt_price_x64: Q64, liquidity: 0 };
        assert_eq!(
            quote_quote_in_for_base_out(state, 1),
            Err(QuoteError::ZeroLiquidity)
        );
    }
}
