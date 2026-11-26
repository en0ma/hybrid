use super::*;

/// Opcodes 44/45: bounded two-page, two-price atomic BID/ASK execution and seal.
///
/// Data: [44, base_fill:u64LE, min_quote_out:u64LE].
/// Accounts: market, custody, reserve, epoch_0, epoch_1,
///     (bid_page_0, sidecar_0), (bid_page_1, sidecar_1),
///     taker, taker_quote, taker_base, quote_vault, base_vault,
///     quote_mint, base_mint, vault_authority, token_program,
///     payer, system_program, and then one maker-balance PDA for
///     each flattened canonical BID entry.
///
/// Exactly two distinct price ticks and two linked pages, with at most
/// sixteen canonical maker rows. Larger rosters require v0/ALT packing.
const MAX_ATOMIC_BID_MAKERS: usize = 16;
const MAX_ATOMIC_ASK_MAKERS: usize = 8;
const FIXED_ACCOUNTS: usize = 20;

/// Decode directly into heap-backed vectors. Avoid constructing two 1.5 KiB
/// pages as simultaneous stack temporaries in the settlement frame.
#[inline(never)]
fn load_multipage_bid_pairs<'a>(
    page_0: &AccountInfo<'a>,
    owner_0: &AccountInfo<'a>,
    page_1: &AccountInfo<'a>,
    owner_1: &AccountInfo<'a>,
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
fn store_multipage_bid_updates<'a>(
    page_0: &AccountInfo<'a>,
    owner_0: &AccountInfo<'a>,
    page_1: &AccountInfo<'a>,
    owner_1: &AccountInfo<'a>,
    pages: &[hybrid_state::BidPage],
    owners: &[hybrid_state::BidOwnerPage],
) -> ProgramResult {
    for (index, (page_account, owner_account)) in
        [(page_0, owner_0), (page_1, owner_1)].iter().enumerate()
    {
        multipage_sell::store_bid_data_page(page_account, index, pages.get(index))?;
        multipage_sell::store_bid_owner_page(owner_account, index, owners.get(index))?;
    }
    Ok(())
}

/// Decode directly into heap-backed vectors. Avoid constructing two 1.5 KiB
/// pages as simultaneous stack temporaries in the settlement frame.
#[inline(never)]
fn load_multipage_ask_pairs<'a>(
    page_0: &AccountInfo<'a>,
    owner_0: &AccountInfo<'a>,
    page_1: &AccountInfo<'a>,
    owner_1: &AccountInfo<'a>,
) -> Result<(Vec<hybrid_state::AskPage>, Vec<hybrid_state::AskOwnerPage>), ProgramError> {
    let mut pages = Vec::with_capacity(2);
    let mut owners = Vec::with_capacity(2);
    for (page, owner) in [(page_0, owner_0), (page_1, owner_1)] {
        pages.resize_with(pages.len() + 1, hybrid_state::AskPage::default);
        owners.resize_with(owners.len() + 1, hybrid_state::AskOwnerPage::default);
        let index = pages.len() - 1;
        hybrid_state::AskPage::decode_into(&page.try_borrow_data()?, &mut pages[index])
            .map_err(|_| ProgramError::InvalidAccountData)?;
        hybrid_state::AskOwnerPage::decode_into(&owner.try_borrow_data()?, &mut owners[index])
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    Ok((pages, owners))
}

/// Isolate large local page/owner encoding temporaries from the execution
/// frame. Identical layout to the already established multipage buyer.
#[inline(never)]
fn store_multipage_ask_updates<'a>(
    page_0: &AccountInfo<'a>,
    owner_0: &AccountInfo<'a>,
    page_1: &AccountInfo<'a>,
    owner_1: &AccountInfo<'a>,
    pages: &[hybrid_state::AskPage],
    owners: &[hybrid_state::AskOwnerPage],
) -> ProgramResult {
    for (index, (page_account, owner_account)) in
        [(page_0, owner_0), (page_1, owner_1)].iter().enumerate()
    {
        multipage_buy::store_ask_data_page(page_account, index, pages.get(index))?;
        multipage_buy::store_ask_owner_page(owner_account, index, owners.get(index))?;
    }
    Ok(())
}

enum PageUpdates {
    Bid(
        Vec<hybrid_state::BidPage>,
        Vec<hybrid_state::BidOwnerPage>,
        Vec<hybrid_state::MakerBalance>,
    ),
    Ask(Vec<hybrid_state::AskPage>, Vec<hybrid_state::AskOwnerPage>),
}

#[inline(never)]
pub(super) fn execute_and_seal_multipage(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    let is_bid = data.first() == Some(&44);
    let cap = if is_bid {
        MAX_ATOMIC_BID_MAKERS
    } else {
        MAX_ATOMIC_ASK_MAKERS
    };
    if data.len() != 17
        || (!is_bid && data[0] != 45)
        || accounts.len() < FIXED_ACCOUNTS + 2
        || accounts.len() > FIXED_ACCOUNTS + cap
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let base_fill = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let quote_limit = u64::from_le_bytes(
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
    if *token_program.key != TOKEN_PROGRAM_ID || *system_program.key != Pubkey::default() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if fixed.iter().enumerate().any(|(i, account)| {
        let must_write = matches!(i, 0..=8 | 10..=13 | 18);
        must_write && !account.is_writable
    }) || (is_bid && balances.iter().any(|account| !account.is_writable))
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
    let count = if is_bid {
        market.bid_count()
    } else {
        market.ask_count
    };
    if !market.collateralized_active() || count == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    for (index, (page, owner)) in [(page_0, owner_0), (page_1, owner_1)].iter().enumerate() {
        let index_bytes = (index as u32).to_le_bytes();
        let (expected, _) = Pubkey::find_program_address(
            &[
                if is_bid { b"bid-page" } else { b"ask-page" },
                market_account.key.as_ref(),
                &index_bytes,
            ],
            program_id,
        );
        if *page.key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
        if index == 0 {
            if (is_bid && market.bid_owner_tag() != owner_tag(owner.key))
                || (!is_bid && market.reserved2 != owner.key.to_bytes())
            {
                return Err(ProgramError::InvalidSeeds);
            }
        } else {
            let (expected_owner, _) = Pubkey::find_program_address(
                &[
                    if is_bid { b"bid-owner-page" } else { b"ask-owner-page" },
                    market_account.key.as_ref(),
                    &index_bytes,
                ],
                program_id,
            );
            if *owner.key != expected_owner {
                return Err(ProgramError::InvalidSeeds);
            }
        }
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
    let (old_base_floor, old_quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if lazy_bid_execute::token_amount(base_vault)? < old_base_floor
        || lazy_bid_execute::token_amount(quote_vault)? < old_quote_floor
    {
        return Err(ProgramError::InsufficientFunds);
    }

    let (quote_amount, headers, updates) = if is_bid {
        let (pages, owners) = load_multipage_bid_pairs(page_0, owner_0, page_1, owner_1)?;
        let seats: usize = pages.iter().map(|page| page.len()).sum();
        if seats != balances.len() || seats != market.bid_count() as usize {
            return Err(ProgramError::InvalidAccountData);
        }
        let plan = hybrid_state::lazy_multilevel_bid::plan_multilevel_bid(
            &pages,
            &owners,
            base_fill,
            quote_limit,
        )
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.ticks.len() != 2 || plan.ticks[0].price_x64 == plan.ticks[1].price_x64 {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Recheck the exact maker balance PDA for every canonical book row.
        // The account list covers all rows, including unfilled tail seats.
        // Keep still-locked quote debits until claim, unlocking only rounding
        // surplus released by the reduced order obligation.
        if custody.total_quote < plan.quote_paid {
            return Err(ProgramError::InsufficientFunds);
        }
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
                let state = lazy_bid_execute::update_lazy_bid_maker_balance(
                    program_id,
                    market_account,
                    balance,
                    *maker,
                    *original,
                    tick.price_x64,
                    claim,
                )?;
                let filled = claim
                    .base_proceeds
                    .checked_add(claim.base_fee)
                    .ok_or(ProgramError::InvalidAccountData)?;
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
            let epoch_account = if index == 0 { epoch_0 } else { epoch_1 };
            // build_bid_epoch_header authenticates the epoch PDA seeds;
            // avoid deriving the same key twice in this hot SBF path.
            if *epoch_account.owner != Pubkey::default() || !epoch_account.data_is_empty() {
                return Err(ProgramError::InvalidSeeds);
            }
            let totals = tick.totals();
            let header = lazy_bid_execute::build_bid_epoch_header(
                program_id,
                market_account.key,
                epoch_account.key,
                &custody,
                reserve.genesis,
                epoch_id,
                tick.sqrt_price_x64,
                &tick.makers,
                &tick.weights,
                &tick.allocations,
                totals,
            )?;
            reserve
                .reserve_execution(epoch_id, totals)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            headers.push(header);
        }
        (
            plan.quote_paid,
            headers,
            PageUpdates::Bid(next_pages, next_owners, updated_balances),
        )
    } else {
        let (pages, owners) = load_multipage_ask_pairs(page_0, owner_0, page_1, owner_1)?;
        let seats: usize = pages.iter().map(|page| page.len()).sum();
        if seats != balances.len() || seats != market.ask_count as usize {
            return Err(ProgramError::InvalidAccountData);
        }
        let plan = hybrid_state::lazy_multilevel_ask::plan_multilevel_ask(
            &pages,
            &owners,
            base_fill,
            quote_limit,
        )
        .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.ticks.len() != 2 || plan.ticks[0].price_x64 == plan.ticks[1].price_x64 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let quote_in = plan
            .quote_proceeds
            .checked_add(plan.quote_fee)
            .ok_or(ProgramError::InvalidInstructionData)?;
        // Authenticate every maker balance; historical debits remain locked
        // until opcode 40 claims, just as with opcode 41.
        if custody.total_base < base_fill {
            return Err(ProgramError::InsufficientFunds);
        }
        let mut fills = Vec::new();
        let mut row_index = 0usize;
        for tick in &plan.ticks {
            for ((maker, weight), claim) in tick
                .makers
                .iter()
                .zip(tick.weights.iter())
                .zip(tick.allocations.iter())
            {
                let balance = load_maker_balance_for_owner(
                    program_id,
                    market_account,
                    &balances[row_index],
                    *maker,
                )?;
                if balance.locked_base < *weight {
                    return Err(ProgramError::InsufficientFunds);
                }
                if claim.base_fill > 0 {
                    fills.push(hybrid_engine::ActiveFill {
                        ask_index: u16::try_from(row_index)
                            .map_err(|_| ProgramError::InvalidAccountData)?,
                        base_qty: claim.base_fill,
                        quote_qty: claim.quote_proceeds,
                    });
                }
                row_index += 1;
            }
        }
        for (index, maker) in owners
            .iter()
            .flat_map(|sidecar| sidecar.as_slice())
            .enumerate()
            .skip(row_index)
        {
            load_maker_balance_for_owner(program_id, market_account, &balances[index], *maker)?;
        }
        let (next_pages, next_owners, removed) =
            hybrid_state::multipage_ask::apply_linked_ask_fills(&pages, &owners, &fills)
                .map_err(|_| ProgramError::InvalidAccountData)?;
        market.ask_count = market
            .ask_count
            .checked_sub(removed as u32)
            .ok_or(ProgramError::InvalidAccountData)?;
        custody.total_base = custody
            .total_base
            .checked_sub(base_fill)
            .ok_or(ProgramError::InsufficientFunds)?;

        let mut headers = Vec::with_capacity(2);
        let first_epoch = reserve.next_epoch_id;
        for (index, tick) in plan.ticks.iter().enumerate() {
            let epoch_id = first_epoch
                .checked_add(index as u64)
                .ok_or(ProgramError::InvalidAccountData)?;
            let epoch_account = if index == 0 { epoch_0 } else { epoch_1 };
            // build_ask_epoch_header authenticates the epoch PDA seeds;
            // avoid deriving the same key twice in this hot SBF path.
            if *epoch_account.owner != Pubkey::default() || !epoch_account.data_is_empty() {
                return Err(ProgramError::InvalidSeeds);
            }
            let totals = tick.totals();
            let header = lazy_ask_execute::build_ask_epoch_header(
                program_id,
                market_account.key,
                epoch_account.key,
                &custody,
                reserve.genesis,
                epoch_id,
                tick.sqrt_price_x64,
                &tick.makers,
                &tick.weights,
                &tick.allocations,
                totals,
            )?;
            reserve
                .reserve_execution(epoch_id, totals)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            headers.push(header);
        }
        (quote_in, headers, PageUpdates::Ask(next_pages, next_owners))
    };
    let (next_base_floor, next_quote_floor) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let base = lazy_bid_execute::token_amount(base_vault)?;
    let quote = lazy_bid_execute::token_amount(quote_vault)?;
    let (next_base, next_quote) = if is_bid {
        (base.checked_add(base_fill), quote.checked_sub(quote_amount))
    } else {
        (base.checked_sub(base_fill), quote.checked_add(quote_amount))
    };
    if next_base.ok_or(ProgramError::InsufficientFunds)? < next_base_floor
        || next_quote.ok_or(ProgramError::InsufficientFunds)? < next_quote_floor
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
    if is_bid {
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
            (quote_amount, custody.quote_decimals),
            Some(seeds),
        )?;
    } else {
        transfer_checked(
            taker_quote,
            quote_mint,
            quote_vault,
            taker,
            token_program,
            (quote_amount, custody.quote_decimals),
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
    }

    market
        .encode_into(&mut market_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    match updates {
        PageUpdates::Bid(pages, owners, updated_balances) => {
            store_multipage_bid_updates(page_0, owner_0, page_1, owner_1, &pages, &owners)?;
            for (account, state) in balances.iter().zip(updated_balances.iter()) {
                store_maker_balance(account, state)?;
            }
        }
        PageUpdates::Ask(pages, owners) => {
            store_multipage_ask_updates(page_0, owner_0, page_1, owner_1, &pages, &owners)?;
        }
    }
    store_custody(custody_account, &custody)?;
    reserve
        .encode(&mut reserve_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}
