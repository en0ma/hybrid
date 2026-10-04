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

    high.checked_add(cross_a)
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

    // Require the canonical floor square root: s^2 <= price < (s+1)^2.
    // Comparing the squared Q64 prices avoids assuming a fixed error bound,
    // which is incorrect as price magnitude grows.
    let cached = spot_price_x64(order.sqrt_price_x64)?;
    if cached > order.price_x64 {
        return Err(QuoteError::InvalidCachedSqrtPrice);
    }
    let next_sqrt = order
        .sqrt_price_x64
        .checked_add(1)
        .ok_or(QuoteError::InvalidCachedSqrtPrice)?;
    let next_price = spot_price_x64(next_sqrt)?;
    if next_price <= order.price_x64 {
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

fn passive_to_target(state: PassiveState, target_sqrt_x64: u128) -> Result<Quote, QuoteError> {
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
    let base_out = passive_base_delta(state.liquidity, state.sqrt_price_x64, target_sqrt_x64)?;
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
pub fn quote_buy_exact_in(market: HybridMarket, quote_in: u64) -> Result<HybridQuote, QuoteError> {
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
        let delta = ask.sqrt_price_x64 - passive_state.sqrt_price_x64;
        let boundary_cost = mul_q64_ceil(passive_state.liquidity, delta)?;

        // Compare reachability in u128 before converting the boundary cost to
        // u64. A distant boundary can cost more than u64::MAX while a small
        // taker input is still perfectly valid.
        if boundary_cost > u128::from(remaining_quote) {
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

        // Equality deliberately lands exactly on the active-order boundary.
        // The resting explicit order owns execution priority at that price.
        let to_ask = passive_to_target(passive_state, ask.sqrt_price_x64)?;
        remaining_quote -= to_ask.amount_in;
        passive_base_out = passive_base_out
            .checked_add(to_ask.amount_out)
            .ok_or(QuoteError::Overflow)?;
        passive_state.sqrt_price_x64 = ask.sqrt_price_x64;
    }

    // Active wins at equal or better price.
    if remaining_quote > 0 && ask.base_qty > 0 {
        // Compute affordability from the taker's bounded u64 input first.
        // This avoids materializing the full notional of a very large ask,
        // which may exceed u64 even though a small partial fill is valid.
        let affordable = base_for_quote_at_price(remaining_quote, ask.price_x64)?.min(ask.base_qty);

        if affordable > 0 {
            let quote_used = quote_for_base_at_price(affordable, ask.price_x64)?;
            debug_assert!(quote_used <= remaining_quote);
            remaining_quote -= quote_used;
            ask.base_qty -= affordable;
            active_base_out = active_base_out
                .checked_add(affordable)
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
        remaining_quote = 0;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassiveBoundary {
    pub sqrt_price_x64: u128,
    pub liquidity_after: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiLevelQuote {
    pub amount_in: u64,
    pub amount_out: u64,
    pub active_base_out: u64,
    pub passive_base_out: u64,
    pub next_sqrt_price_x64: u128,
    pub final_liquidity: u128,
    pub fully_consumed_asks: u32,
    pub crossed_boundaries: u32,
}

fn fill_active_ask(ask: LimitAsk, remaining_quote: u64) -> Result<(u64, u64, bool), QuoteError> {
    validate_limit_ask(ask)?;
    let base_fill = base_for_quote_at_price(remaining_quote, ask.price_x64)?.min(ask.base_qty);
    if base_fill == 0 {
        return Ok((0, 0, false));
    }
    let quote_used = quote_for_base_at_price(base_fill, ask.price_x64)?;
    if quote_used > remaining_quote {
        return Err(QuoteError::Overflow);
    }
    Ok((quote_used, base_fill, base_fill == ask.base_qty))
}

/// V0.2 allocation-free traversal across sorted explicit asks and passive
/// liquidity boundaries.
///
/// Asks must be sorted by ascending price, then time priority within the same
/// price. Boundaries must be strictly increasing by sqrt price. Explicit asks
/// own equality: when passive marginal price reaches an ask, that resting order
/// executes before passive liquidity may move above the price.
pub fn quote_buy_exact_in_levels(
    passive: PassiveState,
    asks: &[LimitAsk],
    boundaries: &[PassiveBoundary],
    quote_in: u64,
) -> Result<MultiLevelQuote, QuoteError> {
    if passive.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if passive.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }

    for ask in asks {
        validate_limit_ask(*ask)?;
    }
    for pair in asks.windows(2) {
        if pair[0].price_x64 > pair[1].price_x64 {
            return Err(QuoteError::InvalidPrice);
        }
    }

    let mut previous_boundary = passive.sqrt_price_x64;
    for boundary in boundaries {
        if boundary.sqrt_price_x64 <= previous_boundary || boundary.liquidity_after == 0 {
            return Err(QuoteError::InvalidPrice);
        }
        previous_boundary = boundary.sqrt_price_x64;
    }

    let mut remaining_quote = quote_in;
    let mut passive_state = passive;
    let mut ask_index = 0usize;
    let mut boundary_index = 0usize;
    let mut active_base_out = 0u64;
    let mut passive_base_out = 0u64;
    let mut fully_consumed_asks = 0u32;
    let mut crossed_boundaries = 0u32;

    while remaining_quote > 0 {
        while ask_index < asks.len() && asks[ask_index].base_qty == 0 {
            ask_index += 1;
            fully_consumed_asks = fully_consumed_asks
                .checked_add(1)
                .ok_or(QuoteError::Overflow)?;
        }

        let next_ask = asks.get(ask_index).copied();

        if let Some(ask) = next_ask {
            if ask.sqrt_price_x64 <= passive_state.sqrt_price_x64 {
                let (quote_used, base_fill, full) = fill_active_ask(ask, remaining_quote)?;
                if base_fill == 0 {
                    break;
                }
                remaining_quote -= quote_used;
                active_base_out = active_base_out
                    .checked_add(base_fill)
                    .ok_or(QuoteError::Overflow)?;
                if full {
                    ask_index += 1;
                    fully_consumed_asks = fully_consumed_asks
                        .checked_add(1)
                        .ok_or(QuoteError::Overflow)?;
                    continue;
                }
                break;
            }
        }

        let ask_target = next_ask.map(|a| a.sqrt_price_x64);
        let boundary_target = boundaries.get(boundary_index).map(|b| b.sqrt_price_x64);

        let target = match (ask_target, boundary_target) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                let tail = quote_quote_in_for_base_out(passive_state, remaining_quote)?;
                passive_base_out = passive_base_out
                    .checked_add(tail.amount_out)
                    .ok_or(QuoteError::Overflow)?;
                passive_state.sqrt_price_x64 = tail.next_sqrt_price_x64;
                remaining_quote = 0;
                break;
            }
        };

        let delta = target - passive_state.sqrt_price_x64;
        let target_cost = mul_q64_ceil(passive_state.liquidity, delta)?;
        if target_cost > u128::from(remaining_quote) {
            let partial = quote_quote_in_for_base_out(passive_state, remaining_quote)?;
            passive_base_out = passive_base_out
                .checked_add(partial.amount_out)
                .ok_or(QuoteError::Overflow)?;
            passive_state.sqrt_price_x64 = partial.next_sqrt_price_x64;
            remaining_quote = 0;
            break;
        }

        let to_target = passive_to_target(passive_state, target)?;
        remaining_quote -= to_target.amount_in;
        passive_base_out = passive_base_out
            .checked_add(to_target.amount_out)
            .ok_or(QuoteError::Overflow)?;
        passive_state.sqrt_price_x64 = target;

        if ask_target == Some(target) {
            continue;
        }

        if boundary_target == Some(target) {
            let boundary = boundaries[boundary_index];
            passive_state.liquidity = boundary.liquidity_after;
            boundary_index += 1;
            crossed_boundaries = crossed_boundaries
                .checked_add(1)
                .ok_or(QuoteError::Overflow)?;
        }
    }

    Ok(MultiLevelQuote {
        amount_in: quote_in - remaining_quote,
        amount_out: active_base_out
            .checked_add(passive_base_out)
            .ok_or(QuoteError::Overflow)?,
        active_base_out,
        passive_base_out,
        next_sqrt_price_x64: passive_state.sqrt_price_x64,
        final_liquidity: passive_state.liquidity,
        fully_consumed_asks,
        crossed_boundaries,
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

    #[test]
    fn passive_tail_is_counted_as_consumed_input() {
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            best_ask: Some(one_dollar_ask(100)),
        };
        let q = quote_buy_exact_in(market, 1_000).unwrap();
        assert_eq!(q.amount_in, 1_000);
        assert_eq!(q.remaining_active_base, 0);
        assert!(q.passive_base_out > 0);
    }

    #[test]
    fn huge_ask_allows_small_partial_fill() {
        let sqrt_two_x64 = 26_087_635_650_665_564_436u128;
        let price_two_x64 = spot_price_x64(sqrt_two_x64).unwrap();
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: sqrt_two_x64,
                liquidity: 1_000_000,
            },
            best_ask: Some(LimitAsk {
                price_x64: price_two_x64,
                sqrt_price_x64: sqrt_two_x64,
                base_qty: u64::MAX,
            }),
        };
        let q = quote_buy_exact_in(market, 100).unwrap();
        assert!(q.active_base_out > 0);
        assert!(q.active_base_out < 100);
    }

    #[test]
    fn unreachable_boundary_does_not_overflow_small_trade() {
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: Q64,
                liquidity: Q64,
            },
            best_ask: Some(LimitAsk {
                price_x64: 4 * Q64,
                sqrt_price_x64: 2 * Q64,
                base_qty: 1,
            }),
        };
        let q = quote_buy_exact_in(market, 1).unwrap();
        assert_eq!(q.amount_in, 1);
        assert_eq!(q.active_base_out, 0);
        assert!(q.next_sqrt_price_x64 < 2 * Q64);
    }

    #[test]
    fn canonical_cached_sqrt_accepts_multi_atom_squared_gap() {
        let ask = LimitAsk {
            price_x64: 4 * Q64 - 1,
            sqrt_price_x64: 2 * Q64 - 1,
            base_qty: 1,
        };
        assert_eq!(validate_limit_ask(ask), Ok(()));
        assert!(ask.price_x64 - spot_price_x64(ask.sqrt_price_x64).unwrap() > 1);
    }

    #[test]
    fn exact_boundary_payment_does_not_cross_active_ask() {
        let market = HybridMarket {
            passive: PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 3,
            },
            best_ask: Some(LimitAsk {
                price_x64: spot_price_x64(Q64 + 1).unwrap(),
                sqrt_price_x64: Q64 + 1,
                base_qty: 1,
            }),
        };
        let q = quote_buy_exact_in(market, 1).unwrap();
        assert_eq!(q.next_sqrt_price_x64, Q64 + 1);
        assert_eq!(q.active_base_out, 0);
        assert_eq!(q.remaining_active_base, 1);
    }
    #[test]
    fn multilevel_consumes_equal_price_asks_in_slice_time_order() {
        let asks = [
            one_dollar_ask(100),
            one_dollar_ask(200),
            LimitAsk {
                price_x64: spot_price_x64(Q64 + Q64 / 100).unwrap(),
                sqrt_price_x64: Q64 + Q64 / 100,
                base_qty: 500,
            },
        ];
        let q = quote_buy_exact_in_levels(
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            &asks,
            &[],
            250,
        )
        .unwrap();
        assert_eq!(q.active_base_out, 250);
        assert_eq!(q.passive_base_out, 0);
        assert_eq!(q.fully_consumed_asks, 1);
    }

    #[test]
    fn multilevel_crosses_passive_boundary_before_higher_ask() {
        let boundary_sqrt = Q64 + Q64 / 200;
        let ask_sqrt = Q64 + Q64 / 100;
        let asks = [LimitAsk {
            price_x64: spot_price_x64(ask_sqrt).unwrap(),
            sqrt_price_x64: ask_sqrt,
            base_qty: 10_000,
        }];
        let boundaries = [PassiveBoundary {
            sqrt_price_x64: boundary_sqrt,
            liquidity_after: 2_000_000,
        }];
        let q = quote_buy_exact_in_levels(
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            &asks,
            &boundaries,
            30_000,
        )
        .unwrap();
        assert_eq!(q.crossed_boundaries, 1);
        assert!(q.passive_base_out > 0);
        assert!(q.active_base_out > 0);
        assert_eq!(q.final_liquidity, 2_000_000);
    }

    #[test]
    fn active_wins_when_ask_and_boundary_share_price() {
        let target = Q64 + Q64 / 100;
        let asks = [LimitAsk {
            price_x64: spot_price_x64(target).unwrap(),
            sqrt_price_x64: target,
            base_qty: 1_000,
        }];
        let boundaries = [PassiveBoundary {
            sqrt_price_x64: target,
            liquidity_after: 2_000_000,
        }];
        let q = quote_buy_exact_in_levels(
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            &asks,
            &boundaries,
            20_000,
        )
        .unwrap();
        assert_eq!(q.fully_consumed_asks, 1);
        assert_eq!(q.crossed_boundaries, 1);
    }

    #[test]
    fn rejects_unsorted_asks_and_boundaries() {
        let high = Q64 + Q64 / 100;
        let low = Q64 + Q64 / 200;
        let asks = [
            LimitAsk {
                price_x64: spot_price_x64(high).unwrap(),
                sqrt_price_x64: high,
                base_qty: 1,
            },
            LimitAsk {
                price_x64: spot_price_x64(low).unwrap(),
                sqrt_price_x64: low,
                base_qty: 1,
            },
        ];
        assert_eq!(
            quote_buy_exact_in_levels(
                PassiveState {
                    sqrt_price_x64: Q64,
                    liquidity: 1_000_000,
                },
                &asks,
                &[],
                1_000,
            ),
            Err(QuoteError::InvalidPrice)
        );

        let boundaries = [
            PassiveBoundary {
                sqrt_price_x64: high,
                liquidity_after: 1_000_000,
            },
            PassiveBoundary {
                sqrt_price_x64: low,
                liquidity_after: 1_000_000,
            },
        ];
        assert_eq!(
            quote_buy_exact_in_levels(
                PassiveState {
                    sqrt_price_x64: Q64,
                    liquidity: 1_000_000,
                },
                &[],
                &boundaries,
                1_000,
            ),
            Err(QuoteError::InvalidPrice)
        );
    }
    #[test]
    fn multilevel_skips_zero_quantity_ask_and_continues() {
        let asks = [one_dollar_ask(0), one_dollar_ask(100)];
        let q = quote_buy_exact_in_levels(
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_000,
            },
            &asks,
            &[],
            50,
        )
        .unwrap();
        assert_eq!(q.amount_in, 50);
        assert_eq!(q.active_base_out, 50);
        assert_eq!(q.passive_base_out, 0);
        assert_eq!(q.fully_consumed_asks, 1);
    }

    #[test]
    fn future_zero_quantity_ask_does_not_segment_passive_quote() {
        let tombstone_sqrt = Q64 + Q64 / 20_000;
        let live_sqrt = Q64 + Q64 / 1_000;
        let tombstone = LimitAsk {
            price_x64: spot_price_x64(tombstone_sqrt).unwrap(),
            sqrt_price_x64: tombstone_sqrt,
            base_qty: 0,
        };
        let live = LimitAsk {
            price_x64: spot_price_x64(live_sqrt).unwrap(),
            sqrt_price_x64: live_sqrt,
            base_qty: 1_000,
        };
        let passive = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 1_000_003,
        };

        let with_tombstone =
            quote_buy_exact_in_levels(passive, &[tombstone, live], &[], 37).unwrap();
        let without_tombstone = quote_buy_exact_in_levels(passive, &[live], &[], 37).unwrap();

        assert_eq!(with_tombstone.amount_in, without_tombstone.amount_in);
        assert_eq!(with_tombstone.amount_out, without_tombstone.amount_out);
        assert_eq!(
            with_tombstone.active_base_out,
            without_tombstone.active_base_out
        );
        assert_eq!(
            with_tombstone.passive_base_out,
            without_tombstone.passive_base_out
        );
        assert_eq!(
            with_tombstone.next_sqrt_price_x64,
            without_tombstone.next_sqrt_price_x64
        );
        assert_eq!(
            with_tombstone.final_liquidity,
            without_tombstone.final_liquidity
        );
        assert_eq!(
            with_tombstone.crossed_boundaries,
            without_tombstone.crossed_boundaries
        );
        assert_eq!(
            with_tombstone.fully_consumed_asks,
            without_tombstone.fully_consumed_asks + 1
        );
    }
}
