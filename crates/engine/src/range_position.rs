//! Canonical bounded passive-liquidity position validation.
//! This module does not mint shares or authorize vault movements. In
//! particular, a valid range is not proof that a swap can be settled.
use crate::{required_range_deposit, PassiveState, QuoteError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangePosition {
    pub lower_sqrt_price_x64: u128,
    pub upper_sqrt_price_x64: u128,
    pub liquidity: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeError {
    InvalidBounds,
    ZeroLiquidity,
    PriceOutsideRange,
    Arithmetic(QuoteError),
    LiquidityOverflow,
}

impl RangePosition {
    /// Validate the canonical open-upper/closed-lower sqrt-price interval.
    pub fn validate(self) -> Result<(), RangeError> {
        if self.lower_sqrt_price_x64 == 0 || self.lower_sqrt_price_x64 >= self.upper_sqrt_price_x64
        {
            return Err(RangeError::InvalidBounds);
        }
        if self.liquidity == 0 {
            return Err(RangeError::ZeroLiquidity);
        }
        Ok(())
    }

    /// Returns whether a position contributes nonzero *in-range* liquidity.
    /// Crossing its upper boundary removes its liquidity before the next step.
    pub fn active_at(self, sqrt_price_x64: u128) -> Result<bool, RangeError> {
        self.validate()?;
        Ok(sqrt_price_x64 >= self.lower_sqrt_price_x64
            && sqrt_price_x64 < self.upper_sqrt_price_x64)
    }

    /// Conservatively calculate both required deposits using the existing
    /// checked engine math, not a floating-point or off-chain approximation.
    pub fn required_deposit(self, sqrt_price_x64: u128) -> Result<(u64, u64), RangeError> {
        self.validate()?;
        if sqrt_price_x64 == 0 {
            return Err(RangeError::PriceOutsideRange);
        }
        let amounts = required_range_deposit(
            PassiveState {
                sqrt_price_x64,
                liquidity: self.liquidity,
            },
            self.lower_sqrt_price_x64,
            self.upper_sqrt_price_x64,
        )
        .map_err(RangeError::Arithmetic)?;
        Ok(amounts)
    }
}

/// Sum only currently active position liquidity; never count out-of-range LPs.
/// Price-range crossings are evaluated with the same lower-inclusive,
/// upper-exclusive convention as individual positions.
pub fn active_liquidity(
    positions: &[RangePosition],
    sqrt_price_x64: u128,
) -> Result<u128, RangeError> {
    if sqrt_price_x64 == 0 {
        return Err(RangeError::PriceOutsideRange);
    }
    let mut total = 0u128;
    for position in positions {
        if position.active_at(sqrt_price_x64)? {
            total = total
                .checked_add(position.liquidity)
                .ok_or(RangeError::LiquidityOverflow)?;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Q64;

    #[test]
    fn rejects_zero_and_reversed_ranges() {
        for range in [
            RangePosition {
                lower_sqrt_price_x64: 0,
                upper_sqrt_price_x64: Q64,
                liquidity: 1,
            },
            RangePosition {
                lower_sqrt_price_x64: Q64 * 2,
                upper_sqrt_price_x64: Q64,
                liquidity: 1,
            },
            RangePosition {
                lower_sqrt_price_x64: Q64,
                upper_sqrt_price_x64: Q64 * 2,
                liquidity: 0,
            },
        ] {
            assert!(range.validate().is_err());
        }
    }

    #[test]
    fn overlapping_ranges_sum_only_in_range_liquidity() {
        let positions = [
            RangePosition {
                lower_sqrt_price_x64: Q64,
                upper_sqrt_price_x64: Q64 * 3,
                liquidity: 7,
            },
            RangePosition {
                lower_sqrt_price_x64: Q64 * 2,
                upper_sqrt_price_x64: Q64 * 4,
                liquidity: 11,
            },
        ];
        assert_eq!(active_liquidity(&positions, Q64), Ok(7));
        assert_eq!(active_liquidity(&positions, Q64 * 2), Ok(18));
        assert_eq!(active_liquidity(&positions, Q64 * 3), Ok(11));
        assert_eq!(active_liquidity(&positions, Q64 * 4), Ok(0));
    }

    #[test]
    fn overlapping_ranges_reject_liquidity_overflow() {
        let positions = [
            RangePosition {
                lower_sqrt_price_x64: Q64,
                upper_sqrt_price_x64: Q64 * 2,
                liquidity: u128::MAX,
            },
            RangePosition {
                lower_sqrt_price_x64: Q64,
                upper_sqrt_price_x64: Q64 * 2,
                liquidity: 1,
            },
        ];
        assert_eq!(
            active_liquidity(&positions, Q64),
            Err(RangeError::LiquidityOverflow)
        );
    }

    #[test]
    fn range_boundary_has_single_canonical_owner() {
        let left = RangePosition {
            lower_sqrt_price_x64: Q64,
            upper_sqrt_price_x64: Q64 * 2,
            liquidity: 100,
        };
        let right = RangePosition {
            lower_sqrt_price_x64: Q64 * 2,
            upper_sqrt_price_x64: Q64 * 3,
            liquidity: 100,
        };
        assert_eq!(left.active_at(Q64 * 2), Ok(false));
        assert_eq!(right.active_at(Q64 * 2), Ok(true));
        assert_eq!(left.active_at(Q64), Ok(true));
    }
}
