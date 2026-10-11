//! Host-only, deterministic maker quote reconciliation.
//! Intent planning is advisory: refresh the authenticated snapshot and simulate
//! every instruction before signing/sending; never retry a submission blindly.
use crate::client::{BuildError, TwoPageBook};
use hybrid_engine::{validate_limit_ask, LimitAsk};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestingOrder {
    pub sequence: u64,
    pub index: u8,
    pub maker: Pubkey,
    pub price_x64: u128,
    pub sqrt_price_x64: u128,
    pub base_qty: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetQuote {
    pub index: u8,
    pub price_x64: u128,
    pub sqrt_price_x64: u128,
    pub base_qty: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileError {
    InvalidTarget,
    DuplicateOwnedOrder,
    UnconfirmedMutation,
    Build(BuildError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MakerAction {
    Cancel { index: u8, sequence: u64 },
    Place(TargetQuote),
}

#[derive(Clone, Debug)]
pub struct MakerPlan {
    /// Submit first phase, confirm and refresh state before any later phase.
    pub cancellations: Vec<Instruction>,
    pub placements: Vec<Instruction>,
    pub actions: Vec<MakerAction>,
}

fn target_matches(resting: &RestingOrder, target: &TargetQuote) -> bool {
    resting.index == target.index
        && resting.price_x64 == target.price_x64
        && resting.sqrt_price_x64 == target.sqrt_price_x64
        && resting.base_qty == target.base_qty
}

/// Reconcile an operator's requested two-sided inventory *for one side*.
/// Orders are not amended in-place. This function emits cancels before places.
/// Callers must **not** submit the entire plan as one transaction: a confirmed
/// fresh-state read is required between phases to avoid oversubscription.
pub fn reconcile_maker(
    book: TwoPageBook,
    maker: Pubkey,
    resting: &[RestingOrder],
    targets: &[TargetQuote],
    has_unconfirmed_mutation: bool,
) -> Result<MakerPlan, ReconcileError> {
    if has_unconfirmed_mutation {
        return Err(ReconcileError::UnconfirmedMutation);
    }
    if targets
        .iter()
        .any(|t| t.index > 1
            || t.base_qty == 0
            || validate_limit_ask(LimitAsk {
                price_x64: t.price_x64,
                sqrt_price_x64: t.sqrt_price_x64,
                base_qty: t.base_qty,
            })
            .is_err())
    {
        return Err(ReconcileError::InvalidTarget);
    }
    if resting.iter().enumerate().any(|(i, order)| {
        order.maker == maker
            && (order.index > 1
                || order.sequence == 0
                || resting[i + 1..]
                    .iter()
                    .any(|next| next.maker == maker && next.sequence == order.sequence))
    }) {
        return Err(ReconcileError::DuplicateOwnedOrder);
    }

    let ours: Vec<_> = resting.iter().filter(|r| r.maker == maker).collect();
    let mut occupied = vec![false; targets.len()];
    let mut cancellations = Vec::new();
    let mut placements = Vec::new();
    let mut actions = Vec::new();
    for existing in ours {
        if let Some((i, _)) = targets
            .iter()
            .enumerate()
            .find(|(i, target)| !occupied[*i] && target_matches(existing, target))
        {
            occupied[i] = true;
        } else {
            cancellations.push(
                book.cancel(existing.index, maker, existing.sequence)
                    .map_err(ReconcileError::Build)?,
            );
            actions.push(MakerAction::Cancel {
                index: existing.index,
                sequence: existing.sequence,
            });
        }
    }
    for (i, target) in targets.iter().enumerate() {
        if !occupied[i] {
            placements.push(
                book.place(
                    target.index,
                    maker,
                    target.price_x64,
                    target.sqrt_price_x64,
                    target.base_qty,
                )
                .map_err(ReconcileError::Build)?,
            );
            actions.push(MakerAction::Place(*target));
        }
    }
    Ok(MakerPlan {
        cancellations,
        placements,
        actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Side;
    const Q64: u128 = 1u128 << 64;

    #[test]
    fn maker_reconciliation_keeps_matching_quote_and_does_not_cancel_other_makers() {
        let market = Pubkey::new_unique();
        let maker = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let book = TwoPageBook::new(market, Pubkey::new_unique(), Side::Ask);
        let current = [
            RestingOrder {
                maker,
                sequence: 10,
                index: 0,
                price_x64: Q64,
                sqrt_price_x64: Q64,
                base_qty: 10,
            },
            RestingOrder {
                maker: other,
                sequence: 11,
                index: 0,
                price_x64: Q64,
                sqrt_price_x64: Q64,
                base_qty: 20,
            },
        ];
        let wanted = [TargetQuote {
            index: 0,
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: 10,
        }];
        let plan = reconcile_maker(book, maker, &current, &wanted, false).unwrap();
        assert!(plan.actions.is_empty());
        let changed = [TargetQuote {
            base_qty: 12,
            ..wanted[0]
        }];
        let plan = reconcile_maker(book, maker, &current, &changed, false).unwrap();
        assert_eq!(plan.cancellations.len(), 1);
        assert_eq!(plan.placements.len(), 1);
        assert_eq!(plan.cancellations[0].data[0], 31);
        assert_eq!(plan.placements[0].data[0], 30);
    }

    #[test]
    fn rejects_noncanonical_sqrt_cache_before_building_transactions() {
        let book = TwoPageBook::new(Pubkey::new_unique(), Pubkey::new_unique(), Side::Ask);
        let maker = Pubkey::new_unique();
        for (price_x64, sqrt_price_x64) in [(Q64, Q64 + 1), (Q64 * 4, Q64), (0, Q64)] {
            assert!(matches!(
                reconcile_maker(
                    book,
                    maker,
                    &[],
                    &[TargetQuote {
                        index: 0,
                        price_x64,
                        sqrt_price_x64,
                        base_qty: 10,
                    }],
                    false,
                ),
                Err(ReconcileError::InvalidTarget)
            ));
        }
    }

    #[test]
    fn maker_reconciliation_blocks_retries_and_invalid_intent() {
        let book = TwoPageBook::new(Pubkey::new_unique(), Pubkey::new_unique(), Side::Bid);
        let maker = Pubkey::new_unique();
        assert!(matches!(
            reconcile_maker(book, maker, &[], &[], true),
            Err(ReconcileError::UnconfirmedMutation)
        ));
        assert!(matches!(
            reconcile_maker(
                book,
                maker,
                &[],
                &[TargetQuote {
                    index: 2,
                    price_x64: Q64,
                    sqrt_price_x64: Q64,
                    base_qty: 10,
                }],
                false
            ),
            Err(ReconcileError::InvalidTarget)
        ));
    }
}
