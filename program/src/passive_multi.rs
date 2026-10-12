//! Authenticated, funded multi-position passive settlement, one price segment.
//! Fee proceeds are compounded into each LP's current redeemable principal;
//! pool accrued-fee counters must be zero in this accounting mode.
use crate::{
    check_passive_vault_coverage, load_custody, token_account_fields, transfer_checked,
    TOKEN_PROGRAM_ID,
};
use hybrid_engine::{
    passive_exact_out::{buy_exact_out, sell_exact_out},
    quote_base_in_for_quote_out, quote_quote_in_for_base_out, PassiveState,
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
    if data.len() < 2 || data[0] != 37 || data[1] == 0 || data[1] > 8
        || data.len() != 2 + usize::from(data[1]) * 21
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    for segment in data[2..].chunks_exact(21) {
        process(program_id, accounts, segment)?;
    }
    Ok(())
}

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
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
    if input == 0 || output == 0 || bps == 0 || bps >= 10_000 {
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
        || pool.pool.accrued_base_fees != 0
        || pool.pool.accrued_quote_fees != 0
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
            base_fees: 0,
            quote_fees: 0,
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
    let segment_price = if buy {
        header.sqrt_price_x64
    } else {
        header.sqrt_price_x64.checked_sub(1)
            .ok_or(ProgramError::InvalidInstructionData)?
    };
    let mut active_liquidity = 0u128;
    for claim in &ledger.positions {
        if claim
            .range
            .active_at(segment_price)
            .map_err(|_| ProgramError::InvalidAccountData)?
        {
            active_liquidity = active_liquidity
                .checked_add(claim.range.liquidity)
                .ok_or(ProgramError::InvalidAccountData)?;
        }
    }
    if active_liquidity == 0 {
        return Err(ProgramError::InsufficientFunds);
    }
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    let state = PassiveState {
        sqrt_price_x64: header.sqrt_price_x64,
        liquidity: active_liquidity,
    };
    let (gross, fee, quote) = if exact_out {
        let first = if buy {
            buy_exact_out(state, output)
        } else {
            sell_exact_out(state, output)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        let gross = u64::try_from(
            (u128::from(first.amount_in) * 10_000).div_ceil(10_000 - u128::from(bps)),
        )
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        if gross > input {
            return Err(ProgramError::InvalidInstructionData);
        }
        let fee = u64::try_from((u128::from(gross) * u128::from(bps)).div_ceil(10_000))
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        let net = gross
            .checked_sub(fee)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let q = if buy {
            quote_quote_in_for_base_out(state, net)
        } else {
            quote_base_in_for_quote_out(state, net)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        (gross, fee, q)
    } else {
        let fee = u64::try_from((u128::from(input) * u128::from(bps)).div_ceil(10_000))
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        let net = input
            .checked_sub(fee)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let q = if buy {
            quote_quote_in_for_base_out(state, net)
        } else {
            quote_base_in_for_quote_out(state, net)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        (input, fee, q)
    };
    if fee == 0 || quote.amount_out < output || quote.amount_out == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    // No single segment may jump over a range boundary. Endpoint equality
    // is permitted: the next segment reselects the appropriate active LPs.
    let start_price = header.sqrt_price_x64;
    let end_price = quote.next_sqrt_price_x64;
    for claim in &ledger.positions {
        let lo = claim.range.lower_sqrt_price_x64;
        let hi = claim.range.upper_sqrt_price_x64;
        let crossed = if buy {
            end_price < start_price
                || (start_price < lo && lo < end_price)
                || (start_price < hi && hi < end_price)
        } else {
            end_price > start_price
                || (end_price < lo && lo < start_price)
                || (end_price < hi && hi < start_price)
        };
        if crossed {
            return Err(ProgramError::InvalidInstructionData);
        }
    }
    // Compound the fee into the per-position principal, so the legacy
    // authenticated close transfers current LP inventory including fees.
    let trade = if buy {
        SegmentTrade::Buy {
            quote_in: gross,
            base_out: quote.amount_out,
            quote_fee: fee,
        }
    } else {
        SegmentTrade::Sell {
            base_in: gross,
            quote_out: quote.amount_out,
            base_fee: fee,
        }
    };
    ledger
        .apply_segment(segment_price, trade)
        .map_err(|_| ProgramError::InsufficientFunds)?;
    for claim in &mut ledger.positions {
        claim.base = claim
            .base
            .checked_add(claim.base_fees)
            .ok_or(ProgramError::InvalidAccountData)?;
        claim.quote = claim
            .quote
            .checked_add(claim.quote_fees)
            .ok_or(ProgramError::InvalidAccountData)?;
        claim.base_fees = 0;
        claim.quote_fees = 0;
    }
    ledger.pool.base_reserve = ledger
        .pool
        .base_reserve
        .checked_add(ledger.pool.accrued_base_fees)
        .ok_or(ProgramError::InvalidAccountData)?;
    ledger.pool.quote_reserve = ledger
        .pool
        .quote_reserve
        .checked_add(ledger.pool.accrued_quote_fees)
        .ok_or(ProgramError::InvalidAccountData)?;
    ledger.pool.accrued_base_fees = 0;
    ledger.pool.accrued_quote_fees = 0;
    ledger
        .verify()
        .map_err(|_| ProgramError::InvalidAccountData)?;

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
            (quote.amount_out, custody.base_decimals),
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
            (quote.amount_out, custody.quote_decimals),
            Some(seeds),
        )?;
    }
    for (i, account) in positions.iter().enumerate() {
        states[i].position.base_principal = ledger.positions[i].base;
        states[i].position.quote_principal = ledger.positions[i].quote;
        states[i]
            .encode_into(&mut account.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    pool.pool = ledger.pool;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    header.sqrt_price_x64 = quote.next_sqrt_price_x64;
    header
        .encode_into(&mut market.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}
