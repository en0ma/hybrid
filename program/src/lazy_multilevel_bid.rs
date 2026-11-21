use super::*;

/// Opcode 43: read-only, account-authenticated multilevel BID preflight.
///
/// Data: [43, page_count:u8, requested_base:u64LE, min_quote:u64LE].
/// Accounts: market, then exactly page_count ordered (BID page, owner
/// sidecar) pairs, starting with canonical page 0.
/// This opcode never writes, transfers tokens, seals epochs or authorizes
/// a maker payout. Its purpose is to validate the future execution ABI
/// against live program-owned accounts.
#[inline(never)]
pub(super) fn preflight_multilevel_bid(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 18 || data[0] != 43 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let count = usize::from(data[1]);
    if count == 0 || count > hybrid_state::lazy_multilevel_bid::MAX_PLANNED_BID_PAGES {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() != 1 + count * 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let requested_base = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let min_quote_out = u64::from_le_bytes(
        data[10..18]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if accounts.iter().any(|account| account.owner != program_id) {
        return Err(ProgramError::IncorrectProgramId);
    }
    // Reject aliased market/page/sidecar accounts before decoding any
    // program-owned data. No account in the chain may serve two roles.
    for (index, account) in accounts.iter().enumerate() {
        if accounts[index + 1..]
            .iter()
            .any(|other| other.key == account.key)
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    let market_account = &accounts[0];
    let market = hybrid_state::MarketHeader::decode_from(&market_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market.collateralized_active() || market.bid_count() == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut pages = Vec::with_capacity(count);
    let mut sidecars = Vec::with_capacity(count);
    let mut total = 0usize;
    for index in 0..count {
        let page = &accounts[1 + index * 2];
        let owners = &accounts[2 + index * 2];
        let index_bytes = (index as u32).to_le_bytes();
        let (page_pda, _) = Pubkey::find_program_address(
            &[b"bid-page", market_account.key.as_ref(), &index_bytes],
            program_id,
        );
        if *page.key != page_pda {
            return Err(ProgramError::InvalidSeeds);
        }
        // The head owner sidecar is authorized by its market-header tag.
        // All later owner sidecars must use canonical market/index PDAs.
        if index == 0 {
            if market.bid_owner_tag() != owner_tag(owners.key) {
                return Err(ProgramError::InvalidSeeds);
            }
        } else {
            let (owner_pda, _) = Pubkey::find_program_address(
                &[b"bid-owner-page", market_account.key.as_ref(), &index_bytes],
                program_id,
            );
            if *owners.key != owner_pda {
                return Err(ProgramError::InvalidSeeds);
            }
        }
        let book = load_active_bid_page(page)?;
        let sidecar = load_owner_page_box(owners)?;
        if book.len() != sidecar.len() || book.links() != sidecar.links() {
            return Err(ProgramError::InvalidAccountData);
        }
        total = total
            .checked_add(book.len())
            .ok_or(ProgramError::InvalidAccountData)?;
        pages.push(*book);
        sidecars.push(*sidecar);
    }
    if total != market.bid_count() as usize {
        return Err(ProgramError::InvalidAccountData);
    }
    let plan = hybrid_state::lazy_multilevel_bid::plan_multilevel_bid(
        &pages,
        &sidecars,
        requested_base,
        min_quote_out,
    )
    .map_err(|_| ProgramError::InvalidInstructionData)?;
    if plan.filled_base != requested_base || plan.quote_paid < min_quote_out {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(())
}
