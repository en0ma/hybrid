use super::*;

/// Opcode 44: bounded two-page, two-price atomic BID execution and seal.
///
/// Data: [44, base_fill:u64LE, min_quote_out:u64LE].
/// Accounts: market, custody, reserve, epoch_0, epoch_1,
///     (bid_page_0, sidecar_0), (bid_page_1, sidecar_1),
///     taker, taker_quote, taker_base, quote_vault, base_vault,
///     quote_mint, base_mint, vault_authority, token_program,
///     payer, system_program, and then one maker-balance PDA for
///     each flattened canonical BID entry.
///
/// Exactly two distinct price ticks and two linked pages. This smaller
/// launch envelope is intentionally separate from opcode 42's ABI.
const MAX_ATOMIC_BID_MAKERS: usize = 8;
const FIXED_ACCOUNTS: usize = 20;

/// Decode directly into heap-backed vectors. Avoid constructing two 1.5 KiB
/// pages as simultaneous stack temporaries in the settlement frame.
#[inline(never)]
fn load_multipage_bid_pairs(
    page_0: &AccountInfo,
    owner_0: &AccountInfo,
    page_1: &AccountInfo,
    owner_1: &AccountInfo,
) -> Result<(Vec<hybrid_state::BidPage>, Vec<hybrid_state::BidOwnerPage>), ProgramError> {
    let mut pages = Vec::with_capacity(2);
    let mut owners = Vec::with_capacity(2);
    for (page, owner) in [(page_0, owner_0), (page_1, owner_1)] {
        pages.resize_with(pages.len() + 1, hybrid_state::BidPage::default);
        owners.resize_with(owners.len() + 1, hybrid_state::BidOwnerPage::default);
        let index = pages.len() - 1;
        hybrid_state::BidPage::decode_into(&page.try_borrow_data()?, &mut pages[index])
            .map_err(|_| ProgramError::InvalidAccountData)?;
        hybrid_state::BidOwnerPage::decode_into(&owner.try_borrow_data()?, &mut owners[index])
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    Ok((pages, owners))
}

/// Isolate large local page/owner encoding temporaries from the execution
/// frame. Identical layout to the already established multipage seller.
#[inline(never)]
fn store_multipage_bid_updates(
    page_0: &AccountInfo,
    owner_0: &AccountInfo,
    page_1: &AccountInfo,
    owner_1: &AccountInfo,
    pages: &[hybrid_state::BidPage],
    owners: &[hybrid_state::BidOwnerPage],
) -> ProgramResult {
    for (index, (page_account, owner_account)) in
        [(page_0, owner_0), (page_1, owner_1)].iter().enumerate()
    {
        write_multipage_bid_pair(
            page_account,
            owner_account,
            index,
            pages.get(index),
            owners.get(index),
        )?;
    }
    Ok(())
}

#[inline(never)]
fn write_multipage_bid_pair(
    page_account: &AccountInfo,
    owner_account: &AccountInfo,
    index: usize,
    page: Option<&hybrid_state::BidPage>,
    owner: Option<&hybrid_state::BidOwnerPage>,
) -> ProgramResult {
    let mut next_page = page.copied().unwrap_or_default();
    let mut next_owner = owner.copied().unwrap_or_default();
    if page.is_none() {
        let links = hybrid_state::PageLinks::new(index as u32, None, None);
        next_page.set_links(links);
        next_owner.set_links(links);
    }
    next_page
        .encode_into(&mut page_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    next_owner
        .encode_into(&mut owner_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}

#[inline(never)]
pub(super) fn execute_and_seal_multipage_bid(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 17
        || data[0] != 44
        || accounts.len() < FIXED_ACCOUNTS + 2
        || accounts.len() > FIXED_ACCOUNTS + MAX_ATOMIC_BID_MAKERS
    {
        return Err(ProgramError::InvalidInstructionData);
    }
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
    let (fixed, balances) = accounts.split_at(FIXED_ACCOUNTS);
    let [market_account, custody_account, reserve_account, epoch_0, epoch_1, page_0, owner_0, page_1, owner_1, taker, taker_quote, taker_base, quote_vault, base_vault, quote_mint, base_mint, vault_authority, token_program, payer, system_program] =
        fixed
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !taker.is_signer || !payer.is_signer || !payer.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *token_program.key != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if fixed.iter().enumerate().any(|(i, account)| {
        let must_write = matches!(i, 0..=8 | 10..=13 | 18);
        must_write && !account.is_writable
    }) || balances.iter().any(|account| !account.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    // All state/token/epoch/authority accounts are role-distinct.
    // Repeated account metas must never turn into a bypass.
    for (i, account) in accounts.iter().enumerate() {
        if accounts[i + 1..]
            .iter()
            .any(|other| other.key == account.key)
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    if [
        market_account,
        custody_account,
        reserve_account,
        page_0,
        owner_0,
        page_1,
        owner_1,
    ]
    .iter()
    .any(|account| account.owner != program_id)
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut market = hybrid_state::MarketHeader::decode_from(&market_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market.collateralized_active() || market.bid_count() == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    for (index, (page, owner)) in [(page_0, owner_0), (page_1, owner_1)].iter().enumerate() {
        let index_bytes = (index as u32).to_le_bytes();
        let (expected, _) = Pubkey::find_program_address(
            &[b"bid-page", market_account.key.as_ref(), &index_bytes],
            program_id,
        );
        if *page.key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
        if index == 0 {
            if market.bid_owner_tag() != owner_tag(owner.key) {
                return Err(ProgramError::InvalidSeeds);
            }
        } else {
            let (expected_owner, _) = Pubkey::find_program_address(
                &[b"bid-owner-page", market_account.key.as_ref(), &index_bytes],
                program_id,
            );
            if *owner.key != expected_owner {
                return Err(ProgramError::InvalidSeeds);
            }
        }
    }
    let (pages, owners) = load_multipage_bid_pairs(page_0, owner_0, page_1, owner_1)?;
    let seats: usize = pages.iter().map(|page| page.len()).sum();
    if seats != balances.len() || seats != market.bid_count() as usize {
        return Err(ProgramError::InvalidAccountData);
    }
    let plan = hybrid_state::lazy_multilevel_bid::plan_multilevel_bid(
        &pages,
        &owners,
        base_fill,
        min_quote_out,
    )
    .map_err(|_| ProgramError::InvalidInstructionData)?;
    if plan.ticks.len() != 2 || plan.ticks[0].price_x64 == plan.ticks[1].price_x64 {
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
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let (old_base_floor, old_quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if lazy_bid_execute::token_amount(base_vault)? < old_base_floor
        || lazy_bid_execute::token_amount(quote_vault)? < old_quote_floor
        || custody.total_quote < plan.quote_paid
    {
        return Err(ProgramError::InsufficientFunds);
    }

    // Recheck the exact maker balance PDA for every canonical book row.
    // The account list covers all rows, including unfilled tail seats.
    // Keep still-locked quote debits until claim, unlocking only rounding
    // surplus released by the reduced order obligation.
    let mut updated_balances = Vec::with_capacity(seats);
    let mut fills = Vec::new();
    let mut row_index = 0usize;
    for tick in &plan.ticks {
        for ((maker, original), claim) in tick
            .makers
            .iter()
            .zip(tick.weights.iter())
            .zip(tick.allocations.iter())
        {
            let balance = balances
                .get(row_index)
                .ok_or(ProgramError::NotEnoughAccountKeys)?;
            let mut state =
                load_maker_balance_for_owner(program_id, market_account, balance, *maker)?;
            let filled = claim
                .base_proceeds
                .checked_add(claim.base_fee)
                .ok_or(ProgramError::InvalidAccountData)?;
            let remaining = original
                .checked_sub(filled)
                .ok_or(ProgramError::InvalidAccountData)?;
            let old_lock = hybrid_engine::quote_for_base_at_price(*original, tick.price_x64)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let new_lock = if remaining == 0 {
                0
            } else {
                hybrid_engine::quote_for_base_at_price(remaining, tick.price_x64)
                    .map_err(|_| ProgramError::InvalidAccountData)?
            };
            let released = old_lock
                .checked_sub(new_lock)
                .ok_or(ProgramError::InvalidAccountData)?;
            let surplus = released
                .checked_sub(claim.quote_spent)
                .ok_or(ProgramError::InsufficientFunds)?;
            state
                .unlock_quote(surplus)
                .map_err(|_| ProgramError::InsufficientFunds)?;
            if state.locked_quote < claim.quote_spent {
                return Err(ProgramError::InsufficientFunds);
            }
            if filled > 0 {
                fills.push(hybrid_engine::ActiveBidFill {
                    bid_index: u16::try_from(row_index)
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                    base_qty: filled,
                    quote_qty: claim.quote_spent,
                });
            }
            updated_balances.push(state);
            row_index += 1;
        }
    }
    // The planner stops once requested liquidity is filled; any rows
    // following the last executed price still require a canonical balance
    // PDA, but must not be debited by this execution.
    for (index, maker) in owners
        .iter()
        .flat_map(|sidecar| sidecar.as_slice())
        .enumerate()
        .skip(row_index)
    {
        updated_balances.push(load_maker_balance_for_owner(
            program_id,
            market_account,
            &balances[index],
            *maker,
        )?);
    }
    if updated_balances.len() != seats {
        return Err(ProgramError::InvalidAccountData);
    }

    // Precompute linked-page compaction and reserve liabilities before any
    // SPL CPI. Retired pages are reset to their canonical empty encoding.
    let (next_pages, next_owners, removed) =
        hybrid_state::multipage_bid::apply_linked_bid_fills(&pages, &owners, &fills)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    market.set_bid_count(
        market
            .bid_count()
            .checked_sub(removed as u32)
            .ok_or(ProgramError::InvalidAccountData)?,
    );
    custody.total_quote = custody
        .total_quote
        .checked_sub(plan.quote_paid)
        .ok_or(ProgramError::InsufficientFunds)?;

    let mut headers = Vec::with_capacity(2);
    let first_epoch = reserve.next_epoch_id;
    for (index, tick) in plan.ticks.iter().enumerate() {
        let epoch_id = first_epoch
            .checked_add(index as u64)
            .ok_or(ProgramError::InvalidAccountData)?;
        let id_bytes = epoch_id.to_le_bytes();
        let (expected_epoch, bump) = Pubkey::find_program_address(
            &[b"frozen-epoch", market_account.key.as_ref(), &id_bytes],
            program_id,
        );
        let epoch_account = if index == 0 { epoch_0 } else { epoch_1 };
        if *epoch_account.key != expected_epoch
            || *epoch_account.owner != Pubkey::default()
            || !epoch_account.data_is_empty()
        {
            return Err(ProgramError::InvalidSeeds);
        }
        let totals = tick.totals();
        let digest = lazy_bid_execute::epoch_commitment(
            market_account.key,
            program_id,
            &reserve.genesis,
            (epoch_id, tick.sqrt_price_x64),
            &tick.makers,
            &tick.weights,
            totals,
        )?;
        let root = lazy_bid_execute::sealed_claim_root(
            &digest,
            &tick.makers,
            &tick.weights,
            &tick.allocations,
        )?;
        let header = hybrid_state::frozen_epoch::FrozenEpochHeader {
            bump,
            side: 1,
            market: market_account.key.to_bytes(),
            authority: market_account.key.to_bytes(),
            program: program_id.to_bytes(),
            genesis: reserve.genesis,
            epoch_id,
            tick_sqrt_price_x64: tick.sqrt_price_x64,
            commitment: digest,
            claim_root: root,
            maker_count: tick.makers.len() as u32,
            totals,
            base_vault: custody.base_vault,
            quote_vault: custody.quote_vault,
        };
        header
            .validate()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        reserve
            .reserve_execution(epoch_id, totals)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        headers.push(header);
    }
    let (next_base_floor, next_quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if lazy_bid_execute::token_amount(base_vault)?
        .checked_add(base_fill)
        .ok_or(ProgramError::InvalidAccountData)?
        < next_base_floor
        || lazy_bid_execute::token_amount(quote_vault)?
            .checked_sub(plan.quote_paid)
            .ok_or(ProgramError::InsufficientFunds)?
            < next_quote_floor
    {
        return Err(ProgramError::InsufficientFunds);
    }

    // Each immutable epoch PDA is created and initialized before the SPL
    // CPIs. Transaction-level atomicity rolls back both epochs and every
    // token/book/balance change on either failed transfer or late write.
    for (index, header) in headers.iter().enumerate() {
        let epoch_account = if index == 0 { epoch_0 } else { epoch_1 };
        let id = header.epoch_id.to_le_bytes();
        create_program_pda(
            program_id,
            payer,
            epoch_account,
            system_program,
            &[b"frozen-epoch", market_account.key.as_ref(), &id],
            header.bump,
            hybrid_state::frozen_epoch::FROZEN_EPOCH_BYTES,
        )?;
        header
            .initialize(&mut epoch_account.try_borrow_mut_data()?)
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
        (plan.quote_paid, custody.quote_decimals),
        Some(seeds),
    )?;

    market
        .encode_into(&mut market_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    store_multipage_bid_updates(page_0, owner_0, page_1, owner_1, &next_pages, &next_owners)?;
    for (account, state) in balances.iter().zip(updated_balances.iter()) {
        store_maker_balance(account, state)?;
    }
    store_custody(custody_account, &custody)?;
    reserve
        .encode(&mut reserve_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}
