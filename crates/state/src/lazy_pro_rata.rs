//! Deterministic cumulative pro-rata settlement at an authenticated tick.
//!
//! No maker account writes occur during the matching step. The on-chain
//! sealer commits these exact frozen amounts into the immutable epoch root.
//! All calculations use u128 intermediates with u64 aggregate weight caps,
//! so multiplication cannot overflow and floor dust telescopes exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProRataError {
    Invalid,
    Overflow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MakerAllocation {
    pub base_fill: u64,
    pub quote_proceeds: u64,
    pub quote_fee: u64,
}

/// Ask-side total quote and fee are positive *separate* vault inflows.
/// Returning zero-fill makers is intentional: their order stays resting,
/// and their sealed zero allocation is never eligible for a claim.
pub fn allocate_ask(
    weights: &[u64],
    filled_base: u64,
    quote_proceeds: u64,
    quote_fee: u64,
) -> Result<Vec<MakerAllocation>, ProRataError> {
    if weights.is_empty() || weights.len() > 32 || filled_base == 0 || quote_proceeds == 0 {
        return Err(ProRataError::Invalid);
    }
    let total = weights
        .iter()
        .try_fold(0u64, |sum, weight| {
            if *weight == 0 {
                return None;
            }
            sum.checked_add(*weight)
        })
        .ok_or(ProRataError::Overflow)?;
    if filled_base > total {
        return Err(ProRataError::Invalid);
    }
    let mut weight_prefix = 0u64;
    let mut fill_prefix = 0u64;
    let mut allocations = Vec::with_capacity(weights.len());
    for weight in weights {
        let weight_next = weight_prefix
            .checked_add(*weight)
            .ok_or(ProRataError::Overflow)?;
        let filled_next =
            ((u128::from(filled_base) * u128::from(weight_next)) / u128::from(total)) as u64;
        let base_fill = filled_next
            .checked_sub(fill_prefix)
            .ok_or(ProRataError::Invalid)?;
        let proceeds_next = ((u128::from(quote_proceeds) * u128::from(filled_next))
            / u128::from(filled_base)) as u64;
        let proceeds_before = ((u128::from(quote_proceeds) * u128::from(fill_prefix))
            / u128::from(filled_base)) as u64;
        let fee_next =
            ((u128::from(quote_fee) * u128::from(filled_next)) / u128::from(filled_base)) as u64;
        let fee_before =
            ((u128::from(quote_fee) * u128::from(fill_prefix)) / u128::from(filled_base)) as u64;
        allocations.push(MakerAllocation {
            base_fill,
            quote_proceeds: proceeds_next
                .checked_sub(proceeds_before)
                .ok_or(ProRataError::Invalid)?,
            quote_fee: fee_next
                .checked_sub(fee_before)
                .ok_or(ProRataError::Invalid)?,
        });
        weight_prefix = weight_next;
        fill_prefix = filled_next;
    }
    if fill_prefix != filled_base
        || allocations
            .iter()
            .map(|item| u128::from(item.quote_proceeds))
            .sum::<u128>()
            != u128::from(quote_proceeds)
        || allocations
            .iter()
            .map(|item| u128::from(item.quote_fee))
            .sum::<u128>()
            != u128::from(quote_fee)
        || allocations
            .iter()
            .any(|item| item.base_fill == 0 && (item.quote_proceeds != 0 || item.quote_fee != 0))
    {
        return Err(ProRataError::Invalid);
    }
    Ok(allocations)
}

/// BID-side cumulative pro-rata allocation in vault token atoms. The maker
/// pays quote to the taker and receives base, net of a base-denominated fee.
/// This is independent of the ASK-side quote-fee rounding rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MakerBidAllocation {
    /// Net base atoms reserved for an eventual maker claim.
    pub base_proceeds: u64,
    /// Historical locked quote atoms to debit upon the maker's claim.
    pub quote_spent: u64,
    /// Base atoms permanently segregated as protocol fees.
    pub base_fee: u64,
}

/// Assign an authenticated, single-price BID fill to a canonical maker roster.
///
/// All three quantities telescope by cumulative prefix, so neither splitting
/// an order nor odd-weight rounding can manufacture or destroy token atoms.
/// The caller must separately authenticate the roster, executable price,
/// resting quote collateral, and the taker's actual SPL transfers.
pub fn allocate_bid(
    weights: &[u64],
    filled_base: u64,
    quote_paid: u64,
    base_fee: u64,
) -> Result<Vec<MakerBidAllocation>, ProRataError> {
    if weights.is_empty()
        || weights.len() > 32
        || filled_base == 0
        || quote_paid == 0
        || base_fee >= filled_base
    {
        return Err(ProRataError::Invalid);
    }
    let total = weights
        .iter()
        .try_fold(0u64, |sum, weight| {
            if *weight == 0 {
                return None;
            }
            sum.checked_add(*weight)
        })
        .ok_or(ProRataError::Overflow)?;
    if filled_base > total {
        return Err(ProRataError::Invalid);
    }

    let mut weight_prefix = 0u64;
    let mut fill_prefix = 0u64;
    let mut allocations = Vec::with_capacity(weights.len());
    for weight in weights {
        let next_weight = weight_prefix
            .checked_add(*weight)
            .ok_or(ProRataError::Overflow)?;
        let next_fill =
            ((u128::from(filled_base) * u128::from(next_weight)) / u128::from(total)) as u64;
        let filled = next_fill
            .checked_sub(fill_prefix)
            .ok_or(ProRataError::Invalid)?;
        let quote_before = ((u128::from(quote_paid) * u128::from(fill_prefix))
            / u128::from(filled_base)) as u64;
        let quote_after = ((u128::from(quote_paid) * u128::from(next_fill))
            / u128::from(filled_base)) as u64;
        let fee_before = ((u128::from(base_fee) * u128::from(fill_prefix))
            / u128::from(filled_base)) as u64;
        let fee_after =
            ((u128::from(base_fee) * u128::from(next_fill)) / u128::from(filled_base)) as u64;
        let fee = fee_after
            .checked_sub(fee_before)
            .ok_or(ProRataError::Invalid)?;
        let proceeds = filled.checked_sub(fee).ok_or(ProRataError::Invalid)?;
        let spent = quote_after
            .checked_sub(quote_before)
            .ok_or(ProRataError::Invalid)?;
        // A positive fill without both a maker credit and an authenticated
        // quote debit cannot be claimed through opcode 40. Reject it early.
        if (filled == 0 && (spent != 0 || fee != 0))
            || (filled > 0 && (proceeds == 0 || spent == 0))
        {
            return Err(ProRataError::Invalid);
        }
        allocations.push(MakerBidAllocation {
            base_proceeds: proceeds,
            quote_spent: spent,
            base_fee: fee,
        });
        weight_prefix = next_weight;
        fill_prefix = next_fill;
    }
    if fill_prefix != filled_base
        || allocations
            .iter()
            .map(|entry| u128::from(entry.base_proceeds) + u128::from(entry.base_fee))
            .sum::<u128>()
            != u128::from(filled_base)
        || allocations
            .iter()
            .map(|entry| u128::from(entry.quote_spent))
            .sum::<u128>()
            != u128::from(quote_paid)
        || allocations
            .iter()
            .map(|entry| u128::from(entry.base_fee))
            .sum::<u128>()
            != u128::from(base_fee)
    {
        return Err(ProRataError::Invalid);
    }
    Ok(allocations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_fills_conserve_all_dust_and_prevent_zero_fill_proceeds() {
        let allocations = allocate_ask(&[1, 1, 1], 2, 7, 2).unwrap();
        assert_eq!(
            allocations,
            vec![
                MakerAllocation {
                    base_fill: 0,
                    quote_proceeds: 0,
                    quote_fee: 0
                },
                MakerAllocation {
                    base_fill: 1,
                    quote_proceeds: 3,
                    quote_fee: 1
                },
                MakerAllocation {
                    base_fill: 1,
                    quote_proceeds: 4,
                    quote_fee: 1
                },
            ]
        );
        for (weights, fill, quote, fee) in [
            (vec![1; 8], 3, 11, 2),
            (vec![1, 2, 3, 4], 7, 29, 1),
            (vec![17, 999, 3, 2], 13, 100, 7),
            (
                vec![u64::MAX / 2, u64::MAX / 2],
                u64::MAX - 1,
                u64::MAX,
                u64::MAX,
            ),
        ] {
            let claims = allocate_ask(&weights, fill, quote, fee).unwrap();
            assert_eq!(
                claims.iter().map(|c| u128::from(c.base_fill)).sum::<u128>(),
                u128::from(fill)
            );
            assert_eq!(
                claims
                    .iter()
                    .map(|c| u128::from(c.quote_proceeds))
                    .sum::<u128>(),
                u128::from(quote)
            );
            assert_eq!(
                claims.iter().map(|c| u128::from(c.quote_fee)).sum::<u128>(),
                u128::from(fee)
            );
        }
    }

    #[test]
    fn bid_side_cumulative_allocations_conserve_base_quote_fees_and_dust() {
        let amounts = allocate_bid(&[1, 1, 1], 2, 7, 1).unwrap();
        assert_eq!(
            amounts,
            vec![
                MakerBidAllocation {
                    base_proceeds: 0,
                    quote_spent: 0,
                    base_fee: 0,
                },
                MakerBidAllocation {
                    base_proceeds: 1,
                    quote_spent: 3,
                    base_fee: 0,
                },
                MakerBidAllocation {
                    base_proceeds: 0,
                    quote_spent: 4,
                    base_fee: 1,
                },
            ]
        );
        for (weights, fill, quote, fee) in [
            (vec![1; 8], 8, 25, 0),
            (vec![1, 2, 3, 4], 10, 40, 1),
            (vec![17, 999, 3, 2], 1021, 4_084, 3),
            (vec![u64::MAX / 2, u64::MAX / 2], u64::MAX - 1, u64::MAX, 1),
        ] {
            let claims = allocate_bid(&weights, fill, quote, fee).unwrap();
            assert_eq!(
                claims
                    .iter()
                    .map(|x| u128::from(x.base_proceeds) + u128::from(x.base_fee))
                    .sum::<u128>(),
                u128::from(fill)
            );
            assert_eq!(
                claims.iter().map(|x| u128::from(x.quote_spent)).sum::<u128>(),
                u128::from(quote)
            );
            assert_eq!(
                claims.iter().map(|x| u128::from(x.base_fee)).sum::<u128>(),
                u128::from(fee)
            );
        }
    }

    #[test]
    fn bid_allocation_rejects_unclaimable_dust_overfill_and_weight_overflow() {
        assert_eq!(allocate_bid(&[1, 1], 3, 3, 0), Err(ProRataError::Invalid));
        assert_eq!(allocate_bid(&[1, 0], 1, 2, 0), Err(ProRataError::Overflow));
        assert_eq!(
            allocate_bid(&[u64::MAX, 1], 1, 2, 0),
            Err(ProRataError::Overflow)
        );
        assert_eq!(allocate_bid(&[1], 1, 0, 0), Err(ProRataError::Invalid));
        assert_eq!(allocate_bid(&[1], 1, 1, 1), Err(ProRataError::Invalid));
        assert_eq!(allocate_bid(&[1, 1], 2, 1, 0), Err(ProRataError::Invalid));
        assert_eq!(allocate_bid(&[1, 1], 2, 2, 1), Err(ProRataError::Invalid));
        assert_eq!(allocate_bid(&[1; 33], 33, 33, 0), Err(ProRataError::Invalid));
    }

    #[test]
    fn reject_invalid_overfill_or_weight_overflow() {
        assert_eq!(allocate_ask(&[1, 1], 3, 2, 0), Err(ProRataError::Invalid));
        assert_eq!(allocate_ask(&[0, 1], 1, 2, 0), Err(ProRataError::Overflow));
        assert_eq!(
            allocate_ask(&[u64::MAX, 1], 1, 2, 0),
            Err(ProRataError::Overflow)
        );
        assert_eq!(allocate_ask(&[1], 0, 2, 0), Err(ProRataError::Invalid));
        assert_eq!(allocate_ask(&[1], 1, 0, 0), Err(ProRataError::Invalid));
    }
}
