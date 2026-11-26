use super::*;

/// Opcode 41: atomic one-tick ASK pro-rata execution and on-chain epoch seal.
/// This is deliberately narrow: only a single canonical ask page, at most 8
/// distinct maker seats, all resting at the same authenticated Q64 price.
/// No sealer-supplied root or allocation is trusted: every leaf and the root
/// are derived from program-owned resting orders and the paid taker fill.
///
/// Data [41, filled_base:u64LE, max_quote_in:u64LE].
/// Accounts: market, custody, lazy reserve, fresh frozen epoch PDA,
///           ask page 0, ask owner sidecar, taker signer, taker quote SPL,
///           taker base SPL, quote vault, base vault, quote mint, base mint,
///           vault authority PDA, SPL token program, payer, system program.
///
/// At execution, maker balances are deliberately untouched; historical
/// locked collateral is debited only once when its maker later claims.
/// Taker quote proceeds and fees become segregated on-chain liabilities.
const MAX_LAZY_SEATS: usize = 8;

// Use the same canonical floor-sqrt interval accepted by order placement.
fn validate_resting_ask_price(price_x64: u128, sqrt_price_x64: u128) -> ProgramResult {
    hybrid_engine::validate_limit_ask(hybrid_engine::LimitAsk {
        price_x64,
        sqrt_price_x64,
        base_qty: 1,
    })
    .map_err(|_| ProgramError::InvalidAccountData)
}

fn epoch_commitment(
    market: &Pubkey,
    program: &Pubkey,
    genesis: &[u8; 32],
    epoch: (u64, u128),
    owners: &[[u8; 32]],
    weights: &[u64],
    totals: [u64; 6],
) -> Result<[u8; 32], ProgramError> {
    if weights.len() > MAX_LAZY_SEATS {
        return Err(ProgramError::InvalidAccountData);
    }
    lazy_bid_execute::shared_epoch_commitment(market, program, genesis, epoch, owners, weights, totals, 0)
}

fn sealed_claim_root(
    epoch_commitment: &[u8; 32],
    owners: &[[u8; 32]],
    weights: &[u64],
    allocations: &[hybrid_state::lazy_pro_rata::MakerAllocation],
) -> Result<[u8; 32], ProgramError> {
    if allocations.len() > MAX_LAZY_SEATS {
        return Err(ProgramError::InvalidAccountData);
    }
    let claims: Vec<_> = allocations
        .iter()
        .map(|a| [a.base_fill, 0, 0, a.quote_proceeds, 0, a.quote_fee])
        .collect();
    lazy_bid_execute::shared_claim_root(epoch_commitment, owners, weights, &claims)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn build_ask_epoch_header(
    program_id: &Pubkey,
    market: &Pubkey,
    epoch_key: &Pubkey,
    custody: &hybrid_state::CustodyState,
    genesis: [u8; 32],
    epoch_id: u64,
    tick_sqrt_price_x64: u128,
    owners: &[[u8; 32]],
    weights: &[u64],
    allocations: &[hybrid_state::lazy_pro_rata::MakerAllocation],
    totals: [u64; 6],
) -> Result<hybrid_state::frozen_epoch::FrozenEpochHeader, ProgramError> {
    let digest = epoch_commitment(
        market,
        program_id,
        &genesis,
        (epoch_id, tick_sqrt_price_x64),
        owners,
        weights,
        totals,
    )?;
    let root = sealed_claim_root(&digest, owners, weights, allocations)?;
    let id_bytes = epoch_id.to_le_bytes();
    let (expected_epoch, bump) =
        Pubkey::find_program_address(&[b"frozen-epoch", market.as_ref(), &id_bytes], program_id);
    if *epoch_key != expected_epoch {
        return Err(ProgramError::InvalidSeeds);
    }
    let header = hybrid_state::frozen_epoch::FrozenEpochHeader {
        bump,
        side: 0,
        market: market.to_bytes(),
        authority: market.to_bytes(),
        program: program_id.to_bytes(),
        genesis,
        epoch_id,
        tick_sqrt_price_x64,
        commitment: digest,
        claim_root: root,
        maker_count: owners.len() as u32,
        totals,
        base_vault: custody.base_vault,
        quote_vault: custody.quote_vault,
    };
    header
        .validate()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(header)
}

#[inline(never)]
pub(super) fn execute_and_seal_ask(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 17 || data[0] != 41 || accounts.len() != 17 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [market_account, custody_account, reserve_account, epoch_account, ask_account, owner_account, taker, taker_quote, taker_base, quote_vault, base_vault, quote_mint, base_mint, vault_authority, token_program, payer, system_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let base_fill = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let max_quote_in = u64::from_le_bytes(
        data[9..17]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if base_fill == 0 || max_quote_in == 0 || !taker.is_signer || !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market_account.owner != program_id
        || ask_account.owner != program_id
        || owner_account.owner != program_id
        || *token_program.key != TOKEN_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !market_account.is_writable
        || !custody_account.is_writable
        || !reserve_account.is_writable
        || !epoch_account.is_writable
        || !ask_account.is_writable
        || !owner_account.is_writable
        || !taker_quote.is_writable
        || !taker_base.is_writable
        || !quote_vault.is_writable
        || !base_vault.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut market = hybrid_state::MarketHeader::decode_from(&market_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market.collateralized_active() || market.ask_count == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    let page_id = 0u32.to_le_bytes();
    let (expected_page, _) = Pubkey::find_program_address(
        &[b"ask-page", market_account.key.as_ref(), &page_id],
        program_id,
    );
    if *ask_account.key != expected_page || market.reserved2 != owner_account.key.to_bytes() {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut asks = load_active_ask_page(ask_account)?;
    let mut owner_page = load_owner_page_box(owner_account)?;
    owner_page
        .validate_parallel(&asks)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let links = asks.links();
    if links.page_index != 0
        || links.prev_page.is_some()
        || links.next_page.is_some()
        || asks.len() != market.ask_count as usize
        || asks.is_empty()
        || asks.len() > MAX_LAZY_SEATS
    {
        return Err(ProgramError::InvalidAccountData);
    }
    asks.validate_order()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let resting_tick = asks.as_slice()[0].sqrt_price_x64;
    let resting_price = asks.as_slice()[0].price_x64;
    if resting_tick == 0 || resting_price == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    let weights: Vec<u64> = asks.as_slice().iter().map(|entry| entry.base_qty).collect();
    let owners: Vec<[u8; 32]> = owner_page.as_slice().to_vec();
    for (index, entry) in asks.as_slice().iter().enumerate() {
        if entry.sqrt_price_x64 != resting_tick
            || entry.price_x64 != resting_price
            || owners[..index].contains(&owners[index])
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    // Resting orders are admitted using the canonical floor-sqrt interval,
    // not exact equality with the truncated Q64 square. Keep settlement
    // consistent with the limit-ask placement and engine validation rules.
    validate_resting_ask_price(resting_price, resting_tick)?;

    let quote_proceeds = hybrid_engine::quote_for_base_at_price(base_fill, resting_price)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    if quote_proceeds == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let quote_fee =
        ((u128::from(quote_proceeds) * u128::from(PASSIVE_FEE_BPS)) / 10_000u128) as u64;
    let quote_in = quote_proceeds
        .checked_add(quote_fee)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if quote_in > max_quote_in {
        return Err(ProgramError::InsufficientFunds);
    }
    let allocations =
        hybrid_state::lazy_pro_rata::allocate_ask(&weights, base_fill, quote_proceeds, quote_fee)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
    // Fail closed when any positive fill produces no maker proceeds.
    if allocations
        .iter()
        .any(|claim| claim.base_fill > 0 && claim.quote_proceeds == 0)
    {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut custody = load_custody(program_id, market_account, custody_account)?;
    let mut reserve = lazy_reserve::load_lazy_reserve(program_id, market_account, reserve_account)?;
    let authority_bump = lazy_bid_execute::validate_bid_custody_token_accounts(
        program_id,
        market_account,
        &custody,
        taker,
        taker_base,
        taker_quote,
        base_vault,
        quote_vault,
        base_mint,
        quote_mint,
        vault_authority,
    )?;

    // Reconcile vault ownership and ALL historical liabilities before
    // creating the next epoch, not merely the new epoch's Merkle root.
    let (floor_base, floor_quote) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if lazy_bid_execute::token_amount(base_vault)? < floor_base
        || lazy_bid_execute::token_amount(quote_vault)? < floor_quote
        || custody.total_base < base_fill
    {
        return Err(ProgramError::InsufficientFunds);
    }
    let epoch_id = reserve.next_epoch_id;
    let totals = [base_fill, 0, 0, quote_proceeds, 0, quote_fee];
    let header = build_ask_epoch_header(
        program_id,
        market_account.key,
        epoch_account.key,
        &custody,
        reserve.genesis,
        epoch_id,
        resting_tick,
        &owners,
        &weights,
        &allocations,
        totals,
    )?;
    let id_bytes = epoch_id.to_le_bytes();
    let epoch_bump = header.bump;

    // Precalculate every state transition before the first token CPI.
    let fills: Vec<u64> = allocations.iter().map(|claim| claim.base_fill).collect();
    let removed = hybrid_state::apply_lazy_ask_fills(&mut asks, &mut owner_page, &fills)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    market.ask_count = market
        .ask_count
        .checked_sub(removed as u32)
        .ok_or(ProgramError::InvalidAccountData)?;
    custody.total_base = custody
        .total_base
        .checked_sub(base_fill)
        .ok_or(ProgramError::InsufficientFunds)?;
    reserve
        .reserve_execution(epoch_id, totals)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (next_floor_base, next_floor_quote) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if lazy_bid_execute::token_amount(base_vault)?
        .checked_sub(base_fill)
        .ok_or(ProgramError::InsufficientFunds)?
        < next_floor_base
        || lazy_bid_execute::token_amount(quote_vault)?
            .checked_add(quote_in)
            .ok_or(ProgramError::InvalidAccountData)?
            < next_floor_quote
    {
        return Err(ProgramError::InsufficientFunds);
    }

    // First initialize the canonical epoch PDA, then atomically perform
    // both checked SPL CPIs and commit all bookkeeping. Any failed CPI
    // reverts the account creation and ALL account and token changes.
    create_program_pda(
        program_id,
        payer,
        epoch_account,
        system_program,
        &[b"frozen-epoch", market_account.key.as_ref(), &id_bytes],
        epoch_bump,
        hybrid_state::frozen_epoch::FROZEN_EPOCH_BYTES,
    )?;
    {
        let mut bytes = epoch_account.try_borrow_mut_data()?;
        header
            .initialize(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    transfer_checked(
        taker_quote,
        quote_mint,
        quote_vault,
        taker,
        token_program,
        (quote_in, custody.quote_decimals),
        None,
    )?;
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market_account.key.as_ref(), &bump];
    transfer_checked(
        base_vault,
        base_mint,
        taker_base,
        vault_authority,
        token_program,
        (base_fill, custody.base_decimals),
        Some(seeds),
    )?;
    {
        let mut bytes = market_account.try_borrow_mut_data()?;
        market
            .encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut bytes = ask_account.try_borrow_mut_data()?;
        asks.encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut bytes = owner_account.try_borrow_mut_data()?;
        owner_page
            .encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    store_custody(custody_account, &custody)?;
    {
        let mut bytes = reserve_account.try_borrow_mut_data()?;
        reserve
            .encode(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn resting_ask_accepts_canonical_floor_sqrt_with_rounding_gap() {
        let price = 4 * hybrid_engine::Q64 - 1;
        let sqrt = 2 * hybrid_engine::Q64 - 1;
        assert!(validate_resting_ask_price(price, sqrt).is_ok());
        // Noncanonical cached roots must still be rejected.
        assert!(validate_resting_ask_price(price, 2 * hybrid_engine::Q64).is_err());
        assert!(validate_resting_ask_price(price, sqrt - 1).is_err());
    }

    #[test]
    fn on_chain_epoch_root_matches_independent_sdk_vectors() {
        let owners = [[5u8; 32], [6u8; 32]];
        let weights = [100u64, 200u64];
        let totals = [150u64, 0, 0, 150, 0, 1];
        let epoch = epoch_commitment(
            &Pubkey::new_from_array([1; 32]),
            &Pubkey::new_from_array([2; 32]),
            &[3; 32],
            (0, 1u128 << 64),
            &owners,
            &weights,
            totals,
        )
        .unwrap();
        assert_eq!(
            hex(epoch),
            "5f36a8ed5802bab492090c2811ef96aa10e5a129c486d3d2a5ca99ad5ea677c7"
        );
        let allocations = hybrid_state::lazy_pro_rata::allocate_ask(&weights, 150, 150, 1).unwrap();
        let root = sealed_claim_root(&epoch, &owners, &weights, &allocations).unwrap();
        assert_eq!(
            hex(root),
            "c44b9425f20815d96c14f59c47ba36f997cdfc25a86d2eb5481b701389c94b81"
        );
    }
}
