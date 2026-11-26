//! Authenticated, funded multi-position passive settlement, one price segment.
//! LP fee claims remain segregated from current inventory.
use crate::{
    check_passive_vault_coverage, load_custody, token_account_fields, transfer_checked,
    TOKEN_PROGRAM_ID,
};
use hybrid_engine::{
    passive_exact_out::{buy_exact_out, sell_exact_out},
    passive_sell_to_target, passive_to_target, quote_base_in_for_quote_out,
    quote_quote_in_for_base_out, PassiveState,
};
use hybrid_state::{
    passive::{PoolAccount, PositionAccount},
    passive_segments::{RangeClaim, SegmentTrade, SegmentedPool},
};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

/// Opcode 35 exact-in / 36 exact-out. Data: opcode, side, input-or-max:u64,
/// minimum-output:u64, fee_bps:u16, number_of_positions:u8 (21 bytes).
/// Accounts: market, custody, pool, taker signer, taker base/quote, base/quote
/// vault, base/quote mint, vault authority, SPL Token program, then ALL
/// position PDAs in ascending Pubkey byte order, each writable.
/// Execute up to eight separately verified segments atomically. Any failed
/// leg causes Solana to roll back all earlier CPI transfers and state writes.
pub fn process_batch(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 2 || data[0] != 37 || data[1] == 0 || data[1] > 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let count = usize::from(data[1]);
    let positions = accounts
        .len()
        .checked_sub(12)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if !(1..=8).contains(&positions) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let guard_start = 2 + count * 21;
    if data.len() != guard_start + 24 + 8 * positions {
        return Err(ProgramError::InvalidInstructionData);
    }
    let guard = &data[guard_start..];
    for (i, segment) in data[2..guard_start].as_chunks::<21>().0.iter().enumerate() {
        process_segment(
            program_id,
            accounts,
            segment,
            if i == 0 { Some(guard) } else { None },
        )?;
    }
    Ok(())
}

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 21 || data.len() != 45 + 8 * usize::from(data[20]) {
        return Err(ProgramError::InvalidInstructionData);
    }
    process_segment(program_id, accounts, &data[..21], Some(&data[21..]))
}

fn process_segment(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
    guard: Option<&[u8]>,
) -> ProgramResult {
    if data.len() != 21
        || !matches!(data[0], 35 | 36)
        || data[1] > 1
        || data[20] == 0
        || data[20] > 8
        || accounts.len() != 12 + usize::from(data[20])
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let exact_out = data[0] == 36;
    let buy = data[1] == 0;
    let input = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let output = u64::from_le_bytes(
        data[10..18]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let bps = u16::from_le_bytes(
        data[18..20]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if input == 0 || output == 0 || bps != crate::PASSIVE_FEE_BPS {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let pool_account = &accounts[2];
    let taker = &accounts[3];
    let taker_base = &accounts[4];
    let taker_quote = &accounts[5];
    let base_vault = &accounts[6];
    let quote_vault = &accounts[7];
    let base_mint = &accounts[8];
    let quote_mint = &accounts[9];
    let authority = &accounts[10];
    let token_program = &accounts[11];
    let positions = &accounts[12..];
    if !taker.is_signer
        || !market.is_writable
        || !pool_account.is_writable
        || !taker_base.is_writable
        || !taker_quote.is_writable
        || !base_vault.is_writable
        || !quote_vault.is_writable
        || market.owner != program_id
        || pool_account.owner != program_id
        || positions
            .iter()
            .any(|a| a.owner != program_id || !a.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let custody = load_custody(program_id, market, custody_account)?;
    let mut header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !header.collateralized_active()
        || header.ask_count != 0
        || header.bid_count() != 0
        || header.boundary_count != 0
    {
        return Err(ProgramError::InvalidAccountData);
    }
    if let Some(snapshot) = guard {
        if header.sqrt_price_x64 != u128::from_le_bytes(snapshot[..16].try_into().unwrap())
            || header.next_sequence != u64::from_le_bytes(snapshot[16..24].try_into().unwrap())
        {
            return Err(ProgramError::InvalidInstructionData);
        }
    }
    let mut pool = PoolAccount::decode_from(&pool_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (pool_key, pool_bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    let (authority_key, authority_bump) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *pool_account.key != pool_key
        || pool.bump != pool_bump
        || pool.market != market.key.to_bytes()
        || *authority.key != authority_key
        || *token_program.key != TOKEN_PROGRAM_ID
        || *base_vault.key != Pubkey::new_from_array(custody.base_vault)
        || *quote_vault.key != Pubkey::new_from_array(custody.quote_vault)
        || *base_mint.key != Pubkey::new_from_array(custody.base_mint)
        || *quote_mint.key != Pubkey::new_from_array(custody.quote_mint)
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let all_keys = [
        taker_base.key,
        taker_quote.key,
        base_vault.key,
        quote_vault.key,
    ];
    for i in 0..4 {
        for j in (i + 1)..4 {
            if all_keys[i] == all_keys[j] {
                return Err(ProgramError::InvalidAccountData);
            }
        }
    }
    let (tb_mint, tb_owner) = token_account_fields(taker_base)?;
    let (tq_mint, tq_owner) = token_account_fields(taker_quote)?;
    let (bv_mint, bv_owner) = token_account_fields(base_vault)?;
    let (qv_mint, qv_owner) = token_account_fields(quote_vault)?;
    if tb_owner != taker.key.to_bytes()
        || tq_owner != taker.key.to_bytes()
        || tb_mint != custody.base_mint
        || tq_mint != custody.quote_mint
        || bv_mint != custody.base_mint
        || qv_mint != custody.quote_mint
        || bv_owner != authority_key.to_bytes()
        || qv_owner != authority_key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut states = Vec::with_capacity(positions.len());
    let mut claims = Vec::with_capacity(positions.len());
    for (i, account) in positions.iter().enumerate() {
        if i > 0 && positions[i - 1].key.as_ref() >= account.key.as_ref() {
            return Err(ProgramError::InvalidAccountData);
        }
        let state = PositionAccount::decode_from(&account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if let Some(snapshot) = guard {
            if state.revision
                != u64::from_le_bytes(snapshot[24 + i * 8..32 + i * 8].try_into().unwrap())
            {
                return Err(ProgramError::InvalidInstructionData);
            }
        }
        let owner = Pubkey::new_from_array(state.position.owner);
        let (expected, bump) = Pubkey::find_program_address(
            &[
                b"passive-position",
                market.key.as_ref(),
                owner.as_ref(),
                &state.nonce.to_le_bytes(),
            ],
            program_id,
        );
        if expected != *account.key || state.bump != bump || state.market != market.key.to_bytes() {
            return Err(ProgramError::InvalidSeeds);
        }
        claims.push(RangeClaim {
            range: hybrid_engine::range_position::RangePosition {
                lower_sqrt_price_x64: state.position.lower_sqrt_price_x64,
                upper_sqrt_price_x64: state.position.upper_sqrt_price_x64,
                liquidity: state.position.liquidity,
            },
            base: state.position.base_principal,
            quote: state.position.quote_principal,
            base_fees: state.base_fees,
            quote_fees: state.quote_fees,
        });
        states.push(state);
    }
    let mut ledger = SegmentedPool {
        pool: pool.pool,
        positions: claims,
    };
    ledger
        .verify()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    let mut price = header.sqrt_price_x64;
    let mut gross = 0u64;
    let mut total_out = 0u64;
    // A monotone route crosses at most two endpoints per supplied LP.
    for _ in 0..=2 * positions.len() {
        let remaining = input - gross;
        if remaining == 0 || (exact_out && total_out >= output) {
            break;
        }
        let segment_price = if buy {
            price
        } else {
            price
                .checked_sub(1)
                .ok_or(ProgramError::InvalidInstructionData)?
        };
        let mut liquidity = 0u128;
        let mut boundary: Option<u128> = None;
        for claim in &ledger.positions {
            if claim
                .range
                .active_at(segment_price)
                .map_err(|_| ProgramError::InvalidAccountData)?
            {
                liquidity = liquidity
                    .checked_add(claim.range.liquidity)
                    .ok_or(ProgramError::InvalidAccountData)?;
            }
            for edge in [
                claim.range.lower_sqrt_price_x64,
                claim.range.upper_sqrt_price_x64,
            ] {
                if buy && edge > price && boundary.is_none_or(|b| edge < b)
                    || !buy && edge < price && boundary.is_none_or(|b| edge > b)
                {
                    boundary = Some(edge);
                }
            }
        }
        if liquidity == 0 {
            return Err(ProgramError::InsufficientFunds);
        }
        let state = PassiveState {
            sqrt_price_x64: price,
            liquidity,
        };
        let fee_for = |amount: u64| -> u64 {
            ((u128::from(amount) * u128::from(bps)).div_ceil(10_000)) as u64
        };
        let gross_for = |net: u64| -> Result<u64, ProgramError> {
            u64::try_from((u128::from(net) * 10_000).div_ceil(10_000 - u128::from(bps)))
                .map_err(|_| ProgramError::InvalidInstructionData)
        };
        let net = remaining - fee_for(remaining);
        let mut leg_gross = remaining;
        // Compute the boundary cost before attempting an unbounded quote.
        // If affordable, stop exactly there, not one rounded atom beyond.
        let boundary_quote = boundary.and_then(|edge| {
            if buy {
                passive_to_target(state, edge).ok()
            } else {
                passive_sell_to_target(state, edge).ok()
            }
        });
        let mut quote = if let Some(target) = boundary_quote.filter(|q| q.amount_in <= net) {
            leg_gross = gross_for(target.amount_in)?;
            target
        } else {
            if buy {
                quote_quote_in_for_base_out(state, net)
            } else {
                quote_base_in_for_quote_out(state, net)
            }
            .map_err(|_| ProgramError::InvalidInstructionData)?
        };
        if exact_out && quote.amount_out >= output - total_out {
            let target = output - total_out;
            // A boundary payment is already the conservative quote at that edge.
            // Use the ordinary exact-out solver only if it stays in this segment.
            let exact = if buy {
                buy_exact_out(state, target)
            } else {
                sell_exact_out(state, target)
            };
            if let Ok(exact) = exact {
                let cost = gross_for(exact.amount_in)?;
                if cost <= leg_gross
                    && boundary.is_none_or(|edge| {
                        if buy {
                            exact.next_sqrt_price_x64 <= edge
                        } else {
                            exact.next_sqrt_price_x64 >= edge
                        }
                    })
                {
                    quote = exact;
                    leg_gross = cost;
                }
            }
        }
        if let Some(edge) = boundary {
            if buy && quote.next_sqrt_price_x64 > edge || !buy && quote.next_sqrt_price_x64 < edge {
                return Err(ProgramError::InvalidInstructionData);
            }
        }
        let fee = fee_for(leg_gross);
        if leg_gross == 0 || fee == 0 || quote.amount_out == 0 || quote.next_sqrt_price_x64 == price
        {
            return Err(ProgramError::InvalidInstructionData);
        }
        let trade = if buy {
            SegmentTrade::Buy {
                quote_in: leg_gross,
                base_out: quote.amount_out,
                quote_fee: fee,
            }
        } else {
            SegmentTrade::Sell {
                base_in: leg_gross,
                quote_out: quote.amount_out,
                base_fee: fee,
            }
        };
        ledger
            .apply_segment(segment_price, trade)
            .map_err(|_| ProgramError::InsufficientFunds)?;
        gross = gross
            .checked_add(leg_gross)
            .ok_or(ProgramError::InvalidInstructionData)?;
        total_out = total_out
            .checked_add(quote.amount_out)
            .ok_or(ProgramError::InvalidInstructionData)?;
        price = quote.next_sqrt_price_x64;
    }
    if total_out < output || (!exact_out && gross != input) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market.key.as_ref(), &bump];
    if buy {
        transfer_checked(
            taker_quote,
            quote_mint,
            quote_vault,
            taker,
            token_program,
            (gross, custody.quote_decimals),
            None,
        )?;
        transfer_checked(
            base_vault,
            base_mint,
            taker_base,
            authority,
            token_program,
            (total_out, custody.base_decimals),
            Some(seeds),
        )?;
    } else {
        transfer_checked(
            taker_base,
            base_mint,
            base_vault,
            taker,
            token_program,
            (gross, custody.base_decimals),
            None,
        )?;
        transfer_checked(
            quote_vault,
            quote_mint,
            taker_quote,
            authority,
            token_program,
            (total_out, custody.quote_decimals),
            Some(seeds),
        )?;
    }
    for (i, account) in positions.iter().enumerate() {
        states[i].position.base_principal = ledger.positions[i].base;
        states[i].position.quote_principal = ledger.positions[i].quote;
        states[i].base_fees = ledger.positions[i].base_fees;
        states[i].quote_fees = ledger.positions[i].quote_fees;
        states[i].revision = states[i]
            .revision
            .checked_add(1)
            .ok_or(ProgramError::InvalidAccountData)?;
        states[i]
            .encode_into(&mut account.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    pool.pool = ledger.pool;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    header.sqrt_price_x64 = price;
    header
        .encode_into(&mut market.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}
