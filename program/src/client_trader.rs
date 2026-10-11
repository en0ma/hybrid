//! Host-only executable trader plan, restricted to active CLOB liquidity.
//! This does not advertise passive reserves as executable swap liquidity.
use crate::client::{custody, maker_balance, BuildError, LinkedSwap, Side};
use crate::client_rpc::{MarketRpcSnapshot, SnapshotError};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraderError {
    StaleOrInvalidBook(SnapshotError),
    WrongMarketCustody,
    MakerFanout,
    DuplicateMaker,
    MissingLiquidity,
    InvalidSlippage,
    Build(BuildError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionLimit {
    pub expected_amount: u64,
    pub slippage_bps: u16,
}

impl ExecutionLimit {
    /// For exact-in swaps, calculate minimum output by flooring.
    /// Any output rounding remainder is forfeited rather than overstated.
    pub fn minimum_out(self) -> Result<u64, TraderError> {
        if self.expected_amount == 0 || self.slippage_bps > 10_000 {
            return Err(TraderError::InvalidSlippage);
        }
        let numerator = u128::from(self.expected_amount)
            * u128::from(10_000 - self.slippage_bps);
        Ok((numerator / 10_000) as u64)
    }

    /// For exact-out swaps, calculate maximum input by ceiling.
    pub fn maximum_in(self) -> Result<u64, TraderError> {
        if self.expected_amount == 0 || self.slippage_bps > 10_000 {
            return Err(TraderError::InvalidSlippage);
        }
        let numerator =
            u128::from(self.expected_amount) * u128::from(10_000 + self.slippage_bps);
        u64::try_from(numerator.div_ceil(10_000))
            .map_err(|_| TraderError::InvalidSlippage)
    }
}

pub struct TraderRequest<'a> {
    pub snapshot: &'a MarketRpcSnapshot,
    pub market: Pubkey,
    pub taker: Pubkey,
    pub taker_base: Pubkey,
    pub taker_quote: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub vault_authority: Pubkey,
    pub token_program: Pubkey,
    /// Ordered makers touched by the proposed active-only quote.
    pub maker_owners: &'a [Pubkey],
    pub amount: u64,
    pub limit: u64,
    pub exact_out: bool,
    pub min_slot: u64,
}

pub struct TraderPlan {
    pub snapshot_slot: u64,
    pub instruction: Instruction,
    pub required_maker_balances: Vec<Pubkey>,
}

/// Return a typed active-only swap instruction after authenticated book audit.
/// A consumer must additionally simulate with actual RPC accounts, verify
/// slippage and expiry, then revalidate before broadcasting.
pub fn plan_active_swap(request: TraderRequest<'_>) -> Result<TraderPlan, TraderError> {
    let report = request
        .snapshot
        .audit(request.min_slot)
        .map_err(TraderError::StaleOrInvalidBook)?;
    if request.snapshot.market.key != request.market {
        return Err(TraderError::WrongMarketCustody);
    }
    if report.total_orders == 0 || request.maker_owners.is_empty() {
        return Err(TraderError::MissingLiquidity);
    }
    if request.maker_owners.len() > 8 {
        return Err(TraderError::MakerFanout);
    }
    if request
        .maker_owners
        .iter()
        .enumerate()
        .any(|(i, maker)| request.maker_owners[i + 1..].contains(maker))
    {
        return Err(TraderError::DuplicateMaker);
    }
    let maker_balances: Vec<_> = request
        .maker_owners
        .iter()
        .map(|owner| maker_balance(request.market, *owner))
        .collect();
    let pages = if report.is_linked {
        vec![
            request.snapshot.snapshot().expected_addresses().0,
            request.snapshot.snapshot().expected_addresses().1,
        ]
    } else {
        vec![request.snapshot.snapshot().expected_addresses().0]
    };
    let builder = LinkedSwap {
        market: request.market,
        custody: custody(request.market),
        taker: request.taker,
        taker_base: request.taker_base,
        taker_quote: request.taker_quote,
        base_vault: request.base_vault,
        quote_vault: request.quote_vault,
        vault_authority: request.vault_authority,
        token_program: request.token_program,
        pages,
        maker_balances: maker_balances.clone(),
    };
    let instruction = builder
        .instruction(
            request.snapshot.side,
            request.exact_out,
            request.amount,
            request.limit,
        )
        .map_err(TraderError::Build)?;
    Ok(TraderPlan {
        snapshot_slot: request.snapshot.slot,
        instruction,
        required_maker_balances: maker_balances,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trader_slippage_bounds_round_in_conservative_direction() {
        assert_eq!(
            ExecutionLimit {
                expected_amount: 101,
                slippage_bps: 100,
            }
            .minimum_out(),
            Ok(99)
        );
        assert_eq!(
            ExecutionLimit {
                expected_amount: 101,
                slippage_bps: 100,
            }
            .maximum_in(),
            Ok(103)
        );
        assert_eq!(
            ExecutionLimit {
                expected_amount: 1,
                slippage_bps: 10_001,
            }
            .minimum_out(),
            Err(TraderError::InvalidSlippage)
        );
    }
}
