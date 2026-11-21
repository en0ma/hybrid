use super::*;

/// Opcode 42: atomic one-tick BID pro-rata execution and on-chain epoch seal.
/// Single canonical BID page, up to 16 authenticated maker seats, all at
/// the same Q64 price. Multipage/multitick execution is a separate ABI.
/// No sealer-supplied root or allocation is trusted: every leaf and the root
/// are derived from program-owned resting orders and the paid taker fill.
///
/// Data [42, filled_base:u64LE, min_quote_out:u64LE].
/// Accounts: market, custody, lazy reserve, fresh frozen epoch PDA,
///           bid page 0, bid owner sidecar, taker signer, taker quote SPL,
///           taker base SPL, quote vault, base vault, quote mint, base mint,
///           vault authority PDA, SPL token program, payer, system program,
///           then one writable maker balance PDA per canonical maker seat.
///
/// At execution, maker balances unlock only surplus quote collateral
/// released by rounded-up BID order reductions. Historical quote debits
/// remain locked until a maker's authenticated claim.
/// Maker base proceeds and base fees become segregated on-chain liabilities.
const MAX_LAZY_SEATS: usize = 16;
const EPOCH_DOMAIN: &[u8] = b"HYBRID_LAZY_PUBKEY_EPOCH_V1\0";
const LEAF_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_CLAIM_V1\0";
const NODE_DOMAIN: &[u8] = b"HYBRID_MAKER_PUBKEY_NODE_V1\0";

/// Update a verified maker PDA using the same collateral rounding rules
/// for both one-page and linked-page BID settlement. Quote debits remain
/// locked until the separately authenticated maker claim.
#[inline(never)]
pub(super) fn update_lazy_bid_maker_balance(
    program_id: &Pubkey,
    market: &AccountInfo,
    account: &AccountInfo,
    owner: [u8; 32],
    original: u64,
    price_x64: u128,
    allocation: &hybrid_state::lazy_pro_rata::MakerBidAllocation,
) -> Result<hybrid_state::MakerBalance, ProgramError> {
    let mut balance = load_maker_balance_for_owner(program_id, market, account, owner)?;
    let filled = allocation
        .base_proceeds
        .checked_add(allocation.base_fee)
        .ok_or(ProgramError::InvalidAccountData)?;
    let remaining = original
        .checked_sub(filled)
        .ok_or(ProgramError::InvalidAccountData)?;
    let old_lock = hybrid_engine::quote_for_base_at_price(original, price_x64)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let new_lock = if remaining == 0 {
        0
    } else {
        hybrid_engine::quote_for_base_at_price(remaining, price_x64)
            .map_err(|_| ProgramError::InvalidAccountData)?
    };
    let released = old_lock
        .checked_sub(new_lock)
        .ok_or(ProgramError::InvalidAccountData)?;
    let surplus = released
        .checked_sub(allocation.quote_spent)
        .ok_or(ProgramError::InsufficientFunds)?;
    balance
        .unlock_quote(surplus)
        .map_err(|_| ProgramError::InsufficientFunds)?;
    if balance.locked_quote < allocation.quote_spent {
        return Err(ProgramError::InsufficientFunds);
    }
    Ok(balance)
}

/// Verify custody-owned SPL accounts and canonical vault authority for
/// both BID sealer ABIs. Shared to avoid expanding linked-SBF bytecode.
#[inline(never)]
pub(super) fn validate_bid_custody_token_accounts<'a>(
    program_id: &Pubkey,
    market_account: &AccountInfo<'a>,
    custody: &hybrid_state::CustodyState,
    taker: &AccountInfo<'a>,
    taker_base: &AccountInfo<'a>,
    taker_quote: &AccountInfo<'a>,
    base_vault: &AccountInfo<'a>,
    quote_vault: &AccountInfo<'a>,
    base_mint: &AccountInfo<'a>,
    quote_mint: &AccountInfo<'a>,
    vault_authority: &AccountInfo<'a>,
) -> Result<u8, ProgramError> {
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
    Ok(authority_bump)
}

/// Derive the same immutable V1 BID epoch commitment and claim root from
/// program-owned resting rows in both execution ABIs.
#[inline(never)]
pub(super) fn build_bid_epoch_header(
    program_id: &Pubkey,
    market: &Pubkey,
    epoch_key: &Pubkey,
    custody: &hybrid_state::CustodyState,
    genesis: [u8; 32],
    epoch_id: u64,
    tick_sqrt_price_x64: u128,
    owners: &[[u8; 32]],
    weights: &[u64],
    allocations: &[hybrid_state::lazy_pro_rata::MakerBidAllocation],
    totals: [u64; 6],
) -> Result<hybrid_state::frozen_epoch::FrozenEpochHeader, ProgramError> {
    let digest = epoch_commitment(
        market, program_id, &genesis, (epoch_id, tick_sqrt_price_x64),
        owners, weights, totals,
    )?;
    let root = sealed_claim_root(&digest, owners, weights, allocations)?;
    let id_bytes = epoch_id.to_le_bytes();
    let (expected_epoch, bump) = Pubkey::find_program_address(
        &[b"frozen-epoch", market.as_ref(), &id_bytes], program_id,
    );
    if *epoch_key != expected_epoch {
        return Err(ProgramError::InvalidSeeds);
    }
    let header = hybrid_state::frozen_epoch::FrozenEpochHeader {
        bump,
        side: 1,
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
    header.validate().map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(header)
}

// Use the same canonical floor-sqrt interval accepted by order placement.
fn validate_resting_bid_price(price_x64: u128, sqrt_price_x64: u128) -> ProgramResult {
    hybrid_engine::validate_limit_ask(hybrid_engine::LimitAsk {
        price_x64,
        sqrt_price_x64,
        base_qty: 1,
    })
    .map_err(|_| ProgramError::InvalidAccountData)
}

pub(super) fn token_amount(account: &AccountInfo) -> Result<u64, ProgramError> {
    token_account_fields(account)?;
    let data = account.try_borrow_data()?;
    Ok(u64::from_le_bytes(
        data[64..72]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    ))
}

pub(super) fn epoch_commitment(
    market: &Pubkey,
    program: &Pubkey,
    genesis: &[u8; 32],
    epoch: (u64, u128),
    owners: &[[u8; 32]],
    weights: &[u64],
    totals: [u64; 6],
) -> Result<[u8; 32], ProgramError> {
    if weights.is_empty() || weights.len() > MAX_LAZY_SEATS || owners.len() != weights.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    // 16 maker rows at 44 bytes each plus the 201-byte epoch envelope
    // require 905 bytes. Keep fixed capacity for bounded SBF hashing.
    let mut bytes = [0u8; 928];
    let mut used = 0usize;
    let mut append = |part: &[u8]| {
        let end = used + part.len();
        bytes[used..end].copy_from_slice(part);
        used = end;
    };
    append(EPOCH_DOMAIN);
    append(market.as_ref());
    append(program.as_ref());
    append(genesis);
    append(&epoch.0.to_le_bytes());
    append(&epoch.1.to_le_bytes());
    append(&[1]); // bid side
    append(&(weights.len() as u32).to_le_bytes());
    for (index, (owner, weight)) in owners.iter().zip(weights).enumerate() {
        append(&(index as u32).to_le_bytes());
        append(owner);
        append(&weight.to_le_bytes());
    }
    for amount in totals {
        append(&amount.to_le_bytes());
    }
    Ok(solana_program::hash::hash(&bytes[..used]).to_bytes())
}

pub(super) fn sealed_claim_root(
    epoch_commitment: &[u8; 32],
    owners: &[[u8; 32]],
    weights: &[u64],
    allocations: &[hybrid_state::lazy_pro_rata::MakerBidAllocation],
) -> Result<[u8; 32], ProgramError> {
    if owners.is_empty()
        || owners.len() > MAX_LAZY_SEATS
        || owners.len() != weights.len()
        || weights.len() != allocations.len()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    // In-place binary Merkle reduction needs only sixteen fixed hashes.
    // Odd final leaves are duplicated, matching Rust and SDK proofs.
    let mut nodes = [[0u8; 32]; MAX_LAZY_SEATS];
    for (index, ((maker, weight), allocation)) in
        owners.iter().zip(weights).zip(allocations).enumerate()
    {
        let seat = (index as u32).to_le_bytes();
        let claim = [
            0,
            allocation.base_proceeds,
            allocation.quote_spent,
            0,
            allocation.base_fee,
            0,
        ];
        let amounts = claim.map(u64::to_le_bytes);
        nodes[index] = solana_program::hash::hashv(&[
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
        ])
        .to_bytes();
    }
    let mut count = owners.len();
    while count > 1 {
        let next = count.div_ceil(2);
        for index in 0..next {
            let left = nodes[index * 2];
            let right = if index * 2 + 1 < count {
                nodes[index * 2 + 1]
            } else {
                left
            };
            nodes[index] = solana_program::hash::hashv(&[NODE_DOMAIN, &left, &right]).to_bytes();
        }
        count = next;
    }
    Ok(nodes[0])
}

#[inline(never)]
pub(super) fn execute_and_seal_bid(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 17 || data[0] != 42 || accounts.len() < 18 || accounts.len() > 33 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let (fixed_accounts, maker_balance_accounts) = accounts.split_at(17);
    let [market_account, custody_account, reserve_account, epoch_account, bid_account, owner_account, taker, taker_quote, taker_base, quote_vault, base_vault, quote_mint, base_mint, vault_authority, token_program, payer, system_program] =
        fixed_accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let base_fill = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let min_quote_out = u64::from_le_bytes(
        data[9..17]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if base_fill == 0 || min_quote_out == 0 || !taker.is_signer || !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market_account.owner != program_id
        || bid_account.owner != program_id
        || owner_account.owner != program_id
        || *token_program.key != TOKEN_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !market_account.is_writable
        || !custody_account.is_writable
        || !reserve_account.is_writable
        || !epoch_account.is_writable
        || !bid_account.is_writable
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
    if !market.collateralized_active() || market.bid_count() == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    let page_id = 0u32.to_le_bytes();
    let (expected_page, _) = Pubkey::find_program_address(
        &[b"bid-page", market_account.key.as_ref(), &page_id],
        program_id,
    );
    if *bid_account.key != expected_page || market.bid_owner_tag() != owner_tag(owner_account.key) {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut bids = load_active_bid_page(bid_account)?;
    let mut owner_page = load_owner_page_box(owner_account)?;
    if owner_page.len() != bids.len()
        || owner_page.links() != bids.links()
        || owner_page.as_slice().contains(&[0; 32])
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let links = bids.links();
    if links.page_index != 0
        || links.prev_page.is_some()
        || links.next_page.is_some()
        || bids.len() != market.bid_count() as usize
        || bids.is_empty()
        || bids.len() > MAX_LAZY_SEATS
    {
        return Err(ProgramError::InvalidAccountData);
    }
    bids.validate_order()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let resting_tick = bids.as_slice()[0].sqrt_price_x64;
    let resting_price = bids.as_slice()[0].price_x64;
    if resting_tick == 0 || resting_price == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    let weights: Vec<u64> = bids.as_slice().iter().map(|entry| entry.base_qty).collect();
    let owners: Vec<[u8; 32]> = owner_page.as_slice().to_vec();
    if maker_balance_accounts.len() != weights.len() {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    for (index, entry) in bids.as_slice().iter().enumerate() {
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
    validate_resting_bid_price(resting_price, resting_tick)?;

    let quote_paid = hybrid_engine::quote_for_base_at_price(base_fill, resting_price)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    if quote_paid < min_quote_out {
        return Err(ProgramError::InsufficientFunds);
    }
    let base_fee = ((u128::from(base_fill) * u128::from(PASSIVE_FEE_BPS)) / 10_000u128) as u64;
    let allocations = hybrid_state::lazy_pro_rata::allocate_bid_at_price(
        &weights,
        base_fill,
        resting_price,
        base_fee,
    )
    .map_err(|_| ProgramError::InvalidInstructionData)?;

    // Authenticate every maker balance against the program-owned sidecar.
    // Release is old ceil-quote collateral minus remaining ceil collateral.
    // Only unused collateral is unlocked now; debited quote remains locked
    // for the maker's eventual Merkle-authenticated claim.
    let mut updated_maker_balances = Vec::with_capacity(weights.len());
    for (index, (account, owner)) in maker_balance_accounts.iter().zip(&owners).enumerate() {
        if !account.is_writable
            || fixed_accounts.iter().any(|fixed| fixed.key == account.key)
            || maker_balance_accounts[..index]
                .iter()
                .any(|previous| previous.key == account.key)
        {
            return Err(ProgramError::InvalidAccountData);
        }
        let balance = update_lazy_bid_maker_balance(
            program_id,
            market_account,
            account,
            *owner,
            weights[index],
            resting_price,
            &allocations[index],
        )?;
        updated_maker_balances.push(balance);
    }

    let mut custody = load_custody(program_id, market_account, custody_account)?;
    let mut reserve = lazy_reserve::load_lazy_reserve(program_id, market_account, reserve_account)?;
    let authority_bump = validate_bid_custody_token_accounts(
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
    if token_amount(base_vault)? < floor_base
        || token_amount(quote_vault)? < floor_quote
        || custody.total_quote < quote_paid
    {
        return Err(ProgramError::InsufficientFunds);
    }
    let epoch_id = reserve.next_epoch_id;
    let totals = [0, base_fill - base_fee, quote_paid, 0, base_fee, 0];
    let id_bytes = epoch_id.to_le_bytes();
    let header = build_bid_epoch_header(
        program_id, market_account.key, epoch_account.key, &custody,
        reserve.genesis, epoch_id, resting_tick, &owners, &weights,
        &allocations, totals,
    )?;
    let epoch_bump = header.bump;

    // Precalculate every state transition before the first token CPI.
    let fills: Vec<u64> = allocations
        .iter()
        .map(|claim| claim.base_proceeds + claim.base_fee)
        .collect();
    let removed = hybrid_state::apply_lazy_bid_fills(&mut bids, &mut owner_page, &fills)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    market.set_bid_count(
        market
            .bid_count()
            .checked_sub(removed as u32)
            .ok_or(ProgramError::InvalidAccountData)?,
    );
    custody.total_quote = custody
        .total_quote
        .checked_sub(quote_paid)
        .ok_or(ProgramError::InsufficientFunds)?;
    reserve
        .reserve_execution(epoch_id, totals)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (next_floor_base, next_floor_quote) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if token_amount(base_vault)?
        .checked_add(base_fill)
        .ok_or(ProgramError::InvalidAccountData)?
        < next_floor_base
        || token_amount(quote_vault)?
            .checked_sub(quote_paid)
            .ok_or(ProgramError::InsufficientFunds)?
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
        taker_base,
        base_mint,
        base_vault,
        taker,
        token_program,
        (base_fill, custody.base_decimals),
        None,
    )?;
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market_account.key.as_ref(), &bump];
    transfer_checked(
        quote_vault,
        quote_mint,
        taker_quote,
        vault_authority,
        token_program,
        (quote_paid, custody.quote_decimals),
        Some(seeds),
    )?;
    {
        let mut bytes = market_account.try_borrow_mut_data()?;
        market
            .encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut bytes = bid_account.try_borrow_mut_data()?;
        bids.encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut bytes = owner_account.try_borrow_mut_data()?;
        owner_page
            .encode_into(&mut bytes)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    // Refunds only reclassify free/locked quote, never move SPL tokens.
    for (account, balance) in maker_balance_accounts.iter().zip(&updated_maker_balances) {
        store_maker_balance(account, balance)?;
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
mod extended_roster_tests {
    use super::*;

    fn as_hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn nine_and_sixteen_seat_bid_roots_match_sdk_v1_and_duplicate_odd_leaves() {
        let market = Pubkey::new_from_array([0x11; 32]);
        let program = Pubkey::new_from_array([0x22; 32]);
        for (count, expected_digest, expected_root) in [
            (
                9usize,
                "81eaae5151672a4b1fb6a0cb1f41dcfc5173003cc6759024fcf91686550733c6",
                "06e7d26453d0aaa9b13b40f2afb5c2136be470326d0e1a2a44e48f443d62a025",
            ),
            (
                16usize,
                "6c753eed7dd4ab15711ec2d48b77c4d3315b7d2839186e9a98a50738cbe14959",
                "7d6f22035f78d5898824a267180132f69fa6bb7c1539b480f88bf41d0551c9e2",
            ),
        ] {
            let owners: Vec<[u8; 32]> = (1..=count).map(|i| [i as u8; 32]).collect();
            let weights = vec![1u64; count];
            let totals = [0, count as u64, count as u64, 0, 0, 0];
            let digest = epoch_commitment(
                &market,
                &program,
                &[0x33; 32],
                (7, hybrid_engine::Q64),
                &owners,
                &weights,
                totals,
            )
            .unwrap();
            assert_eq!(as_hex(digest), expected_digest);
            let allocations = hybrid_state::lazy_pro_rata::allocate_bid_at_price(
                &weights,
                count as u64,
                hybrid_engine::Q64,
                0,
            )
            .unwrap();
            let root = sealed_claim_root(&digest, &owners, &weights, &allocations).unwrap();
            assert_eq!(as_hex(root), expected_root);
        }
    }

    #[test]
    fn seventeen_seats_are_rejected_before_commitment_or_merkle_hashing() {
        let owners: Vec<[u8; 32]> = (1..=17).map(|i| [i; 32]).collect();
        let weights = vec![1; owners.len()];
        let market = Pubkey::new_from_array([0x11; 32]);
        let program = Pubkey::new_from_array([0x22; 32]);
        assert_eq!(
            epoch_commitment(
                &market,
                &program,
                &[0x33; 32],
                (7, hybrid_engine::Q64),
                &owners,
                &weights,
                [0, 17, 17, 0, 0, 0],
            ),
            Err(ProgramError::InvalidAccountData)
        );
    }
}
