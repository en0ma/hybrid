use super::*;

/// Opcode 27: close a DETACHED, EMPTY non-head linked-book page and its owner
/// sidecar. Rent flows into the authenticated market account, never to an
/// arbitrary keeper, because historical page funding ownership is not stored.
/// Accounts: market(w), page(w), owner-sidecar(w).
/// Data: [27, side (0 ask / 1 bid), page_index:u32 LE].
///
/// Opcode 28: recreate canonical detached page index 1 without attaching.
/// Accounts: market(w), head-page(w), head-owner(w), page-1(w),
/// page-1-owner(w), payer(signer,w), system-program.
/// Data: [28, side (0 ask / 1 bid)].
/// It does NOT add orders or refund rent to a historical payer.
#[inline(never)]
pub(super) fn process_page_lifecycle(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 2 || data[1] > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    match data[0] {
        27 => close_detached(program_id, accounts, data),
        28 => recreate_tail(program_id, accounts, data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn page_seed(side: u8) -> &'static [u8] {
    if side == 0 {
        b"ask-page"
    } else {
        b"bid-page"
    }
}
fn sidecar_seed(side: u8) -> &'static [u8] {
    if side == 0 {
        b"ask-owner-page"
    } else {
        b"bid-owner-page"
    }
}
fn canonical(
    program_id: &Pubkey,
    market: &Pubkey,
    side: u8,
    index: u32,
) -> ((Pubkey, u8), (Pubkey, u8)) {
    let n = index.to_le_bytes();
    (
        Pubkey::find_program_address(&[page_seed(side), market.as_ref(), &n], program_id),
        Pubkey::find_program_address(&[sidecar_seed(side), market.as_ref(), &n], program_id),
    )
}
fn validate_empty_detached(
    side: u8,
    index: u32,
    page: &AccountInfo,
    owner: &AccountInfo,
) -> ProgramResult {
    let detached = hybrid_state::PageLinks::new(index, None, None);
    if side == 0 {
        let p = load_active_ask_page(page)?;
        let o = load_owner_page_box(owner)?;
        if !p.is_empty() || !o.is_empty() || p.links() != detached || o.links() != detached {
            return Err(ProgramError::InvalidAccountData);
        }
    } else {
        let p = load_active_bid_page(page)?;
        let mut o = Box::new(hybrid_state::BidOwnerPage::default());
        hybrid_state::BidOwnerPage::decode_into(&owner.try_borrow_data()?, &mut o)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if !p.is_empty() || !o.is_empty() || p.links() != detached || o.links() != detached {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    Ok(())
}

fn close_detached(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 6 || accounts.len() != 3 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let side = data[1];
    let index = u32::from_le_bytes(
        data[2..6]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if index == 0 || index > 7 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let page = &accounts[1];
    let owners = &accounts[2];
    if market.owner != program_id
        || page.owner != program_id
        || owners.owner != program_id
        || !market.is_writable
        || !page.is_writable
        || !owners.is_writable
        || market.key == page.key
        || market.key == owners.key
        || page.key == owners.key
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !header.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let ((expected_page, _), (expected_owner, _)) = canonical(program_id, market.key, side, index);
    if *page.key != expected_page || *owners.key != expected_owner {
        return Err(ProgramError::InvalidSeeds);
    }
    validate_empty_detached(side, index, page, owners)?;
    // Neither account may be trusted merely because it is empty: require
    // its canonical PDA and a detached, canonical page index.
    let credit = page
        .lamports()
        .checked_add(owners.lamports())
        .ok_or(ProgramError::InvalidAccountData)?;
    let next = market
        .lamports()
        .checked_add(credit)
        .ok_or(ProgramError::InvalidAccountData)?;
    **market.try_borrow_mut_lamports()? = next;
    **page.try_borrow_mut_lamports()? = 0;
    **owners.try_borrow_mut_lamports()? = 0;
    page.resize(0)?;
    owners.resize(0)?;
    page.assign(&Pubkey::default());
    owners.assign(&Pubkey::default());
    Ok(())
}

#[inline(never)]
fn recreate_tail(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 2 || accounts.len() != 7 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let side = data[1];
    let market = &accounts[0];
    let head = &accounts[1];
    let head_owner = &accounts[2];
    let tail = &accounts[3];
    let tail_owner = &accounts[4];
    let payer = &accounts[5];
    let system = &accounts[6];
    if market.owner != program_id
        || head.owner != program_id
        || head_owner.owner != program_id
        || !market.is_writable
        || !payer.is_writable
        || !payer.is_signer
        || !tail.is_writable
        || !tail_owner.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }
    for (i, account) in accounts.iter().enumerate() {
        if accounts[i + 1..]
            .iter()
            .any(|other| account.key == other.key)
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    let header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !header.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let ((expected_head, _), _) = canonical(program_id, market.key, side, 0);
    let ((expected_tail, page_bump), (expected_owner, owner_bump)) =
        canonical(program_id, market.key, side, 1);
    if *head.key != expected_head || *tail.key != expected_tail || *tail_owner.key != expected_owner
    {
        return Err(ProgramError::InvalidSeeds);
    }
    // Only detached canonical page PDAs can be recreated. Do not link the
    // empty page into the head: legacy maker cancellation requires next=None.
    if side == 0 {
        if header.reserved2 != head_owner.key.to_bytes() {
            return Err(ProgramError::InvalidSeeds);
        }
        let page = load_active_ask_page(head)?;
        let owner = load_owner_page_box(head_owner)?;
        owner
            .validate_parallel(&page)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if page.links() != hybrid_state::PageLinks::new(0, None, None)
            || header.ask_count as usize != page.len()
        {
            return Err(ProgramError::InvalidAccountData);
        }
    } else {
        if header.bid_owner_tag() != owner_tag(head_owner.key) {
            return Err(ProgramError::InvalidSeeds);
        }
        let page = load_active_bid_page(head)?;
        let mut owner = Box::new(hybrid_state::BidOwnerPage::default());
        hybrid_state::BidOwnerPage::decode_into(&head_owner.try_borrow_data()?, &mut owner)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if page.links() != hybrid_state::PageLinks::new(0, None, None)
            || owner.links() != page.links()
            || owner.len() != page.len()
            || header.bid_count() as usize != page.len()
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    let index = 1u32.to_le_bytes();
    let (page_space, owner_space) = if side == 0 {
        (
            hybrid_state::ASK_PAGE_BYTES,
            hybrid_state::ASK_OWNER_PAGE_BYTES,
        )
    } else {
        (
            hybrid_state::BID_PAGE_BYTES,
            hybrid_state::BID_OWNER_PAGE_BYTES,
        )
    };
    create_program_pda(
        program_id,
        payer,
        tail,
        system,
        &[page_seed(side), market.key.as_ref(), &index],
        page_bump,
        page_space,
    )?;
    create_program_pda(
        program_id,
        payer,
        tail_owner,
        system,
        &[sidecar_seed(side), market.key.as_ref(), &index],
        owner_bump,
        owner_space,
    )?;
    let links = hybrid_state::PageLinks::new(1, None, None);
    if side == 0 {
        let mut page = Box::new(hybrid_state::AskPage::default());
        let mut owner = Box::new(hybrid_state::AskOwnerPage::default());
        page.set_links(links);
        owner.set_links(links);
        page.encode_into(&mut tail.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        owner
            .encode_into(&mut tail_owner.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    } else {
        let mut page = Box::new(hybrid_state::BidPage::default());
        let mut owner = Box::new(hybrid_state::BidOwnerPage::default());
        page.set_links(links);
        owner.set_links(links);
        page.encode_into(&mut tail.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        owner
            .encode_into(&mut tail_owner.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    Ok(())
}
