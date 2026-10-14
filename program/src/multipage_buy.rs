use super::*;

const MULTIPAGE_BUY_FIXED: usize = 9;

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
    let mut pages = Vec::with_capacity(count);
    let mut sidecars = Vec::with_capacity(count);
    let mut flat = Vec::new();
    let mut owners_flat = Vec::new();
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
        let page = load_active_ask_page(page_account)?;
        let owner = load_owner_page_box(owner_account)?;
        owner
            .validate_parallel(&page)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        flat.extend(
            page.as_slice()
                .iter()
                .copied()
                .map(hybrid_state::AskEntry::as_limit_ask),
        );
        owners_flat.extend(owner.as_slice().iter().copied());
        pages.push(*page);
        sidecars.push(*owner);
    }
    hybrid_state::validate_ask_chain(&pages).map_err(|_| ProgramError::InvalidAccountData)?;
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
    let plan = plan_active_buy(
        if data[0] == 23 { 15 } else { 16 },
        market.sqrt_price_x64,
        &flat,
        amount,
        limit,
    )?;
    let fills = &plan.fills[..usize::from(plan.fill_count)];
    let mut maker_states = Vec::with_capacity(maker_accounts.len());
    for account in maker_accounts {
        let state = hybrid_state::MakerBalance::decode_from(&account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        maker_states.push(load_maker_balance_for_owner(
            program_id,
            market_account,
            account,
            state.owner,
        )?);
    }
    let mut settled = maker_states.clone();
    for fill in fills {
        let owner = owners_flat[usize::from(fill.ask_index)];
        let maker_index = maker_states
            .iter()
            .position(|state| state.owner == owner)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        settled[maker_index]
            .settle_ask(fill.base_qty, fill.quote_qty)
            .map_err(|_| ProgramError::InsufficientFunds)?;
    }
    let (mut next_pages, mut next_sidecars, removed) =
        hybrid_state::multipage_ask::apply_linked_ask_fills(&pages, &sidecars, fills)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    // Linked maker place/cancel (opcodes 30/31) address a fixed two-page chain.
    // Compaction can retire page 1 even for a partial fill, which would make
    // the maker's subsequent cancellation impossible. Keep the empty tail
    // linked for the two-page ABI; the generic state compactor remains free
    // to retire trailing pages for callers without this fixed-page contract.
    if count == 2 && next_pages.len() == 1 {
        restore_two_page_maker_chain(&mut next_pages, &mut next_sidecars);
    }
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
    store_ask_page_updates(accounts, count, &next_pages, &next_sidecars)?;
    for (account, state) in maker_accounts.iter().zip(settled.iter()) {
        store_maker_balance(account, state)?;
    }
    store_custody(custody_account, &custody)
}

#[inline(never)]
fn restore_two_page_maker_chain(
    pages: &mut Vec<hybrid_state::AskPage>,
    sidecars: &mut Vec<hybrid_state::AskOwnerPage>,
) {
    let head = hybrid_state::PageLinks::new(0, None, Some(1));
    let tail = hybrid_state::PageLinks::new(1, Some(0), None);
    pages[0].set_links(head);
    sidecars[0].set_links(head);
    let mut empty_tail = hybrid_state::AskPage::default();
    let mut empty_owners = hybrid_state::AskOwnerPage::default();
    empty_tail.set_links(tail);
    empty_owners.set_links(tail);
    pages.push(empty_tail);
    sidecars.push(empty_owners);
}

#[cfg(test)]
mod linked_maker_tests {
    use super::*;
    #[test]
    fn a_partially_filled_two_page_book_keeps_the_empty_tail_linked() {
        let mut pages = vec![hybrid_state::AskPage::default()];
        let mut owners = vec![hybrid_state::AskOwnerPage::default()];
        restore_two_page_maker_chain(&mut pages, &mut owners);
        assert_eq!(pages.len(), 2);
        assert_eq!(owners.len(), 2);
        assert_eq!(
            pages[0].links(),
            hybrid_state::PageLinks::new(0, None, Some(1))
        );
        assert_eq!(
            pages[1].links(),
            hybrid_state::PageLinks::new(1, Some(0), None)
        );
        assert_eq!(owners[0].links(), pages[0].links());
        assert_eq!(owners[1].links(), pages[1].links());
        hybrid_state::validate_ask_chain(&pages).unwrap();
    }
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
fn store_ask_data_page(
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
fn store_ask_owner_page(
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
