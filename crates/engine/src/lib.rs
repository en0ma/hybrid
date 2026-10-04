#![forbid(unsafe_code)]

pub const Q64: u128 = 1u128 << 64;
const Q64_MASK: u128 = Q64 - 1;

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
pub struct LimitAsk {
    /// Quote atoms per base atom, Q64 fixed point.
    pub price_x64: u128,
    /// Cached sqrt(price) in Q64. Placement code must validate this cache.
    pub sqrt_price_x64: u128,
    pub base_qty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridMarket {
    pub passive: PassiveState,
    pub best_ask: Option<LimitAsk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridQuote {
    pub amount_in: u64,
    pub amount_out: u64,
    pub active_base_out: u64,
    pub passive_base_out: u64,
    pub next_sqrt_price_x64: u128,
    pub remaining_active_base: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteError {
    ZeroLiquidity,
    Overflow,
    InvalidPrice,
    InvalidCachedSqrtPrice,
}

/// floor(a * b / 2^64) without requiring a 256-bit intermediate.
fn mul_q64_floor(a: u128, b: u128) -> Result<u128, QuoteError> {
    let a_hi = a >> 64;
    let a_lo = a & Q64_MASK;
    let b_hi = b >> 64;
    let b_lo = b & Q64_MASK;

    let high = a_hi
        .checked_mul(b_hi)
        .and_then(|v| v.checked_mul(Q64))
        .ok_or(QuoteError::Overflow)?;
    let cross_a = a_hi.checked_mul(b_lo).ok_or(QuoteError::Overflow)?;
    let cross_b = b_hi.checked_mul(a_lo).ok_or(QuoteError::Overflow)?;
    let low = a_lo.checked_mul(b_lo).ok_or(QuoteError::Overflow)? >> 64;

    high
        .checked_add(cross_a)
        .and_then(|v| v.checked_add(cross_b))
        .and_then(|v| v.checked_add(low))
        .ok_or(QuoteError::Overflow)
}

fn mul_q64_ceil(a: u128, b: u128) -> Result<u128, QuoteError> {
    let floor = mul_q64_floor(a, b)?;
    let remainder = (a & Q64_MASK)
        .checked_mul(b & Q64_MASK)
        .ok_or(QuoteError::Overflow)?
        & Q64_MASK;
    if remainder == 0 {
        Ok(floor)
    } else {
        floor.checked_add(1).ok_or(QuoteError::Overflow)
    }
}

pub fn spot_price_x64(sqrt_price_x64: u128) -> Result<u128, QuoteError> {
    if sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    mul_q64_floor(sqrt_price_x64, sqrt_price_x64)
}

pub fn validate_limit_ask(order: LimitAsk) -> Result<(), QuoteError> {
    if order.price_x64 == 0 || order.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }

    // The cached square root may round down by one Q64 price atom.
    let cached = spot_price_x64(order.sqrt_price_x64)?;
    if cached > order.price_x64 || order.price_x64 - cached > 1 {
        return Err(QuoteError::InvalidCachedSqrtPrice);
    }
    Ok(())
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

    let delta_sqrt = (u128::from(quote_in))
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(state.liquidity)
        .ok_or(QuoteError::Overflow)?;
    let next = state
        .sqrt_price_x64
        .checked_add(delta_sqrt)
        .ok_or(QuoteError::Overflow)?;

    let amount_out = passive_base_delta(state.liquidity, state.sqrt_price_x64, next)?;
    let amount_out = u64::try_from(amount_out).map_err(|_| QuoteError::Overflow)?;

    Ok(Quote {
        amount_in: quote_in,
        amount_out,
        next_sqrt_price_x64: next,
    })
}

fn passive_base_delta(
    liquidity: u128,
    start_sqrt_x64: u128,
    end_sqrt_x64: u128,
) -> Result<u128, QuoteError> {
    if start_sqrt_x64 == 0 || end_sqrt_x64 < start_sqrt_x64 {
        return Err(QuoteError::InvalidPrice);
    }
    liquidity
        .checked_mul(end_sqrt_x64 - start_sqrt_x64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(start_sqrt_x64)
        .ok_or(QuoteError::Overflow)?
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?
        .checked_div(end_sqrt_x64)
        .ok_or(QuoteError::Overflow)
}

fn passive_to_target(
    state: PassiveState,
    target_sqrt_x64: u128,
) -> Result<Quote, QuoteError> {
    if state.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if target_sqrt_x64 < state.sqrt_price_x64 || state.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    if target_sqrt_x64 == state.sqrt_price_x64 {
        return Ok(Quote {
            amount_in: 0,
            amount_out: 0,
            next_sqrt_price_x64: target_sqrt_x64,
        });
    }

    let delta = target_sqrt_x64 - state.sqrt_price_x64;
    let quote_in = mul_q64_ceil(state.liquidity, delta)?;
    let quote_in = u64::try_from(quote_in).map_err(|_| QuoteError::Overflow)?;
    let base_out = passive_base_delta(
        state.liquidity,
        state.sqrt_price_x64,
        target_sqrt_x64,
    )?;
    let base_out = u64::try_from(base_out).map_err(|_| QuoteError::Overflow)?;

    Ok(Quote {
        amount_in: quote_in,
        amount_out: base_out,
        next_sqrt_price_x64: target_sqrt_x64,
    })
}

fn quote_for_base_at_price(base: u64, price_x64: u128) -> Result<u64, QuoteError> {
    let q = mul_q64_ceil(u128::from(base), price_x64)?;
    u64::try_from(q).map_err(|_| QuoteError::Overflow)
}

fn base_for_quote_at_price(quote: u64, price_x64: u128) -> Result<u64, QuoteError> {
    if price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    let numerator = u128::from(quote)
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?;
    let base = numerator / price_x64;
    u64::try_from(base).map_err(|_| QuoteError::Overflow)
}

/// V0.1 exact-in buy matcher.
///
/// The active ask wins ties. If passive liquidity is initially better, it is
/// consumed only until its marginal price reaches the explicit ask; then the
/// resting ask gets priority. Any remaining input returns to passive liquidity
/// after the active order is exhausted.
pub fn quote_buy_exact_in(
    market: HybridMarket,
    quote_in: u64,
) -> Result<HybridQuote, QuoteError> {
    if market.passive.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if market.passive.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }

    let Some(mut ask) = market.best_ask else {
        let passive = quote_quote_in_for_base_out(market.passive, quote_in)?;
        return Ok(HybridQuote {
            amount_in: quote_in,
            amount_out: passive.amount_out,
            active_base_out: 0,
            passive_base_out: passive.amount_out,
            next_sqrt_price_x64: passive.next_sqrt_price_x64,
            remaining_active_base: 0,
        });
    };
    validate_limit_ask(ask)?;

    let mut remaining_quote = quote_in;
    let mut passive_state = market.passive;
    let mut active_base_out = 0u64;
    let mut passive_base_out = 0u64;

    // Passive is strictly better below the ask. Move it up to the ask, but not
    // farther; exact equality belongs to the resting explicit order.
    if passive_state.sqrt_price_x64 < ask.sqrt_price_x64 && remaining_quote > 0 {
        let to_ask = passive_to_target(passive_state, ask.sqrt_price_x64)?;
        if to_ask.amount_in >= remaining_quote {
            let passive = quote_quote_in_for_base_out(passive_state, remaining_quote)?;
            return Ok(HybridQuote {
                amount_in: quote_in,
                amount_out: passive.amount_out,
                active_base_out: 0,
                passive_base_out: passive.amount_out,
                next_sqrt_price_x64: passive.next_sqrt_price_x64,
                remaining_active_base: ask.base_qty,
            });
        }

        remaining_quote -= to_ask.amount_in;
        passive_base_out = passive_base_out
            .checked_add(to_ask.amount_out)
            .ok_or(QuoteError::Overflow)?;
        passive_state.sqrt_price_x64 = ask.sqrt_price_x64;
    }

    // Active wins at equal or better price.
    if remaining_quote > 0 && ask.base_qty > 0 {
        let quote_for_all = quote_for_base_at_price(ask.base_qty, ask.price_x64)?;
        let base_fill = if remaining_quote >= quote_for_all {
            ask.base_qty
        } else {
            base_for_quote_at_price(remaining_quote, ask.price_x64)?.min(ask.base_qty)
        };

        if base_fill > 0 {
            let quote_used = quote_for_base_at_price(base_fill, ask.price_x64)?;
            remaining_quote = remaining_quote.saturating_sub(quote_used);
            ask.base_qty -= base_fill;
            active_base_out = active_base_out
                .checked_add(base_fill)
                .ok_or(QuoteError::Overflow)?;
        }
    }

    // Once the explicit ask is exhausted, remaining taker input continues
    // against passive liquidity from the current marginal price.
    if remaining_quote > 0 && ask.base_qty == 0 {
        let passive = quote_quote_in_for_base_out(passive_state, remaining_quote)?;
        passive_base_out = passive_base_out
            .checked_add(passive.amount_out)
            .ok_or(QuoteError::Overflow)?;
        passive_state.sqrt_price_x64 = passive.next_sqrt_price_x64;
    }

    let amount_out = active_base_out
        .checked_add(passive_base_out)
        .ok_or(QuoteError::Overflow)?;

    Ok(HybridQuote {
        amount_in: quote_in - remaining_quote,
        amount_out,
        active_base_out,
        passive_base_out,
        next_sqrt_price_x64: passive_state.sqrt_price_x64,
        remaining_active_base: ask.base_qty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_dollar_ask(base_qty: u64) -> LimitAsk {
        LimitAsk {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty,
        }
    }

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
        let state = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 0,
        };
        assert_eq!(
            quote_quote_in_for_base_out(state, 1),
            Err(QuoteError::ZeroLiquidity)
        );
    }

    #[test]
    fn explicit_ask_wins_equal_price() {
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            best_ask: Some(one_dollar_ask(1_000)),
        };
        let q = quote_buy_exact_in(market, 500).unwrap();
        assert_eq!(q.active_base_out, 500);
        assert_eq!(q.passive_base_out, 0);
        assert_eq!(q.remaining_active_base, 500);
        assert_eq!(q.next_sqrt_price_x64, Q64);
    }

    #[test]
    fn active_then_passive_after_order_exhausted() {
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            best_ask: Some(one_dollar_ask(100)),
        };
        let q = quote_buy_exact_in(market, 1_000).unwrap();
        assert_eq!(q.active_base_out, 100);
        assert!(q.passive_base_out > 0);
        assert_eq!(q.remaining_active_base, 0);
        assert!(q.next_sqrt_price_x64 > Q64);
    }

    #[test]
    fn passive_runs_until_worse_ask_then_active_gets_tie() {
        let start_sqrt = Q64;
        let ask_sqrt = Q64 + Q64 / 100; // ~1% higher sqrt price
        let ask_price = spot_price_x64(ask_sqrt).unwrap();
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: start_sqrt,
                liquidity: 1_000_000,
            },
            best_ask: Some(LimitAsk {
                price_x64: ask_price,
                sqrt_price_x64: ask_sqrt,
                base_qty: 5_000,
            }),
        };

        let q = quote_buy_exact_in(market, 20_000).unwrap();
        assert!(q.passive_base_out > 0);
        assert!(q.active_base_out > 0);
        assert!(q.remaining_active_base < 5_000);
    }

    #[test]
    fn rejects_bad_cached_sqrt_price() {
        let ask = LimitAsk {
            price_x64: Q64,
            sqrt_price_x64: Q64 + 10,
            base_qty: 1,
        };
        assert_eq!(
            validate_limit_ask(ask),
            Err(QuoteError::InvalidCachedSqrtPrice)
        );
    }
}
