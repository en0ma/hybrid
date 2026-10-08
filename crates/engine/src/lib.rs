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

/// Multiply two u128 values into little-endian 32-bit limbs.
/// The extra two limbs leave room for multiplication by Q64.
fn range_product_q64(a: u128, b: u128) -> [u32; 10] {
    let mut product = [0u32; 8];
    for i in 0..4 {
        let mut carry = 0u64;
        for j in 0..4 {
            let existing = u64::from(product[i + j]);
            let part = ((a >> (i * 32)) & 0xffff_ffff) as u64;
            let other = ((b >> (j * 32)) & 0xffff_ffff) as u64;
            let sum = u128::from(part) * u128::from(other)
                + u128::from(existing)
                + u128::from(carry);
            product[i + j] = sum as u32;
            carry = (sum >> 32) as u64;
        }
        product[i + 4] = carry as u32;
    }
    let mut result = [0u32; 10];
    result[2..].copy_from_slice(&product);
    result
}

/// Divide up to 320 bits by u128. Return the quotient and remainder.
/// Track the carry bit before shifting a 128-bit remainder.
fn div_range_limbs(
    value: [u32; 10],
    divisor: u128,
) -> Result<([u32; 10], u128), QuoteError> {
    if divisor == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    let mut quotient = [0u32; 10];
    let mut remainder = 0u128;
    for bit in (0..320).rev() {
        let carry = remainder >> 127;
        remainder = (remainder << 1)
            | u128::from((value[bit / 32] >> (bit % 32)) & 1);
        if carry != 0 || remainder >= divisor {
            remainder = remainder.wrapping_sub(divisor);
            quotient[bit / 32] |= 1u32 << (bit % 32);
        }
    }
    Ok((quotient, remainder))
}

/// Exact ceil(L * (upper - lower) * Q64 / lower / upper).
/// The wide numerator avoids false overflow for large Q64 price bounds.
fn range_base_ceil(
    liquidity: u128,
    lower: u128,
    upper: u128,
) -> Result<u64, QuoteError> {
    let numerator = range_product_q64(liquidity, upper - lower);
    let (quotient, first_remainder) = div_range_limbs(numerator, lower)?;
    let (result, second_remainder) = div_range_limbs(quotient, upper)?;
    if result[2..].iter().any(|digit| *digit != 0) {
        return Err(QuoteError::Overflow);
    }
    let whole = u64::from(result[0]) | (u64::from(result[1]) << 32);
    let rounded = u64::from(first_remainder != 0 || second_remainder != 0);
    whole.checked_add(rounded).ok_or(QuoteError::Overflow)
}

/// Minimum tokens that must back a range position at the current price.
/// Amounts are rounded up. This calculation does not mint liquidity or move
/// tokens. The caller must check custody and the position owner.
pub fn required_range_deposit(
    state: PassiveState,
    lower_sqrt_price_x64: u128,
    upper_sqrt_price_x64: u128,
) -> Result<(u64, u64), QuoteError> {
    if state.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if lower_sqrt_price_x64 == 0
        || lower_sqrt_price_x64 >= upper_sqrt_price_x64
        || state.sqrt_price_x64 == 0
    {
        return Err(QuoteError::InvalidPrice);
    }
    let current = state.sqrt_price_x64;
    let base_lower = current.max(lower_sqrt_price_x64);
    let base = if base_lower >= upper_sqrt_price_x64 {
        0
    } else {
        range_base_ceil(state.liquidity, base_lower, upper_sqrt_price_x64)?
    };
    let quote_upper = current.min(upper_sqrt_price_x64);
    let quote = if quote_upper <= lower_sqrt_price_x64 {
        0
    } else {
        mul_q64_ceil(state.liquidity, quote_upper - lower_sqrt_price_x64)?
    };
    Ok((
        base,
        u64::try_from(quote).map_err(|_| QuoteError::Overflow)?,
    ))
}

/// Reject a position if its declared principal cannot cover its liquidity
/// at the current price. Excess deposits remain the LP's reserved principal.
pub fn validate_range_collateral(
    state: PassiveState,
    lower_sqrt_price_x64: u128,
    upper_sqrt_price_x64: u128,
    base_principal: u64,
    quote_principal: u64,
) -> Result<(), QuoteError> {
    let (base, quote) =
        required_range_deposit(state, lower_sqrt_price_x64, upper_sqrt_price_x64)?;
    if base_principal < base || quote_principal < quote {
        return Err(QuoteError::ZeroLiquidity);
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

/// A conservative, bounded passive sell quote. This quote layer does not
/// mutate accounts or authorize settlement. Inputs are base atoms and outputs
/// are quote atoms; the price moves downward.
pub fn quote_base_in_for_quote_out(state: PassiveState, base_in: u64) -> Result<Quote, QuoteError> {
    if state.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if state.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    if base_in == 0 {
        return Ok(Quote {
            amount_in: 0,
            amount_out: 0,
            next_sqrt_price_x64: state.sqrt_price_x64,
        });
    }
    // For sqrt P' <= sqrt P, base movement is
    // L * (P - P') * Q64 / (P * P').
    // Find the largest representable price movement covered by base_in.
    // This uses checked arithmetic and no approximate floating point.
    let mut lo = 1u128;
    let mut hi = state.sqrt_price_x64;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let required = passive_base_delta(state.liquidity, mid, state.sqrt_price_x64)?;
        // Reserve one atom against fractional required input to avoid an
        // optimistic floor granting more price movement than paid for.
        if mid != state.sqrt_price_x64 && required >= u128::from(base_in) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let quote_out = mul_q64_floor(state.liquidity, state.sqrt_price_x64 - lo)?;
    let quote_out = u64::try_from(quote_out).map_err(|_| QuoteError::Overflow)?;
    Ok(Quote {
        amount_in: base_in,
        amount_out: quote_out,
        next_sqrt_price_x64: lo,
    })
}

/// Find the minimum whole base amount whose passive quote covers the requested
/// exact output. A nonrepresentable target is rejected rather than silently
/// overcharging the taker. No vault or LP state is changed here.
pub fn quote_base_in_for_quote_exact_out(
    state: PassiveState,
    quote_out: u64,
) -> Result<Quote, QuoteError> {
    if state.liquidity == 0 {
        return Err(QuoteError::ZeroLiquidity);
    }
    if state.sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
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
    let mut lo = 1u64;
    let mut hi = u64::MAX;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if quote_base_in_for_quote_out(state, mid)?.amount_out >= quote_out {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    quote_base_in_for_quote_out(state, lo)
}

/// Divide `remainder * 2^64` by `denominator` without a wider integer.
///
/// The precondition `remainder < denominator` keeps every doubled remainder
/// representable. The quotient is strictly smaller than `2^64`.
fn div_shift_64(remainder: u128, denominator: u128) -> Result<(u128, u128), QuoteError> {
    if denominator == 0 || remainder >= denominator {
        return Err(QuoteError::Overflow);
    }

    if remainder <= u128::from(u64::MAX) {
        let numerator = remainder << 64;
        return Ok((numerator / denominator, numerator % denominator));
    }

    let mut quotient = 0u128;
    let mut rem = remainder;
    for _ in 0..64 {
        quotient <<= 1;
        let complement = denominator - rem;
        if rem >= complement {
            rem -= complement;
            quotient |= 1;
        } else {
            rem += rem;
        }
    }
    Ok((quotient, rem))
}

fn passive_base_delta(
    liquidity: u128,
    start_sqrt_x64: u128,
    end_sqrt_x64: u128,
) -> Result<u128, QuoteError> {
    if start_sqrt_x64 == 0 || end_sqrt_x64 < start_sqrt_x64 {
        return Err(QuoteError::InvalidPrice);
    }

    // Exact target:
    // floor(liquidity * delta * Q64 / start / end)
    //
    // Dividing by start before multiplying by Q64 would normally lose the
    // start-division remainder. Carry that remainder through the Q64 scaling
    // explicitly so the result has the same single-floor semantics without a
    // 256-bit intermediate.
    let scaled_delta = liquidity
        .checked_mul(end_sqrt_x64 - start_sqrt_x64)
        .ok_or(QuoteError::Overflow)?;
    let start_quotient = scaled_delta / start_sqrt_x64;
    let start_remainder = scaled_delta % start_sqrt_x64;
    let (fraction_x64, _) = div_shift_64(start_remainder, start_sqrt_x64)?;

    if start_quotient <= u128::from(u64::MAX) {
        let scaled = start_quotient
            .checked_mul(Q64)
            .and_then(|value| value.checked_add(fraction_x64))
            .ok_or(QuoteError::Overflow)?;
        return Ok(scaled / end_sqrt_x64);
    }

    let end_quotient = start_quotient / end_sqrt_x64;
    let end_remainder = start_quotient % end_sqrt_x64;
    let whole = end_quotient.checked_mul(Q64).ok_or(QuoteError::Overflow)?;

    let (fraction_quotient, fraction_remainder) = div_shift_64(end_remainder, end_sqrt_x64)?;
    let carried_quotient = fraction_x64 / end_sqrt_x64;
    let carried_remainder = fraction_x64 % end_sqrt_x64;
    let carry = if carried_remainder >= end_sqrt_x64 - fraction_remainder {
        1
    } else {
        0
    };

    whole
        .checked_add(fraction_quotient)
        .and_then(|value| value.checked_add(carried_quotient))
        .and_then(|value| value.checked_add(carry))
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

pub fn quote_for_base_at_price(base: u64, price_x64: u128) -> Result<u64, QuoteError> {
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

pub const MAX_ACTIVE_FILLS: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveFill {
    pub ask_index: u16,
    pub base_qty: u64,
    pub quote_qty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuyExecutionPlan {
    pub quote: MultiLevelQuote,
    pub fill_count: u8,
    pub fills: [ActiveFill; MAX_ACTIVE_FILLS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveOnlyBuyPlan {
    pub amount_in: u64,
    pub amount_out: u64,
    pub fill_count: u8,
    pub fills: [ActiveFill; MAX_ACTIVE_FILLS],
}

fn validate_active_asks(asks: &[LimitAsk]) -> Result<(), QuoteError> {
    for ask in asks {
        validate_limit_ask(*ask)?;
    }
    for pair in asks.windows(2) {
        if pair[0].price_x64 > pair[1].price_x64 {
            return Err(QuoteError::InvalidPrice);
        }
    }
    Ok(())
}

pub fn plan_buy_active_exact_in(
    passive_sqrt_price_x64: u128,
    asks: &[LimitAsk],
    quote_in: u64,
) -> Result<ActiveOnlyBuyPlan, QuoteError> {
    if passive_sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    validate_active_asks(asks)?;
    let passive_price_x64 = spot_price_x64(passive_sqrt_price_x64)?;
    let mut remaining_quote = quote_in;
    let mut amount_out = 0u64;
    let mut fills = [ActiveFill::default(); MAX_ACTIVE_FILLS];
    let mut fill_count = 0usize;

    for (ask_index, ask) in asks.iter().copied().enumerate() {
        if remaining_quote == 0 || ask.price_x64 > passive_price_x64 {
            break;
        }
        let (quote_used, base_fill, _) = fill_active_ask(ask, remaining_quote)?;
        if base_fill == 0 {
            break;
        }
        if fill_count >= MAX_ACTIVE_FILLS {
            return Err(QuoteError::Overflow);
        }
        fills[fill_count] = ActiveFill {
            ask_index: u16::try_from(ask_index).map_err(|_| QuoteError::Overflow)?,
            base_qty: base_fill,
            quote_qty: quote_used,
        };
        fill_count += 1;
        remaining_quote -= quote_used;
        amount_out = amount_out
            .checked_add(base_fill)
            .ok_or(QuoteError::Overflow)?;
        if base_fill < ask.base_qty {
            break;
        }
    }

    Ok(ActiveOnlyBuyPlan {
        amount_in: quote_in - remaining_quote,
        amount_out,
        fill_count: u8::try_from(fill_count).map_err(|_| QuoteError::Overflow)?,
        fills,
    })
}

pub fn plan_buy_active_exact_out(
    passive_sqrt_price_x64: u128,
    asks: &[LimitAsk],
    base_out: u64,
) -> Result<ActiveOnlyBuyPlan, QuoteError> {
    if passive_sqrt_price_x64 == 0 {
        return Err(QuoteError::InvalidPrice);
    }
    validate_active_asks(asks)?;
    let passive_price_x64 = spot_price_x64(passive_sqrt_price_x64)?;
    let mut remaining_base = base_out;
    let mut amount_in = 0u64;
    let mut fills = [ActiveFill::default(); MAX_ACTIVE_FILLS];
    let mut fill_count = 0usize;

    for (ask_index, ask) in asks.iter().copied().enumerate() {
        if remaining_base == 0 || ask.price_x64 > passive_price_x64 {
            break;
        }
        let base_fill = remaining_base.min(ask.base_qty);
        if base_fill == 0 {
            continue;
        }
        if fill_count >= MAX_ACTIVE_FILLS {
            return Err(QuoteError::Overflow);
        }
        let quote_used = quote_for_base_at_price(base_fill, ask.price_x64)?;
        fills[fill_count] = ActiveFill {
            ask_index: u16::try_from(ask_index).map_err(|_| QuoteError::Overflow)?,
            base_qty: base_fill,
            quote_qty: quote_used,
        };
        fill_count += 1;
        remaining_base -= base_fill;
        amount_in = amount_in
            .checked_add(quote_used)
            .ok_or(QuoteError::Overflow)?;
    }

    Ok(ActiveOnlyBuyPlan {
        amount_in,
        amount_out: base_out - remaining_base,
        fill_count: u8::try_from(fill_count).map_err(|_| QuoteError::Overflow)?,
        fills,
    })
}

/// Sell plans consume bids in descending price-time order. The bid index is
/// stable until execution applies all fills in reverse index order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveBidFill {
    pub bid_index: u16,
    pub base_qty: u64,
    pub quote_qty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveOnlySellPlan {
    pub amount_in: u64,
    pub amount_out: u64,
    pub fill_count: u8,
    pub fills: [ActiveBidFill; MAX_ACTIVE_FILLS],
}

fn validate_active_bids(bids: &[LimitAsk]) -> Result<(), QuoteError> {
    for bid in bids {
        validate_limit_ask(*bid)?;
    }
    for pair in bids.windows(2) {
        if pair[0].price_x64 < pair[1].price_x64 {
            return Err(QuoteError::InvalidPrice);
        }
    }
    Ok(())
}

fn bid_quote_for_base(base: u64, price_x64: u128) -> Result<u64, QuoteError> {
    let value = bid_quote_for_base_wide(base, price_x64)?;
    u64::try_from(value).map_err(|_| QuoteError::Overflow)
}

fn bid_quote_for_base_wide(base: u64, price_x64: u128) -> Result<u128, QuoteError> {
    mul_q64_floor(u128::from(base), price_x64)
}

/// Plan a base exact-in sell against eligible explicit bids only.
/// Resting bids own equality with the passive marginal price.
/// Quote proceeds round down. Zero-quote dust cannot remove resting orders.
pub fn plan_sell_active_exact_in(
    passive_sqrt_price_x64: u128,
    bids: &[LimitAsk],
    base_in: u64,
) -> Result<ActiveOnlySellPlan, QuoteError> {
    let passive_price = spot_price_x64(passive_sqrt_price_x64)?;
    validate_active_bids(bids)?;
    let mut remaining = base_in;
    let mut out = 0u64;
    let mut fills = [ActiveBidFill::default(); MAX_ACTIVE_FILLS];
    let mut count = 0usize;
    for (index, bid) in bids.iter().copied().enumerate() {
        if remaining == 0 || bid.price_x64 < passive_price {
            break;
        }
        let take = remaining.min(bid.base_qty);
        if take == 0 {
            continue;
        }
        let quote = bid_quote_for_base(take, bid.price_x64)?;
        if quote == 0 {
            continue;
        }
        if count == MAX_ACTIVE_FILLS {
            return Err(QuoteError::Overflow);
        }
        fills[count] = ActiveBidFill {
            bid_index: u16::try_from(index).map_err(|_| QuoteError::Overflow)?,
            base_qty: take,
            quote_qty: quote,
        };
        count += 1;
        remaining -= take;
        out = out.checked_add(quote).ok_or(QuoteError::Overflow)?;
    }
    Ok(ActiveOnlySellPlan {
        amount_in: base_in - remaining,
        amount_out: out,
        fill_count: u8::try_from(count).map_err(|_| QuoteError::Overflow)?,
        fills,
    })
}

/// Plan a quote exact-out sell using the minimum whole base atoms per bid.
/// Rounding can produce more quote atoms than requested. Callers must use
/// amount_out (the actual settlement amount) and enforce their max-base limit.
pub fn plan_sell_active_exact_out(
    passive_sqrt_price_x64: u128,
    bids: &[LimitAsk],
    quote_out: u64,
) -> Result<ActiveOnlySellPlan, QuoteError> {
    let passive_price = spot_price_x64(passive_sqrt_price_x64)?;
    validate_active_bids(bids)?;
    let mut remaining = quote_out;
    let mut base_in = 0u64;
    let mut total_quote = 0u64;
    let mut fills = [ActiveBidFill::default(); MAX_ACTIVE_FILLS];
    let mut count = 0usize;
    for (index, bid) in bids.iter().copied().enumerate() {
        if remaining == 0 || bid.price_x64 < passive_price {
            break;
        }
        if bid.base_qty == 0 {
            continue;
        }
        let available = bid_quote_for_base_wide(bid.base_qty, bid.price_x64)?;
        if available == 0 {
            continue;
        }
        let (take, quote) = if available < u128::from(remaining) {
            (
                bid.base_qty,
                u64::try_from(available).map_err(|_| QuoteError::Overflow)?,
            )
        } else {
            let mut lo = 1u64;
            let mut hi = bid.base_qty;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if bid_quote_for_base_wide(mid, bid.price_x64)? >= u128::from(remaining) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            (lo, bid_quote_for_base(lo, bid.price_x64)?)
        };
        if count == MAX_ACTIVE_FILLS {
            return Err(QuoteError::Overflow);
        }
        fills[count] = ActiveBidFill {
            bid_index: u16::try_from(index).map_err(|_| QuoteError::Overflow)?,
            base_qty: take,
            quote_qty: quote,
        };
        count += 1;
        base_in = base_in.checked_add(take).ok_or(QuoteError::Overflow)?;
        total_quote = total_quote.checked_add(quote).ok_or(QuoteError::Overflow)?;
        remaining = remaining.saturating_sub(quote);
    }
    Ok(ActiveOnlySellPlan {
        amount_in: base_in,
        amount_out: total_quote,
        fill_count: u8::try_from(count).map_err(|_| QuoteError::Overflow)?,
        fills,
    })
}

fn fill_active_ask(ask: LimitAsk, remaining_quote: u64) -> Result<(u64, u64, bool), QuoteError> {
    validate_limit_ask(ask)?;
    let numerator = u128::from(remaining_quote)
        .checked_mul(Q64)
        .ok_or(QuoteError::Overflow)?;
    let affordable = numerator / ask.price_x64;
    let base_fill = u64::try_from(affordable.min(u128::from(ask.base_qty)))
        .map_err(|_| QuoteError::Overflow)?;
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
fn match_buy_exact_in_levels(
    passive: PassiveState,
    asks: &[LimitAsk],
    boundaries: &[PassiveBoundary],
    quote_in: u64,
    mut fills: Option<&mut [ActiveFill; MAX_ACTIVE_FILLS]>,
) -> Result<(MultiLevelQuote, usize), QuoteError> {
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
    let mut fill_count = 0usize;

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
                let active_index = ask_index;
                let (quote_used, base_fill, full) = fill_active_ask(ask, remaining_quote)?;
                if base_fill == 0 {
                    break;
                }
                if let Some(plan) = fills.as_deref_mut() {
                    if fill_count >= MAX_ACTIVE_FILLS {
                        return Err(QuoteError::Overflow);
                    }
                    plan[fill_count] = ActiveFill {
                        ask_index: u16::try_from(active_index).map_err(|_| QuoteError::Overflow)?,
                        base_qty: base_fill,
                        quote_qty: quote_used,
                    };
                    fill_count += 1;
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

    Ok((
        MultiLevelQuote {
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
        },
        fill_count,
    ))
}

pub fn quote_buy_exact_in_levels(
    passive: PassiveState,
    asks: &[LimitAsk],
    boundaries: &[PassiveBoundary],
    quote_in: u64,
) -> Result<MultiLevelQuote, QuoteError> {
    match_buy_exact_in_levels(passive, asks, boundaries, quote_in, None).map(|(quote, _)| quote)
}

pub fn plan_buy_exact_in_levels(
    passive: PassiveState,
    asks: &[LimitAsk],
    boundaries: &[PassiveBoundary],
    quote_in: u64,
) -> Result<BuyExecutionPlan, QuoteError> {
    let mut fills = [ActiveFill::default(); MAX_ACTIVE_FILLS];
    let (quote, fill_count) =
        match_buy_exact_in_levels(passive, asks, boundaries, quote_in, Some(&mut fills))?;
    Ok(BuyExecutionPlan {
        quote,
        fill_count: u8::try_from(fill_count).map_err(|_| QuoteError::Overflow)?,
        fills,
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
    fn active_only_exact_in_stops_before_passive_price() {
        let passive_sqrt = Q64;
        let asks = [
            one_dollar_ask(100),
            LimitAsk {
                price_x64: Q64 + 1,
                sqrt_price_x64: Q64,
                base_qty: 100,
            },
        ];
        assert_eq!(validate_limit_ask(asks[1]), Ok(()));

        let asks = [
            one_dollar_ask(100),
            LimitAsk {
                price_x64: spot_price_x64(Q64 + 1).unwrap(),
                sqrt_price_x64: Q64 + 1,
                base_qty: 100,
            },
        ];
        let plan = plan_buy_active_exact_in(passive_sqrt, &asks, 150).unwrap();
        assert_eq!(plan.amount_in, 100);
        assert_eq!(plan.amount_out, 100);
        assert_eq!(plan.fill_count, 1);
    }

    #[test]
    fn active_fill_clamps_low_price_affordability_before_u64_narrowing() {
        let ask = LimitAsk {
            price_x64: 1,
            sqrt_price_x64: 6_074_000_999,
            base_qty: 1,
        };
        assert_eq!(validate_limit_ask(ask), Ok(()));

        let plan = plan_buy_active_exact_in(Q64, &[ask], 1).unwrap();
        assert_eq!(plan.amount_in, 1);
        assert_eq!(plan.amount_out, 1);
        assert_eq!(plan.fill_count, 1);
        assert_eq!(plan.fills[0].base_qty, 1);
        assert_eq!(plan.fills[0].quote_qty, 1);
    }

    #[test]
    fn active_only_exact_out_uses_price_time_order() {
        let asks = [one_dollar_ask(100), one_dollar_ask(200)];
        let plan = plan_buy_active_exact_out(Q64, &asks, 250).unwrap();
        assert_eq!(plan.amount_out, 250);
        assert_eq!(plan.amount_in, 250);
        assert_eq!(plan.fill_count, 2);
        assert_eq!(plan.fills[0].ask_index, 0);
        assert_eq!(plan.fills[0].base_qty, 100);
        assert_eq!(plan.fills[1].ask_index, 1);
        assert_eq!(plan.fills[1].base_qty, 150);
    }

    #[test]
    fn active_only_exact_out_reports_partial_when_passive_is_required() {
        let asks = [one_dollar_ask(100)];
        let plan = plan_buy_active_exact_out(Q64, &asks, 150).unwrap();
        assert_eq!(plan.amount_out, 100);
        assert_eq!(plan.amount_in, 100);
    }

    #[test]
    fn execution_plan_matches_quote_and_records_active_fills() {
        let asks = [
            one_dollar_ask(100),
            one_dollar_ask(200),
            LimitAsk {
                price_x64: spot_price_x64(Q64 + Q64 / 100).unwrap(),
                sqrt_price_x64: Q64 + Q64 / 100,
                base_qty: 500,
            },
        ];
        let passive = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 1_000_000,
        };
        let quote = quote_buy_exact_in_levels(passive, &asks, &[], 250).unwrap();
        let plan = plan_buy_exact_in_levels(passive, &asks, &[], 250).unwrap();

        assert_eq!(plan.quote, quote);
        assert_eq!(plan.fill_count, 2);
        assert_eq!(
            &plan.fills[..usize::from(plan.fill_count)],
            &[
                ActiveFill {
                    ask_index: 0,
                    base_qty: 100,
                    quote_qty: 100,
                },
                ActiveFill {
                    ask_index: 1,
                    base_qty: 150,
                    quote_qty: 150,
                },
            ]
        );
    }

    #[test]
    fn execution_plan_keeps_passive_only_trade_fill_free() {
        let passive = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 1_000_000,
        };
        let plan = plan_buy_exact_in_levels(passive, &[], &[], 500).unwrap();
        assert_eq!(plan.fill_count, 0);
        assert_eq!(plan.quote.active_base_out, 0);
        assert!(plan.quote.passive_base_out > 0);
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
    fn spot_price_is_monotone_near_q64() {
        let mut previous = spot_price_x64(Q64).unwrap();
        for offset in 1..=10_000u128 {
            let current = spot_price_x64(Q64 + offset).unwrap();
            assert!(current >= previous);
            previous = current;
        }
    }

    #[test]
    fn active_affordability_rounding_is_maximal() {
        let prices = [
            Q64,
            Q64 + Q64 / 10_000,
            Q64 + Q64 / 100,
            2 * Q64,
            4 * Q64 - 1,
        ];
        let quotes = [0u64, 1, 2, 3, 10, 100, 1_000, 1_000_000];

        for price_x64 in prices {
            for quote in quotes {
                let base = base_for_quote_at_price(quote, price_x64).unwrap();
                let used = quote_for_base_at_price(base, price_x64).unwrap();
                assert!(used <= quote);

                if base < u64::MAX {
                    let next = base + 1;
                    if let Ok(next_cost) = quote_for_base_at_price(next, price_x64) {
                        assert!(next_cost > quote);
                    }
                }
            }
        }
    }

    #[test]
    fn passive_target_cost_is_minimal_integer_input() {
        let states = [
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1,
            },
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 3,
            },
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_003,
            },
        ];
        let deltas = [1u128, 2, 17, Q64 / 20_000, Q64 / 1_000];

        for state in states {
            for delta in deltas {
                let target = state.sqrt_price_x64 + delta;
                let to_target = passive_to_target(state, target).unwrap();
                let reached = quote_quote_in_for_base_out(state, to_target.amount_in).unwrap();
                assert!(reached.next_sqrt_price_x64 >= target);

                if to_target.amount_in > 0 {
                    let below =
                        quote_quote_in_for_base_out(state, to_target.amount_in - 1).unwrap();
                    assert!(below.next_sqrt_price_x64 < target);
                }
            }
        }
    }

    #[test]
    fn passive_exact_in_is_monotone_and_does_not_overdeliver_at_or_above_one() {
        let states = [
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1,
            },
            PassiveState {
                sqrt_price_x64: Q64,
                liquidity: 1_000_003,
            },
            PassiveState {
                sqrt_price_x64: Q64 + Q64 / 100,
                liquidity: 2_000_000,
            },
        ];

        for state in states {
            let mut previous = quote_quote_in_for_base_out(state, 0).unwrap();
            for quote_in in 1..=10_000u64 {
                let current = quote_quote_in_for_base_out(state, quote_in).unwrap();
                assert_eq!(current.amount_in, quote_in);
                assert!(current.next_sqrt_price_x64 >= previous.next_sqrt_price_x64);
                assert!(current.amount_out >= previous.amount_out);
                assert!(current.amount_out <= quote_in);
                previous = current;
            }
        }
    }

    #[test]
    fn passive_output_is_monotone_at_review_counterexample() {
        let state = PassiveState {
            sqrt_price_x64: (u128::from((1u64 << 48) - 1)) << 32,
            liquidity: 1u128 << 64,
        };
        let lower = quote_quote_in_for_base_out(state, 1u64 << 32).unwrap();
        let higher = quote_quote_in_for_base_out(state, (1u64 << 32) + 1).unwrap();

        assert!(higher.next_sqrt_price_x64 >= lower.next_sqrt_price_x64);
        assert!(higher.amount_out >= lower.amount_out);
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
            quote_buy_exact_in_levels(passive, &[tombstone, live], &[], 60).unwrap();
        let without_tombstone = quote_buy_exact_in_levels(passive, &[live], &[], 60).unwrap();

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

#[cfg(test)]
mod sell_plan_regression_tests {
    use super::*;

    fn bid(price: u128, quantity: u64) -> LimitAsk {
        LimitAsk {
            price_x64: price,
            sqrt_price_x64: if price == Q64 * 4 { Q64 * 2 } else { Q64 },
            base_qty: quantity,
        }
    }

    #[test]
    fn exact_in_uses_best_bid_then_stops_at_passive() {
        let bids = [bid(Q64, 8), bid(Q64, 7)];
        let plan = plan_sell_active_exact_in(Q64, &bids, 10).unwrap();
        assert_eq!(
            (plan.amount_in, plan.amount_out, plan.fill_count),
            (10, 10, 2)
        );
        assert_eq!(plan.fills[0].bid_index, 0);
        assert_eq!(plan.fills[1].base_qty, 2);
    }

    #[test]
    fn exact_out_rounds_minimal_base_and_tracks_actual_quote() {
        let bids = [bid(Q64, 5), bid(Q64, 7)];
        let plan = plan_sell_active_exact_out(Q64, &bids, 9).unwrap();
        assert_eq!(
            (plan.amount_in, plan.amount_out, plan.fill_count),
            (9, 9, 2)
        );
        assert_eq!(plan.fills[1].base_qty, 4);
    }

    #[test]
    fn sell_plans_reject_unsorted_bids_and_report_partial_depth() {
        let reversed = [bid(Q64, 1), bid(Q64 * 4, 1)];
        assert_eq!(
            plan_sell_active_exact_in(Q64, &reversed, 1),
            Err(QuoteError::InvalidPrice)
        );
        let plan = plan_sell_active_exact_out(Q64, &[bid(Q64, 2)], 10).unwrap();
        assert_eq!((plan.amount_in, plan.amount_out), (2, 2));
    }

    #[test]
    fn sell_plans_do_not_spend_bids_below_passive_price() {
        let low = LimitAsk {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: 10,
        };
        let plan = plan_sell_active_exact_in(Q64 * 2, &[low], 5).unwrap();
        assert_eq!((plan.amount_in, plan.amount_out), (0, 0));
    }

    #[test]
    fn zero_notional_bid_does_not_block_later_fills() {
        let bids = [
            LimitAsk {
                price_x64: Q64 / 4,
                sqrt_price_x64: Q64 / 2,
                base_qty: 1,
            },
            LimitAsk {
                price_x64: Q64 / 4,
                sqrt_price_x64: Q64 / 2,
                base_qty: 8,
            },
        ];
        let exact_in = plan_sell_active_exact_in(Q64 / 2, &bids, 4).unwrap();
        assert_eq!(exact_in.fill_count, 1);
        assert_eq!(exact_in.fills[0].bid_index, 1);
        assert_eq!(exact_in.amount_out, 1);
        let exact_out = plan_sell_active_exact_out(Q64 / 2, &bids, 1).unwrap();
        assert_eq!(exact_out.fill_count, 1);
        assert_eq!(exact_out.fills[0].bid_index, 1);
        assert_eq!(exact_out.amount_in, 4);
    }

    #[test]
    fn exact_out_uses_minimal_base_when_capacity_equals_target() {
        let bids = [LimitAsk {
            price_x64: Q64 / 4,
            sqrt_price_x64: Q64 / 2,
            base_qty: 7,
        }];
        let plan = plan_sell_active_exact_out(Q64 / 2, &bids, 1).unwrap();
        assert_eq!((plan.amount_in, plan.amount_out), (4, 1));
    }

    #[test]
    fn large_bid_notional_allows_small_exact_out() {
        let bids = [LimitAsk {
            price_x64: Q64 * 4,
            sqrt_price_x64: Q64 * 2,
            base_qty: u64::MAX,
        }];
        let plan = plan_sell_active_exact_out(Q64, &bids, 1).unwrap();
        assert_eq!((plan.amount_in, plan.amount_out), (1, 4));
    }
}

#[cfg(test)]
mod passive_sell_quote_tests {
    use super::*;

    fn fixture() -> PassiveState {
        PassiveState {
            sqrt_price_x64: Q64,
            liquidity: Q64,
        }
    }

    #[test]
    fn passive_sell_zero_input_preserves_state() {
        let quote = quote_base_in_for_quote_out(fixture(), 0).unwrap();
        assert_eq!(quote.amount_in, 0);
        assert_eq!(quote.amount_out, 0);
        assert_eq!(quote.next_sqrt_price_x64, Q64);
    }

    #[test]
    fn passive_sell_moves_price_downward_and_is_monotone() {
        let small = quote_base_in_for_quote_out(fixture(), 20).unwrap();
        let large = quote_base_in_for_quote_out(fixture(), 100).unwrap();
        assert_eq!(small.amount_in, 20);
        assert_eq!(large.amount_in, 100);
        assert!(small.next_sqrt_price_x64 <= Q64);
        assert!(large.next_sqrt_price_x64 <= small.next_sqrt_price_x64);
        assert!(large.amount_out >= small.amount_out);
    }

    #[test]
    fn passive_sell_exact_out_finds_minimal_input() {
        let target = 10;
        let quote = quote_base_in_for_quote_exact_out(fixture(), target).unwrap();
        assert!(quote.amount_out >= target);
        if quote.amount_in > 1 {
            let previous = quote_base_in_for_quote_out(fixture(), quote.amount_in - 1).unwrap();
            assert!(previous.amount_out < target);
        }
    }

    #[test]
    fn passive_sell_rejects_zero_liquidity() {
        let empty = PassiveState {
            sqrt_price_x64: Q64,
            liquidity: 0,
        };
        assert_eq!(
            quote_base_in_for_quote_out(empty, 1),
            Err(QuoteError::ZeroLiquidity)
        );
        assert_eq!(
            quote_base_in_for_quote_exact_out(empty, 1),
            Err(QuoteError::ZeroLiquidity)
        );
    }
}

#[cfg(test)]
mod range_collateral_tests {
    use super::*;

    #[test]
    fn inside_range_requires_both_assets() {
        let l = Q64 / 100;
        let (base, quote) = required_range_deposit(
            PassiveState { sqrt_price_x64: Q64, liquidity: l },
            Q64 - Q64 / 10,
            Q64 + Q64 / 10,
        ).unwrap();
        assert!(base > 0 && quote > 0);
        validate_range_collateral(
            PassiveState { sqrt_price_x64: Q64, liquidity: l },
            Q64 - Q64 / 10,
            Q64 + Q64 / 10,
            base, quote,
        ).unwrap();
        assert!(validate_range_collateral(
            PassiveState { sqrt_price_x64: Q64, liquidity: l },
            Q64 - Q64 / 10,
            Q64 + Q64 / 10,
            base - 1, quote,
        ).is_err());
    }

    #[test]
    fn out_of_range_uses_only_one_token() {
        let l = Q64 / 100;
        let below = required_range_deposit(
            PassiveState { sqrt_price_x64: Q64 / 2, liquidity: l },
            Q64, Q64 + Q64 / 10,
        ).unwrap();
        assert!(below.0 > 0);
        assert_eq!(below.1, 0);
        let above = required_range_deposit(
            PassiveState { sqrt_price_x64: Q64 * 2, liquidity: l },
            Q64, Q64 + Q64 / 10,
        ).unwrap();
        assert_eq!(above.0, 0);
        assert!(above.1 > 0);
    }

    #[test]
    fn invalid_ranges_and_overflow_are_rejected() {
        let state = PassiveState { sqrt_price_x64: Q64, liquidity: Q64 };
        assert_eq!(required_range_deposit(state, Q64, Q64), Err(QuoteError::InvalidPrice));
        assert_eq!(required_range_deposit(state, 0, Q64), Err(QuoteError::InvalidPrice));
        assert_eq!(
            required_range_deposit(PassiveState { liquidity: 0, ..state }, 1, Q64),
            Err(QuoteError::ZeroLiquidity)
        );
        assert!(required_range_deposit(
            PassiveState { liquidity: u128::MAX, ..state }, 1, Q64 * 2,
        ).is_err());
    }

    #[test]
    fn higher_liquidity_never_reduces_required_deposit() {
        let mut last = (0, 0);
        for units in 1..=100u128 {
            let value = required_range_deposit(
                PassiveState { sqrt_price_x64: Q64, liquidity: units * 10000 },
                Q64 - Q64 / 4, Q64 + Q64 / 4,
            ).unwrap();
            assert!(value.0 >= last.0 && value.1 >= last.1);
            last = value;
        }
    }
}
