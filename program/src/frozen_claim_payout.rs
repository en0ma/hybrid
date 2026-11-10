use super::*;

/// Opcode 40: authenticated, atomic CLAIM payout from an already trusted
/// frozen epoch. There is deliberately no public arbitrary-root sealer.
/// Only a future execution-derived sealing path may create epoch PDAs.
///
/// The entire instruction, including all state writes and the checked SPL
/// CPI, is one Solana transaction. Any failure MUST return Err, allowing
/// Solana's rollback to restore the claimed bitmap and all reserve balances.
///
/// data = exactly the opcode 38 claim payload with byte 0 replaced by 40.
/// accounts = [market, frozen epoch, maker balance, maker signer, custody,
///             market lazy reserve, payout vault, destination SPL account,
///             mint, vault authority PDA, SPL token program].
#[inline(never)]
pub(super) fn process_atomic_frozen_claim(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 11 || data.first() != Some(&40) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [market, epoch_account, balance_account, maker, custody_account, reserve_account, vault, destination, mint, vault_authority, token_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !epoch_account.is_writable
        || !balance_account.is_writable
        || !reserve_account.is_writable
        || !vault.is_writable
        || !destination.is_writable
        || market.is_writable
        || custody_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }
    if *token_program.key != TOKEN_PROGRAM_ID
        || *vault.key == *destination.key
        || *reserve_account.key == *epoch_account.key
        || *reserve_account.key == *balance_account.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // The same exact bounded proof/seat/owner checks as read-only opcode 38.
    // Decode the claim payload with its *preflight* discriminator; the
    // external claim opcode can never be silently accepted as preflight.
    let mut preflight_data = data.to_vec();
    preflight_data[0] = 38;
    let (seat, amounts) =
        frozen_claim_preflight::verify_claim(program_id, &accounts[..4], &preflight_data, true)?;

    let epoch_data = epoch_account.try_borrow_data()?;
    let epoch = hybrid_state::frozen_epoch::FrozenEpochView::decode(&epoch_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(epoch_data);
    let custody = load_custody(program_id, market, custody_account)?;
    let mut reserve = lazy_reserve::load_lazy_reserve(program_id, market, reserve_account)?;
    if reserve.genesis != epoch.header.genesis
        || epoch.header.epoch_id >= reserve.next_epoch_id
        || epoch.header.base_vault != custody.base_vault
        || epoch.header.quote_vault != custody.quote_vault
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // Existing maker collateral is debited exactly once, after the fill has
    // moved collateral to the taker during authenticated epoch execution.
    let mut balance = load_maker_balance(program_id, market, balance_account, maker)?;
    let (spent, payout, expected_vault, expected_mint, decimals) = if epoch.header.side == 0 {
        balance.locked_base = balance
            .locked_base
            .checked_sub(amounts[0])
            .ok_or(ProgramError::InsufficientFunds)?;
        (
            amounts[0],
            amounts[3],
            custody.quote_vault,
            custody.quote_mint,
            custody.quote_decimals,
        )
    } else {
        balance.locked_quote = balance
            .locked_quote
            .checked_sub(amounts[2])
            .ok_or(ProgramError::InsufficientFunds)?;
        (
            amounts[2],
            amounts[1],
            custody.base_vault,
            custody.base_mint,
            custody.base_decimals,
        )
    };
    if spent == 0
        || payout == 0
        || vault.key.to_bytes() != expected_vault
        || mint.key.to_bytes() != expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let (expected_authority, authority_bump) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *vault_authority.key != expected_authority {
        return Err(ProgramError::InvalidSeeds);
    }
    let (vault_mint, vault_owner) = token_account_fields(vault)?;
    let (destination_mint, destination_owner) = token_account_fields(destination)?;
    if vault_mint != expected_mint
        || destination_mint != expected_mint
        || vault_owner != expected_authority.to_bytes()
        || destination_owner != maker.key.to_bytes()
        || mint_decimals(mint)? != decimals
    {
        return Err(ProgramError::InvalidAccountData);
    }
    // Physical SPL amount is at the fixed classic SPL Token Account offset
    // (never rely solely on balances recorded by Hybrid's own accounts).
    let token_data = vault.try_borrow_data()?;
    let available = u64::from_le_bytes(
        token_data[64..72]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    drop(token_data);
    let (base_floor, quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let current_floor = if epoch.header.side == 0 {
        quote_floor
    } else {
        base_floor
    };
    if available < current_floor {
        return Err(ProgramError::InsufficientFunds);
    }
    // Simulate the complete successful post-CPI state before touching an
    // account or invoking SPL Token. A malicious member cannot draw another
    // epoch's reserves, even when this epoch has a valid Merkle root.
    reserve
        .release_claim(amounts)
        .map_err(|_| ProgramError::InsufficientFunds)?;
    let (next_base_floor, next_quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let next_floor = if epoch.header.side == 0 {
        next_quote_floor
    } else {
        next_base_floor
    };
    if available
        .checked_sub(payout)
        .ok_or(ProgramError::InsufficientFunds)?
        < next_floor
    {
        return Err(ProgramError::InsufficientFunds);
    }

    // These three writes and the checked SPL transfer succeed or fail as
    // one instruction. A failed CPI returns Err: Solana atomically restores
    // these writes, so no claim can be burned without receiving its payout.
    {
        let mut bytes = epoch_account.try_borrow_mut_data()?;
        epoch
            .record_claim(&mut bytes, seat, amounts)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    store_maker_balance(balance_account, &balance)?;
    {
        let mut bytes = reserve_account.try_borrow_mut_data()?;
        reserve
            .encode(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market.key.as_ref(), &bump];
    transfer_checked(
        vault,
        mint,
        destination,
        vault_authority,
        token_program,
        (payout, decimals),
        Some(seeds),
    )
}
