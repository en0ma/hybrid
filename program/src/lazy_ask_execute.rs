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
const EPOCH_DOMAIN: &[u8] = b"HYBRID_LAZY_PUBKEY_EPOCH_V1\0";
const LEAF_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_CLAIM_V1\0";
const NODE_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_NODE_V1\0";

fn token_amount(account: &AccountInfo) -> Result<u64, ProgramError> {
    token_account_fields(account)?;
    let data = account.try_borrow_data()?;
    Ok(u64::from_le_bytes(
        data[64..72]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    ))
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
    let count = u32::try_from(weights.len()).map_err(|_| ProgramError::InvalidAccountData)?;
    let mut bytes = Vec::with_capacity(256 + weights.len() * 44);
    bytes.extend_from_slice(EPOCH_DOMAIN);
    bytes.extend_from_slice(market.as_ref());
    bytes.extend_from_slice(program.as_ref());
    bytes.extend_from_slice(genesis);
    bytes.extend_from_slice(&epoch.0.to_le_bytes());
    bytes.extend_from_slice(&epoch.1.to_le_bytes());
    bytes.push(0); // ask side
    bytes.extend_from_slice(&count.to_le_bytes());
    for (index, (owner, weight)) in owners.iter().zip(weights).enumerate() {
        bytes.extend_from_slice(&(index as u32).to_le_bytes());
        bytes.extend_from_slice(owner);
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    for amount in totals {
        bytes.extend_from_slice(&amount.to_le_bytes());
    }
    Ok(solana_program::hash::hash(&bytes).to_bytes())
}

fn sealed_claim_root(
    epoch_commitment: &[u8; 32],
    owners: &[[u8; 32]],
    weights: &[u64],
    allocations: &[hybrid_state::lazy_pro_rata::MakerAllocation],
) -> Result<[u8; 32], ProgramError> {
    if owners.is_empty() || owners.len() != weights.len() || weights.len() != allocations.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut nodes = Vec::with_capacity(owners.len());
    for (index, ((maker, weight), allocation)) in
        owners.iter().zip(weights).zip(allocations).enumerate()
    {
        let seat = (index as u32).to_le_bytes();
        let claim = [
            allocation.base_fill,
            0,
            0,
            allocation.quote_proceeds,
            0,
            allocation.quote_fee,
        ];
        let amounts = claim.map(u64::to_le_bytes);
        let digest = solana_program::hash::hashv(&[
            LEAF_DOMAIN,
            epoch_commitment,
            &seat,
            maker,
            &weight.to_le_bytes(),
            &amounts[0],
            &amounts[1],
            &amounts[2],
            &amounts[3],
            &amounts[4],
            &amounts[5],
        ]);
        nodes.push(digest.to_bytes());
    }
    while nodes.len() > 1 {
        let mut parents = Vec::with_capacity(nodes.len().div_ceil(2));
        for pair in nodes.chunks(2) {
            let right = if pair.len() == 2 { &pair[1] } else { &pair[0] };
            parents.push(solana_program::hash::hashv(&[NODE_DOMAIN, &pair[0], right]).to_bytes());
        }
        nodes = parents;
    }
    Ok(nodes[0])
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
        || asks.len() == 0
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
    if hybrid_engine::spot_price_x64(resting_tick).map_err(|_| ProgramError::InvalidAccountData)?
        != resting_price
    {
        return Err(ProgramError::InvalidAccountData);
    }

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
    if custody.base_vault != base_vault.key.to_bytes()
        || custody.quote_vault != quote_vault.key.to_bytes()
        || custody.base_mint != base_mint.key.to_bytes()
        || custody.quote_mint != quote_mint.key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let (expected_authority, authority_bump) = Pubkey::find_program_address(
        &[b"vault-authority", market_account.key.as_ref()],
        program_id,
    );
    if *vault_authority.key != expected_authority {
        return Err(ProgramError::InvalidSeeds);
    }
    for (account, expected_mint, expected_owner) in [
        (base_vault, custody.base_mint, expected_authority.to_bytes()),
        (
            quote_vault,
            custody.quote_mint,
            expected_authority.to_bytes(),
        ),
        (taker_base, custody.base_mint, taker.key.to_bytes()),
        (taker_quote, custody.quote_mint, taker.key.to_bytes()),
    ] {
        let (mint, owner) = token_account_fields(account)?;
        if mint != expected_mint || owner != expected_owner {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    if mint_decimals(base_mint)? != custody.base_decimals
        || mint_decimals(quote_mint)? != custody.quote_decimals
        || *taker_base.key == *base_vault.key
        || *taker_quote.key == *quote_vault.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // Reconcile vault ownership and ALL historical liabilities before
    // creating the next epoch, not merely the new epoch's Merkle root.
    let (floor_base, floor_quote) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if token_amount(base_vault)? < floor_base
        || token_amount(quote_vault)? < floor_quote
        || custody.total_base < base_fill
    {
        return Err(ProgramError::InsufficientFunds);
    }
    let epoch_id = reserve.next_epoch_id;
    let totals = [base_fill, 0, 0, quote_proceeds, 0, quote_fee];
    let digest = epoch_commitment(
        market_account.key,
        program_id,
        &reserve.genesis,
        (epoch_id, resting_tick),
        &owners,
        &weights,
        totals,
    )?;
    let root = sealed_claim_root(&digest, &owners, &weights, &allocations)?;
    let id_bytes = epoch_id.to_le_bytes();
    let (expected_epoch, epoch_bump) = Pubkey::find_program_address(
        &[b"frozen-epoch", market_account.key.as_ref(), &id_bytes],
        program_id,
    );
    if *epoch_account.key != expected_epoch {
        return Err(ProgramError::InvalidSeeds);
    }
    let header = hybrid_state::frozen_epoch::FrozenEpochHeader {
        bump: epoch_bump,
        side: 0,
        market: market_account.key.to_bytes(),
        authority: market_account.key.to_bytes(),
        program: program_id.to_bytes(),
        genesis: reserve.genesis,
        epoch_id,
        tick_sqrt_price_x64: resting_tick,
        commitment: digest,
        claim_root: root,
        maker_count: weights.len() as u32,
        totals,
        base_vault: custody.base_vault,
        quote_vault: custody.quote_vault,
    };
    header
        .validate()
        .map_err(|_| ProgramError::InvalidAccountData)?;

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
    if token_amount(base_vault)?
        .checked_sub(base_fill)
        .ok_or(ProgramError::InsufficientFunds)?
        < next_floor_base
        || token_amount(quote_vault)?
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
