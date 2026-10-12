//! Guarded single-position passive SPL settlement. No multi-LP settlement is
//! permitted: fees are wholly attributable to the sole authenticated LP.
//! Opcode 33: side:u8 (0 buy, 1 sell), gross_in:u64, min_out:u64,
//! fee_bps:u16, position_nonce:u64. Exactly 28 bytes.
use crate::{
    check_passive_vault_coverage, checked_token_amount, load_custody, token_account_fields,
    transfer_checked, TOKEN_PROGRAM_ID,
};
use hybrid_engine::{
    passive_exact_out::{buy_exact_out, sell_exact_out}, quote_base_in_for_quote_out,
    quote_quote_in_for_base_out, PassiveState,
};
use hybrid_state::passive::{PoolAccount, PositionAccount};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 28 || accounts.len() != 14 || !matches!(data[0], 33 | 34) || data[1] > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let exact_out = data[0] == 34;
    let side = data[1];
    let specified_in = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let min_out = u64::from_le_bytes(
        data[10..18]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let fee_bps = u16::from_le_bytes(
        data[18..20]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let nonce_bytes: [u8; 8] = data[20..28]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    if specified_in == 0 || min_out == 0 || fee_bps == 0 || fee_bps >= 10_000 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market_account = &accounts[0];
    let custody_account = &accounts[1];
    let pool_account = &accounts[2];
    let position_account = &accounts[3];
    let lp = &accounts[4];
    let taker = &accounts[5];
    let taker_base = &accounts[6];
    let taker_quote = &accounts[7];
    let base_vault = &accounts[8];
    let quote_vault = &accounts[9];
    let base_mint = &accounts[10];
    let quote_mint = &accounts[11];
    let authority = &accounts[12];
    let token_program = &accounts[13];
    if !taker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market_account.owner != program_id
        || pool_account.owner != program_id
        || position_account.owner != program_id
        || !market_account.is_writable
        || !pool_account.is_writable
        || !position_account.is_writable
        || !taker_base.is_writable
        || !taker_quote.is_writable
        || !base_vault.is_writable
        || !quote_vault.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let custody = load_custody(program_id, market_account, custody_account)?;
    let mut header = hybrid_state::MarketHeader::decode_from(&market_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    // Isolation until price-time priority against active CLOB and multi-range
    // crossing is implemented and independently security reviewed.
    if !header.collateralized_active()
        || header.ask_count != 0
        || header.bid_count() != 0
        || header.boundary_count != 0
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut pool = PoolAccount::decode_from(&pool_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let mut position = PositionAccount::decode_from(&position_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (pool_key, pool_bump) =
        Pubkey::find_program_address(&[b"passive-pool", market_account.key.as_ref()], program_id);
    let (position_key, position_bump) = Pubkey::find_program_address(
        &[
            b"passive-position",
            market_account.key.as_ref(),
            lp.key.as_ref(),
            &nonce_bytes,
        ],
        program_id,
    );
    let (authority_key, authority_bump) = Pubkey::find_program_address(
        &[b"vault-authority", market_account.key.as_ref()],
        program_id,
    );
    if *pool_account.key != pool_key
        || pool.bump != pool_bump
        || pool.market != market_account.key.to_bytes()
        || *position_account.key != position_key
        || position.bump != position_bump
        || position.market != market_account.key.to_bytes()
        || position.nonce != u64::from_le_bytes(nonce_bytes)
        || position.position.owner != lp.key.to_bytes()
        || *authority.key != authority_key
        || *token_program.key != TOKEN_PROGRAM_ID
        || *base_vault.key != Pubkey::new_from_array(custody.base_vault)
        || *quote_vault.key != Pubkey::new_from_array(custody.quote_vault)
        || *base_mint.key != Pubkey::new_from_array(custody.base_mint)
        || *quote_mint.key != Pubkey::new_from_array(custody.quote_mint)
    {
        return Err(ProgramError::InvalidSeeds);
    }
    // The existing position format has no per-position fee counter. Only
    // one-LP pools can safely allocate pool fees without double claiming.
    if pool.pool.total_liquidity != position.position.liquidity
        || pool.pool.base_reserve != position.position.base_principal
        || pool.pool.quote_reserve != position.position.quote_principal
        || pool.pool.total_liquidity == 0
    {
        return Err(ProgramError::InvalidAccountData);
    }
    if [
        taker_base.key,
        taker_quote.key,
        base_vault.key,
        quote_vault.key,
    ]
    .iter()
    .enumerate()
    .any(|(i, key)| {
        [
            taker_base.key,
            taker_quote.key,
            base_vault.key,
            quote_vault.key,
        ][i + 1..]
            .contains(key)
    }) {
        return Err(ProgramError::InvalidAccountData);
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
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    let state = PassiveState {
        sqrt_price_x64: header.sqrt_price_x64,
        liquidity: position.position.liquidity,
    };
    // Opcode 34 interprets specified_in as max gross input, and min_out
    // as requested minimum output. In both modes, ceil fees are charged on
    // the actual gross token debit, not on the post-fee quote amount.
    let (gross_in, fee, net_in, result) = if exact_out {
        let target = if side == 0 {
            buy_exact_out(state, min_out)
        } else {
            sell_exact_out(state, min_out)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        let net = target.amount_in;
        let denominator = 10_000u128 - u128::from(fee_bps);
        let gross = (u128::from(net) * 10_000u128)
            .div_ceil(denominator);
        let gross = u64::try_from(gross)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if gross > specified_in {
            return Err(ProgramError::InvalidInstructionData);
        }
        let fee = u64::try_from(
            (u128::from(gross) * u128::from(fee_bps)).div_ceil(10_000),
        )
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        let actual_net = gross.checked_sub(fee)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let result = if side == 0 {
            quote_quote_in_for_base_out(state, actual_net)
        } else {
            quote_base_in_for_quote_out(state, actual_net)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        (gross, fee, actual_net, result)
    } else {
        let gross = specified_in;
        let fee = u64::try_from(
            (u128::from(gross) * u128::from(fee_bps)).div_ceil(10_000),
        )
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        let net = gross.checked_sub(fee)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let result = if side == 0 {
            quote_quote_in_for_base_out(state, net)
        } else {
            quote_base_in_for_quote_out(state, net)
        }
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        (gross, fee, net, result)
    };
    if net_in == 0 || fee == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if result.amount_out < min_out
        || result.amount_out == 0
        || result.next_sqrt_price_x64 < position.position.lower_sqrt_price_x64
        || result.next_sqrt_price_x64 >= position.position.upper_sqrt_price_x64
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market_account.key.as_ref(), &bump];
    if side == 0 {
        pool.pool
            .buy(gross_in, result.amount_out, fee)
            .map_err(|_| ProgramError::InsufficientFunds)?;
        position.position.base_principal = position
            .position
            .base_principal
            .checked_sub(result.amount_out)
            .ok_or(ProgramError::InsufficientFunds)?;
        position.position.quote_principal = position
            .position
            .quote_principal
            .checked_add(net_in)
            .ok_or(ProgramError::InvalidAccountData)?;
        transfer_checked(
            taker_quote,
            quote_mint,
            quote_vault,
            taker,
            token_program,
            (gross_in, custody.quote_decimals),
            None,
        )?;
        transfer_checked(
            base_vault,
            base_mint,
            taker_base,
            authority,
            token_program,
            (result.amount_out, custody.base_decimals),
            Some(seeds),
        )?;
    } else {
        pool.pool
            .sell(gross_in, result.amount_out, fee)
            .map_err(|_| ProgramError::InsufficientFunds)?;
        position.position.base_principal = position
            .position
            .base_principal
            .checked_add(net_in)
            .ok_or(ProgramError::InvalidAccountData)?;
        position.position.quote_principal = position
            .position
            .quote_principal
            .checked_sub(result.amount_out)
            .ok_or(ProgramError::InsufficientFunds)?;
        transfer_checked(
            taker_base,
            base_mint,
            base_vault,
            taker,
            token_program,
            (gross_in, custody.base_decimals),
            None,
        )?;
        transfer_checked(
            quote_vault,
            quote_mint,
            taker_quote,
            authority,
            token_program,
            (result.amount_out, custody.quote_decimals),
            Some(seeds),
        )?;
    }
    header.sqrt_price_x64 = result.next_sqrt_price_x64;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    header
        .encode_into(&mut market_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    position
        .encode_into(&mut position_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let _ = checked_token_amount(base_vault)?;
    Ok(())
}
