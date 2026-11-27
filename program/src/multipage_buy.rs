use super::*;

const MULTIPAGE_BUY_FIXED: usize = 9;

#[derive(Default)]
pub(super) struct BuyScratch {
    pages: Vec<hybrid_state::AskPage>,
    sidecars: Vec<hybrid_state::AskOwnerPage>,
    flat: Vec<hybrid_engine::LimitAsk>,
    owners_flat: Vec<[u8; 32]>,
    entries: Vec<(hybrid_state::AskEntry, [u8; 32])>,
    makers: Vec<hybrid_state::MakerBalance>,
}

/// Opcodes 23 and 24: bounded linked-book exact-in and exact-out buys.
/// ABI: [opcode, page_count, amount:u64 LE, limit:u64 LE].
/// Accounts: market, custody, taker, taker-quote, taker-base,
/// quote-vault, base-vault, vault-authority, token-program,
/// (ask-page, owner-sidecar) * page_count, then maker-balance PDAs.
#[inline(never)]
pub(super) fn process_multipage_buy(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    process_multipage_buy_bounded(
        program_id,
        accounts,
        data,
        None,
        &mut BuyScratch::default(),
    )
    .map(|_| ())
}

#[inline(never)]
pub(super) fn process_multipage_buy_bounded(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
    tick: Option<u128>,
    scratch: &mut BuyScratch,
) -> Result<usize, ProgramError> {
    if data.len() != 18 || (data[0] != 23 && data[0] != 24) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let count = usize::from(data[1]);
    if !(1..=8).contains(&count) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let maker_start = MULTIPAGE_BUY_FIXED + 2 * count;
    if accounts.len() <= maker_start || accounts.len() > maker_start + MAX_SWAP_MAKER_ACCOUNTS {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let amount = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let limit = u64::from_le_bytes(
        data[10..18]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if amount == 0 || limit == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market_account = &accounts[0];
    let custody_account = &accounts[1];
    let taker = &accounts[2];
    let taker_quote = &accounts[3];
    let taker_base = &accounts[4];
    let quote_vault = &accounts[5];
    let base_vault = &accounts[6];
    let vault_authority = &accounts[7];
    let token_program = &accounts[8];
    let maker_accounts = &accounts[maker_start..];
    if !taker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if accounts[..maker_start]
        .iter()
        .enumerate()
        .any(|(i, a)| i != 2 && i != 7 && i != 8 && !a.is_writable)
        || maker_accounts.iter().any(|a| !a.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    if *market_account.owner != *program_id || *token_program.key != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    // Reject aliases across all state and token accounts.
    for (i, a) in accounts.iter().enumerate() {
        if accounts[i + 1..].iter().any(|b| a.key == b.key) {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    let mut market = hybrid_state::MarketHeader::decode_from(&market_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let BuyScratch {
        pages,
        sidecars,
        flat,
        owners_flat,
        entries,
        makers,
    } = scratch;
    pages.clear();
    sidecars.clear();
    flat.clear();
    owners_flat.clear();
    makers.clear();
    pages.resize_with(count, hybrid_state::AskPage::default);
    sidecars.resize_with(count, hybrid_state::AskOwnerPage::default);
    let order_count = market.ask_count as usize;
    if order_count > count * hybrid_state::ASKS_PER_PAGE {
        return Err(ProgramError::InvalidAccountData);
    }
    flat.reserve_exact(order_count);
    owners_flat.reserve_exact(order_count);
    makers.reserve_exact(maker_accounts.len());
    for index in 0..count {
        let page_account = &accounts[MULTIPAGE_BUY_FIXED + 2 * index];
        let owner_account = &accounts[MULTIPAGE_BUY_FIXED + 2 * index + 1];
        if page_account.owner != program_id || owner_account.owner != program_id {
            return Err(ProgramError::IncorrectProgramId);
        }
        let index_bytes = (index as u32).to_le_bytes();
        let (expected_page, _) = Pubkey::find_program_address(
            &[b"ask-page", market_account.key.as_ref(), &index_bytes],
            program_id,
        );
        // Head sidecar is authenticated by the market header. Every later
        // sidecar must be a canonical market/index PDA, never an arbitrary
        // program-owned sidecar with compatible link metadata.
        let valid_owner = if index == 0 {
            market.reserved2 == owner_account.key.to_bytes()
        } else {
            let (expected_owner, _) = Pubkey::find_program_address(
                &[b"ask-owner-page", market_account.key.as_ref(), &index_bytes],
                program_id,
            );
            *owner_account.key == expected_owner
        };
        if *page_account.key != expected_page || !valid_owner {
            return Err(ProgramError::InvalidSeeds);
        }
        let page = &mut pages[index];
        let owner = &mut sidecars[index];
        hybrid_state::AskPage::decode_into(&page_account.try_borrow_data()?, page)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        hybrid_state::AskOwnerPage::decode_into(&owner_account.try_borrow_data()?, owner)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        owner
            .validate_parallel(page)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        flat.extend(
            page.as_slice()
                .iter()
                .copied()
                .map(hybrid_state::AskEntry::as_limit_ask),
        );
        owners_flat.extend(owner.as_slice().iter().copied());
    }
    hybrid_state::validate_ask_chain(pages).map_err(|_| ProgramError::InvalidAccountData)?;
    if flat.len() != market.ask_count as usize {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut custody = load_custody(program_id, market_account, custody_account)?;
    let (expected_authority, authority_bump) = Pubkey::find_program_address(
        &[b"vault-authority", market_account.key.as_ref()],
        program_id,
    );
    if *vault_authority.key != expected_authority
        || quote_vault.key.to_bytes() != custody.quote_vault
        || base_vault.key.to_bytes() != custody.base_vault
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let (qm, qo) = token_account_fields(taker_quote)?;
    let (bm, bo) = token_account_fields(taker_base)?;
    let (vqm, vqo) = token_account_fields(quote_vault)?;
    let (vbm, vbo) = token_account_fields(base_vault)?;
    if qm != custody.quote_mint
        || bm != custody.base_mint
        || vqm != custody.quote_mint
        || vbm != custody.base_mint
        || qo != taker.key.to_bytes()
        || bo != taker.key.to_bytes()
        || vqo != expected_authority.to_bytes()
        || vbo != expected_authority.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    // The cached ask tick is the canonical floor root. Use the upper
    // edge only for eligibility, restrict fills to this tick, and settle
    // at the original exact order price (never the synthetic spot price).
    let (plan_tick, eligible) = if let Some(tick) = tick {
        if flat
            .first()
            .is_none_or(|entry| entry.sqrt_price_x64 != tick)
        {
            return Err(ProgramError::InvalidInstructionData);
        }
        let end = flat.partition_point(|entry| entry.sqrt_price_x64 == tick);
        (tick.checked_add(1).ok_or(ProgramError::InvalidInstructionData)?, &flat[..end])
    } else {
        (market.sqrt_price_x64, flat.as_slice())
    };
    let plan = plan_active_buy(
        if data[0] == 23 { 15 } else { 16 },
        plan_tick,
        eligible,
        amount,
        limit,
    )?;
    let fills = &plan.fills[..usize::from(plan.fill_count)];
    for account in maker_accounts {
        let state = hybrid_state::MakerBalance::decode_from(&account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        makers.push(load_maker_balance_for_owner(
            program_id,
            market_account,
            account,
            state.owner,
        )?);
    }
    for fill in fills {
        let owner = owners_flat[usize::from(fill.ask_index)];
        let maker_index = makers
            .iter()
            .position(|state| state.owner == owner)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        makers[maker_index]
            .settle_ask(fill.base_qty, fill.quote_qty)
            .map_err(|_| ProgramError::InsufficientFunds)?;
    }
    let removed =
        hybrid_state::multipage_ask::compact_linked_ask_fills(pages, sidecars, fills, entries)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    market.ask_count = market
        .ask_count
        .checked_sub(u32::try_from(removed).map_err(|_| ProgramError::InvalidAccountData)?)
        .ok_or(ProgramError::InvalidAccountData)?;
    custody.total_quote = custody
        .total_quote
        .checked_add(plan.amount_in)
        .ok_or(ProgramError::InvalidAccountData)?;
    custody.total_base = custody
        .total_base
        .checked_sub(plan.amount_out)
        .ok_or(ProgramError::InsufficientFunds)?;
    transfer_tokens(
        taker_quote,
        quote_vault,
        taker,
        token_program,
        plan.amount_in,
        None,
    )?;
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market_account.key.as_ref(), &bump];
    transfer_tokens(
        base_vault,
        taker_base,
        vault_authority,
        token_program,
        plan.amount_out,
        Some(seeds),
    )?;
    market
        .encode_into(&mut market_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    store_ask_page_updates(accounts, count, pages, sidecars)?;
    for (account, state) in maker_accounts.iter().zip(makers.iter()) {
        store_maker_balance(account, state)?;
    }
    store_custody(custody_account, &custody)?;
    Ok(pages.len())
}

/// Isolate large page/owner-page encode temporaries from the settlement
/// function's SBF stack frame; write both only after settlement validation.
#[inline(never)]
fn store_ask_page_updates(
    accounts: &[AccountInfo],
    count: usize,
    next_pages: &[hybrid_state::AskPage],
    next_sidecars: &[hybrid_state::AskOwnerPage],
) -> ProgramResult {
    for index in 0..count {
        store_ask_data_page(
            &accounts[MULTIPAGE_BUY_FIXED + 2 * index],
            index,
            next_pages.get(index),
        )?;
        store_ask_owner_page(
            &accounts[MULTIPAGE_BUY_FIXED + 2 * index + 1],
            index,
            next_sidecars.get(index),
        )?;
    }
    Ok(())
}

#[inline(never)]
pub(super) fn store_ask_data_page(
    account: &AccountInfo,
    index: usize,
    next: Option<&hybrid_state::AskPage>,
) -> ProgramResult {
    let mut page = next.copied().unwrap_or_default();
    if next.is_none() {
        page.set_links(hybrid_state::PageLinks::new(index as u32, None, None));
    }
    page.encode_into(&mut account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}

#[inline(never)]
pub(super) fn store_ask_owner_page(
    account: &AccountInfo,
    index: usize,
    next: Option<&hybrid_state::AskOwnerPage>,
) -> ProgramResult {
    let mut owner = next.copied().unwrap_or_default();
    if next.is_none() {
        owner.set_links(hybrid_state::PageLinks::new(index as u32, None, None));
    }
    owner
        .encode_into(&mut account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}
