#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::AccountInfo,
    declare_id,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

declare_id!("US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx");

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    match data.first().copied() {
        Some(0) => Ok(()),
        Some(1) => {
            let state = hybrid_engine::PassiveState {
                sqrt_price_x64: hybrid_engine::Q64,
                liquidity: 1_000_000,
            };
            hybrid_engine::quote_quote_in_for_base_out(state, 10_000)
                .map(|_| ())
                .map_err(|_| ProgramError::InvalidInstructionData)
        }
        Some(2) => {
            let market = hybrid_engine::HybridMarket {
                passive: hybrid_engine::PassiveState {
                    sqrt_price_x64: hybrid_engine::Q64,
                    liquidity: 1_000_000,
                },
                best_ask: Some(hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::Q64,
                    sqrt_price_x64: hybrid_engine::Q64,
                    base_qty: 5_000,
                }),
            };
            hybrid_engine::quote_buy_exact_in(market, 10_000)
                .map(|_| ())
                .map_err(|_| ProgramError::InvalidInstructionData)
        }
        Some(3) => {
            let step = hybrid_engine::Q64 / 200;
            let ask_1_sqrt = hybrid_engine::Q64;
            let ask_2_sqrt = hybrid_engine::Q64 + step * 2;
            let asks = [
                hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::spot_price_x64(ask_1_sqrt)
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                    sqrt_price_x64: ask_1_sqrt,
                    base_qty: 1_000,
                },
                hybrid_engine::LimitAsk {
                    price_x64: hybrid_engine::spot_price_x64(ask_2_sqrt)
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                    sqrt_price_x64: ask_2_sqrt,
                    base_qty: 2_000,
                },
            ];
            let boundaries = [
                hybrid_engine::PassiveBoundary {
                    sqrt_price_x64: hybrid_engine::Q64 + step,
                    liquidity_after: 1_500_000,
                },
                hybrid_engine::PassiveBoundary {
                    sqrt_price_x64: hybrid_engine::Q64 + step * 3,
                    liquidity_after: 2_000_000,
                },
            ];
            hybrid_engine::quote_buy_exact_in_levels(
                hybrid_engine::PassiveState {
                    sqrt_price_x64: hybrid_engine::Q64,
                    liquidity: 1_000_000,
                },
                &asks,
                &boundaries,
                20_000,
            )
            .map(|_| ())
            .map_err(|_| ProgramError::InvalidInstructionData)
        }

        Some(4) => process_state_backed_match(_accounts),
        Some(5) => process_multipage_state_backed_match(_program_id, _accounts, data),
        Some(6) => process_place_ask(_program_id, _accounts, data),
        Some(7) => process_cancel_ask(_program_id, _accounts, data),
        Some(8) => process_init_ask_owner_page(_program_id, _accounts, data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

#[inline(never)]
fn process_state_backed_match(accounts: &[AccountInfo]) -> ProgramResult {
    if accounts.len() != 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let market_data = accounts[0]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let mut ask_pages = Vec::with_capacity(1);
    decode_and_push_ask_page(&accounts[1], &mut ask_pages)?;
    hybrid_state::validate_ask_chain(&ask_pages).map_err(|_| ProgramError::InvalidAccountData)?;

    let mut boundary_pages = Vec::with_capacity(1);
    decode_and_push_boundary_page(&accounts[2], &mut boundary_pages)?;
    hybrid_state::validate_boundary_chain(&boundary_pages)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    let asks = ask_pages[0]
        .as_slice()
        .iter()
        .copied()
        .map(hybrid_state::AskEntry::as_limit_ask)
        .collect::<Vec<_>>();
    let boundaries = boundary_pages[0]
        .as_slice()
        .iter()
        .copied()
        .map(hybrid_state::BoundaryEntry::as_passive_boundary)
        .collect::<Vec<_>>();

    hybrid_engine::quote_buy_exact_in_levels(
        hybrid_engine::PassiveState {
            sqrt_price_x64: market.sqrt_price_x64,
            liquidity: market.liquidity,
        },
        &asks,
        &boundaries,
        20_000,
    )
    .map(|_| ())
    .map_err(|_| ProgramError::InvalidInstructionData)
}

#[inline(never)]
fn decode_and_push_ask_page(
    account: &AccountInfo,
    pages: &mut Vec<hybrid_state::AskPage>,
) -> ProgramResult {
    let data = account
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let page =
        hybrid_state::AskPage::decode_from(&data).map_err(|_| ProgramError::InvalidAccountData)?;
    pages.push(page);
    Ok(())
}

#[inline(never)]
fn decode_and_push_boundary_page(
    account: &AccountInfo,
    pages: &mut Vec<hybrid_state::BoundaryPage>,
) -> ProgramResult {
    let data = account
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let page = hybrid_state::BoundaryPage::decode_from(&data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pages.push(page);
    Ok(())
}

#[inline(never)]
fn process_multipage_state_backed_match(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 3 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let ask_page_count = usize::from(data[1]);
    let boundary_page_count = usize::from(data[2]);
    if ask_page_count == 0
        || boundary_page_count == 0
        || ask_page_count > 2
        || boundary_page_count > 2
    {
        return Err(ProgramError::InvalidInstructionData);
    }

    let expected_accounts = 1usize
        .checked_add(ask_page_count)
        .and_then(|count| count.checked_add(boundary_page_count))
        .ok_or(ProgramError::InvalidInstructionData)?;
    if accounts.len() != expected_accounts {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if accounts.iter().any(|account| account.owner != program_id) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let market_data = accounts[0]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let market_key = accounts[0].key;
    for (index, account) in accounts[1..1 + ask_page_count].iter().enumerate() {
        let index_bytes = (index as u32).to_le_bytes();
        let (expected, _) = Pubkey::find_program_address(
            &[b"ask-page", market_key.as_ref(), &index_bytes],
            program_id,
        );
        if *account.key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
    }

    let boundary_start = 1 + ask_page_count;
    for (index, account) in accounts[boundary_start..].iter().enumerate() {
        let index_bytes = (index as u32).to_le_bytes();
        let (expected, _) = Pubkey::find_program_address(
            &[b"boundary-page", market_key.as_ref(), &index_bytes],
            program_id,
        );
        if *account.key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
    }

    let mut ask_pages = Vec::with_capacity(ask_page_count);
    for account in &accounts[1..1 + ask_page_count] {
        decode_and_push_ask_page(account, &mut ask_pages)?;
    }
    hybrid_state::validate_ask_chain(&ask_pages).map_err(|_| ProgramError::InvalidAccountData)?;
    let ask_count = ask_pages
        .iter()
        .map(hybrid_state::AskPage::len)
        .sum::<usize>();
    if ask_count > hybrid_state::ASKS_PER_PAGE / 2 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut boundary_pages = Vec::with_capacity(boundary_page_count);
    for account in &accounts[boundary_start..] {
        decode_and_push_boundary_page(account, &mut boundary_pages)?;
    }
    hybrid_state::validate_boundary_chain(&boundary_pages)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let boundary_count = boundary_pages
        .iter()
        .map(hybrid_state::BoundaryPage::len)
        .sum::<usize>();
    if boundary_count > hybrid_state::BOUNDARIES_PER_PAGE / 2 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut asks = Vec::with_capacity(ask_count);
    for page in &ask_pages {
        asks.extend(
            page.as_slice()
                .iter()
                .copied()
                .map(hybrid_state::AskEntry::as_limit_ask),
        );
    }

    let mut boundaries = Vec::with_capacity(boundary_count);
    for page in &boundary_pages {
        boundaries.extend(
            page.as_slice()
                .iter()
                .copied()
                .map(hybrid_state::BoundaryEntry::as_passive_boundary),
        );
    }

    hybrid_engine::quote_buy_exact_in_levels(
        hybrid_engine::PassiveState {
            sqrt_price_x64: market.sqrt_price_x64,
            liquidity: market.liquidity,
        },
        &asks,
        &boundaries,
        20_000,
    )
    .map(|_| ())
    .map_err(|_| ProgramError::InvalidInstructionData)
}

fn require_active_order_accounts(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> Result<[u8; 32], ProgramError> {
    if accounts.len() != 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[3].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if accounts[..3]
        .iter()
        .any(|account| account.owner != program_id)
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if accounts[..3].iter().any(|account| !account.is_writable) {
        return Err(ProgramError::InvalidAccountData);
    }

    let market_key = accounts[0].key;
    let page_index = 0u32.to_le_bytes();
    let (expected_ask, _) =
        Pubkey::find_program_address(&[b"ask-page", market_key.as_ref(), &page_index], program_id);
    if *accounts[1].key != expected_ask {
        return Err(ProgramError::InvalidSeeds);
    }
    let (expected_owner, _) = Pubkey::find_program_address(
        &[b"ask-owner-page", market_key.as_ref(), &page_index],
        program_id,
    );
    if *accounts[2].key != expected_owner {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(accounts[3].key.to_bytes())
}

fn validate_owner_page_bytes(data: &[u8], asks: &hybrid_state::AskPage) -> ProgramResult {
    if data.len() != hybrid_state::ASK_OWNER_PAGE_BYTES {
        return Err(ProgramError::InvalidAccountData);
    }
    let len = usize::from(u16::from_le_bytes([data[0], data[1]]));
    if len != asks.len() || data[2..16] != asks.reserved {
        return Err(ProgramError::InvalidAccountData);
    }

    for index in 0..hybrid_state::ASKS_PER_PAGE {
        let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
        let zero = data[start..start + hybrid_state::ASK_OWNER_BYTES]
            .iter()
            .all(|byte| *byte == 0);
        if (index < len && zero) || (index >= len && !zero) {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    Ok(())
}

fn insert_owner_bytes(data: &mut [u8], index: usize, old_len: usize, owner: [u8; 32]) {
    let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
    let end = hybrid_state::PAGE_HEADER_BYTES + old_len * hybrid_state::ASK_OWNER_BYTES;
    data.copy_within(start..end, start + hybrid_state::ASK_OWNER_BYTES);
    data[start..start + hybrid_state::ASK_OWNER_BYTES].copy_from_slice(&owner);
    data[..2].copy_from_slice(&((old_len + 1) as u16).to_le_bytes());
}

fn remove_owner_bytes(data: &mut [u8], index: usize, old_len: usize) {
    let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
    let source = start + hybrid_state::ASK_OWNER_BYTES;
    let end = hybrid_state::PAGE_HEADER_BYTES + old_len * hybrid_state::ASK_OWNER_BYTES;
    data.copy_within(source..end, start);
    let tail = hybrid_state::PAGE_HEADER_BYTES + (old_len - 1) * hybrid_state::ASK_OWNER_BYTES;
    data[tail..tail + hybrid_state::ASK_OWNER_BYTES].fill(0);
    data[..2].copy_from_slice(&((old_len - 1) as u16).to_le_bytes());
}

#[inline(never)]
fn load_active_order_state(
    accounts: &[AccountInfo],
) -> Result<(hybrid_state::MarketHeader, Vec<hybrid_state::AskPage>), ProgramError> {
    let market_data = accounts[0]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let mut ask_pages = Vec::with_capacity(1);
    decode_and_push_ask_page(&accounts[1], &mut ask_pages)?;
    hybrid_state::validate_ask_chain(&ask_pages).map_err(|_| ProgramError::InvalidAccountData)?;

    let owner_data = accounts[2]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    validate_owner_page_bytes(&owner_data, &ask_pages[0])?;
    drop(owner_data);

    if usize::try_from(market.ask_count).map_err(|_| ProgramError::InvalidAccountData)?
        != ask_pages[0].len()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok((market, ask_pages))
}

fn store_active_order_state(
    accounts: &[AccountInfo],
    market: &hybrid_state::MarketHeader,
    asks: &hybrid_state::AskPage,
) -> ProgramResult {
    {
        let mut data = accounts[0]
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        market
            .encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut data = accounts[1]
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        asks.encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    Ok(())
}

#[inline(never)]
fn process_place_ask(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 41 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let owner = require_active_order_accounts(program_id, accounts)?;
    let price_x64 = u128::from_le_bytes(
        data[1..17]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let sqrt_price_x64 = u128::from_le_bytes(
        data[17..33]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let base_qty = u64::from_le_bytes(
        data[33..41]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );

    let (mut market, mut ask_pages) = load_active_order_state(accounts)?;
    let sequence = market.next_sequence;
    if sequence == 0 || base_qty == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let next_sequence = sequence
        .checked_add(1)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let next_count = market
        .ask_count
        .checked_add(1)
        .ok_or(ProgramError::InvalidInstructionData)?;

    let old_len = ask_pages[0].len();
    let index = ask_pages[0]
        .insert(hybrid_state::AskEntry {
            price_x64,
            sqrt_price_x64,
            base_qty,
            sequence,
        })
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    {
        let mut owner_data = accounts[2]
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        insert_owner_bytes(&mut owner_data, index, old_len, owner);
        validate_owner_page_bytes(&owner_data, &ask_pages[0])?;
    }

    market.next_sequence = next_sequence;
    market.ask_count = next_count;
    store_active_order_state(accounts, &market, &ask_pages[0])
}

#[inline(never)]
fn process_cancel_ask(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 9 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let owner = require_active_order_accounts(program_id, accounts)?;
    let sequence = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if sequence == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let (mut market, mut ask_pages) = load_active_order_state(accounts)?;
    let index = ask_pages[0]
        .as_slice()
        .iter()
        .position(|entry| entry.sequence == sequence)
        .ok_or(ProgramError::InvalidInstructionData)?;
    {
        let owner_data = accounts[2]
            .try_borrow_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
        if owner_data[start..start + hybrid_state::ASK_OWNER_BYTES] != owner {
            return Err(ProgramError::InvalidArgument);
        }
    }

    let old_len = ask_pages[0].len();
    ask_pages[0]
        .remove_by_sequence(sequence)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    {
        let mut owner_data = accounts[2]
            .try_borrow_mut_data()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        remove_owner_bytes(&mut owner_data, index, old_len);
        validate_owner_page_bytes(&owner_data, &ask_pages[0])?;
    }

    market.ask_count = market
        .ask_count
        .checked_sub(1)
        .ok_or(ProgramError::InvalidAccountData)?;
    store_active_order_state(accounts, &market, &ask_pages[0])
}

#[inline(never)]
fn process_init_ask_owner_page(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 1 || accounts.len() != 4 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let market = &accounts[0];
    let owner_page = &accounts[1];
    let payer = &accounts[2];
    let system_program = &accounts[3];

    if market.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !payer.is_signer || !payer.is_writable || !owner_page.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *system_program.key != Pubkey::default() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let market_data = market
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let market_header = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);
    if market_header.ask_count != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let index_bytes = 0u32.to_le_bytes();
    let (expected, bump) = Pubkey::find_program_address(
        &[b"ask-owner-page", market.key.as_ref(), &index_bytes],
        program_id,
    );
    if *owner_page.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    if owner_page.lamports() != 0 || !owner_page.data_is_empty() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(hybrid_state::ASK_OWNER_PAGE_BYTES);
    let mut create_data = Vec::with_capacity(52);
    create_data.extend_from_slice(&0u32.to_le_bytes());
    create_data.extend_from_slice(&lamports.to_le_bytes());
    create_data.extend_from_slice(&(hybrid_state::ASK_OWNER_PAGE_BYTES as u64).to_le_bytes());
    create_data.extend_from_slice(program_id.as_ref());
    let create = Instruction {
        program_id: Pubkey::default(),
        accounts: vec![
            AccountMeta::new(*payer.key, true),
            AccountMeta::new(*owner_page.key, true),
        ],
        data: create_data,
    };
    invoke_signed(
        &create,
        &[payer.clone(), owner_page.clone(), system_program.clone()],
        &[&[
            b"ask-owner-page",
            market.key.as_ref(),
            &index_bytes,
            &[bump],
        ]],
    )?;

    let owner_data = owner_page
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    if owner_data.len() != hybrid_state::ASK_OWNER_PAGE_BYTES
        || owner_data.iter().any(|byte| *byte != 0)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}
