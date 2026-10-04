#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::AccountInfo, declare_id, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

declare_id!("US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx");

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    match data.first().copied() {
        Some(0) => Ok(()),
        Some(1) => {
            let state = hybrid_engine::PassiveState {
                sqrt_price_x64: hybrid_engine::Q64,
                liquidity: 1_000_000,
            };
            hybrid_engine::quote_quote_in_for_base_out(state, 10_000)
                .map(|_| ())
                .map_err(|_| ProgramError::InvalidInstructionData)
        }
        Some(2) => {
            let market = hybrid_engine::HybridMarket {
                passive: hybrid_engine::PassiveState {
                    sqrt_price_x64: hybrid_engine::Q64,
                    liquidity: 1_000_000,
                },
                best_ask: Some(hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::Q64,
                    sqrt_price_x64: hybrid_engine::Q64,
                    base_qty: 5_000,
                }),
            };
            hybrid_engine::quote_buy_exact_in(market, 10_000)
                .map(|_| ())
                .map_err(|_| ProgramError::InvalidInstructionData)
        }
        Some(3) => {
            let step = hybrid_engine::Q64 / 200;
            let ask_1_sqrt = hybrid_engine::Q64;
            let ask_2_sqrt = hybrid_engine::Q64 + step * 2;
            let asks = [
                hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::spot_price_x64(ask_1_sqrt)
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                    sqrt_price_x64: ask_1_sqrt,
                    base_qty: 1_000,
                },
                hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::spot_price_x64(ask_2_sqrt)
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                    sqrt_price_x64: ask_2_sqrt,
                    base_qty: 2_000,
                },
            ];
            let boundaries = [
                hybrid_engine::PassiveBoundary {
                    sqrt_price_x64: hybrid_engine::Q64 + step,
                    liquidity_after: 1_500_000,
                },
                hybrid_engine::PassiveBoundary {
                    sqrt_price_x64: hybrid_engine::Q64 + step * 3,
                    liquidity_after: 2_000_000,
                },
            ];
            hybrid_engine::quote_buy_exact_in_levels(
                hybrid_engine::PassiveState {
                    sqrt_price_x64: hybrid_engine::Q64,
                    liquidity: 1_000_000,
                },
                &asks,
                &boundaries,
                20_000,
            )
            .map(|_| ())
            .map_err(|_| ProgramError::InvalidInstructionData)
        }

        Some(4) => {
            if _accounts.len() != 3 {
                return Err(ProgramError::NotEnoughAccountKeys);
            }

            let market_data = _accounts[0]
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            let ask_data = _accounts[1]
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;
            let boundary_data = _accounts[2]
                .try_borrow_data()
                .map_err(|_| ProgramError::AccountBorrowFailed)?;

            let market = hybrid_state::MarketHeader::decode_from(&market_data)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let asks_page = hybrid_state::AskPage::decode_from(&ask_data)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let boundaries_page = hybrid_state::BoundaryPage::decode_from(&boundary_data)
                .map_err(|_| ProgramError::InvalidAccountData)?;

            hybrid_state::validate_ask_chain(core::slice::from_ref(&asks_page))
                .map_err(|_| ProgramError::InvalidAccountData)?;
            hybrid_state::validate_boundary_chain(core::slice::from_ref(&boundaries_page))
                .map_err(|_| ProgramError::InvalidAccountData)?;

            let mut asks = [hybrid_engine::LimitAsk {
                price_x64: 0,
                sqrt_price_x64: 0,
                base_qty: 0,
            }; hybrid_state::ASKS_PER_PAGE];
            for (index, entry) in asks_page.as_slice().iter().enumerate() {
                asks[index] = entry.as_limit_ask();
            }

            let mut boundaries = [hybrid_engine::PassiveBoundary {
                sqrt_price_x64: 0,
                liquidity_after: 0,
            }; hybrid_state::BOUNDARIES_PER_PAGE];
            for (index, entry) in boundaries_page.as_slice().iter().enumerate() {
                boundaries[index] = entry.as_passive_boundary();
            }

            hybrid_engine::quote_buy_exact_in_levels(
                hybrid_engine::PassiveState {
                    sqrt_price_x64: market.sqrt_price_x64,
                    liquidity: market.liquidity,
                },
                &asks[..asks_page.len()],
                &boundaries[..boundaries_page.len()],
                20_000,
            )
            .map(|_| ())
            .map_err(|_| ProgramError::InvalidInstructionData)
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
