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
