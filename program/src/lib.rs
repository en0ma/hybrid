#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::AccountInfo,
    declare_id,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::{invoke, invoke_signed},
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
        Some(6) | Some(7) => process_active_order(_program_id, _accounts, data),
        Some(8) | Some(9) => process_active_bid_order(_program_id, _accounts, data),
        Some(10) => process_state_backed_plan(_accounts),
        Some(11) => process_init_custody(_program_id, _accounts, data),
        Some(12) => process_init_maker_balance(_program_id, _accounts),
        Some(13) => process_deposit(_program_id, _accounts, data),
        Some(14) => process_withdraw(_program_id, _accounts, data),
        Some(15) | Some(16) => process_buy_swap(_program_id, _accounts, data),
        Some(17) | Some(18) => process_sell_swap(_program_id, _accounts, data),
        Some(19) => process_init_passive_pool(_program_id, _accounts, data),
        Some(20) => process_open_passive_position(_program_id, _accounts, data),
        Some(21) => process_close_passive_position(_program_id, _accounts, data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

const TOKEN_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn token_account_fields(account: &AccountInfo) -> Result<([u8; 32], [u8; 32]), ProgramError> {
    if *account.owner != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    if data.len() < 165 || data[108] == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok((
        data[0..32]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
        data[32..64]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    ))
}

fn mint_decimals(account: &AccountInfo) -> Result<u8, ProgramError> {
    if *account.owner != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = account.try_borrow_data()?;
    if data.len() < 82 || data[45] == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(data[44])
}

fn create_program_pda<'a>(
    program_id: &Pubkey,
    payer: &AccountInfo<'a>,
    target: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    seeds: &[&[u8]],
    bump: u8,
    space: usize,
) -> ProgramResult {
    if *system_program.key != Pubkey::default() || !payer.is_signer || !payer.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if *target.owner != Pubkey::default() || !target.data_is_empty() || !target.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    let required_lamports = Rent::get()?.minimum_balance(space);
    let bump_bytes = [bump];
    let mut signer_seeds = Vec::with_capacity(seeds.len() + 1);
    signer_seeds.extend_from_slice(seeds);
    signer_seeds.push(&bump_bytes);

    if target.lamports() == 0 {
        let mut ix_data = Vec::with_capacity(52);
        ix_data.extend_from_slice(&0u32.to_le_bytes());
        ix_data.extend_from_slice(&required_lamports.to_le_bytes());
        ix_data.extend_from_slice(&(space as u64).to_le_bytes());
        ix_data.extend_from_slice(program_id.as_ref());
        let ix = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![
                AccountMeta::new(*payer.key, true),
                AccountMeta::new(*target.key, true),
            ],
            data: ix_data,
        };
        return invoke_signed(
            &ix,
            &[payer.clone(), target.clone(), system_program.clone()],
            &[&signer_seeds],
        );
    }

    let top_up = required_lamports.saturating_sub(target.lamports());
    if top_up > 0 {
        let mut transfer_data = Vec::with_capacity(12);
        transfer_data.extend_from_slice(&2u32.to_le_bytes());
        transfer_data.extend_from_slice(&top_up.to_le_bytes());
        let transfer = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![
                AccountMeta::new(*payer.key, true),
                AccountMeta::new(*target.key, false),
            ],
            data: transfer_data,
        };
        invoke(
            &transfer,
            &[payer.clone(), target.clone(), system_program.clone()],
        )?;
    }

    let mut allocate_data = Vec::with_capacity(12);
    allocate_data.extend_from_slice(&8u32.to_le_bytes());
    allocate_data.extend_from_slice(&(space as u64).to_le_bytes());
    let allocate = Instruction {
        program_id: Pubkey::default(),
        accounts: vec![AccountMeta::new(*target.key, true)],
        data: allocate_data,
    };
    invoke_signed(
        &allocate,
        &[target.clone(), system_program.clone()],
        &[&signer_seeds],
    )?;

    let mut assign_data = Vec::with_capacity(36);
    assign_data.extend_from_slice(&1u32.to_le_bytes());
    assign_data.extend_from_slice(program_id.as_ref());
    let assign = Instruction {
        program_id: Pubkey::default(),
        accounts: vec![AccountMeta::new(*target.key, true)],
        data: assign_data,
    };
    invoke_signed(
        &assign,
        &[target.clone(), system_program.clone()],
        &[&signer_seeds],
    )
}

fn transfer_checked<'a>(
    source: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    destination: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    amount_and_decimals: (u64, u8),
    signer_seeds: Option<&[&[u8]]>,
) -> ProgramResult {
    let (amount, decimals) = amount_and_decimals;
    if *token_program.key != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut data = Vec::with_capacity(10);
    data.push(12);
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);
    let ix = Instruction {
        program_id: TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new_readonly(*mint.key, false),
            AccountMeta::new(*destination.key, false),
            AccountMeta::new_readonly(*authority.key, signer_seeds.is_none()),
        ],
        data,
    };
    let infos = [
        source.clone(),
        mint.clone(),
        destination.clone(),
        authority.clone(),
        token_program.clone(),
    ];
    if let Some(seeds) = signer_seeds {
        invoke_signed(&ix, &infos, &[seeds])
    } else {
        invoke(&ix, &infos)
    }
}

fn transfer_tokens<'a>(
    source: &AccountInfo<'a>,
    destination: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    amount: u64,
    signer_seeds: Option<&[&[u8]]>,
) -> ProgramResult {
    if *token_program.key != TOKEN_PROGRAM_ID || amount == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut data = Vec::with_capacity(9);
    data.push(3);
    data.extend_from_slice(&amount.to_le_bytes());
    let ix = Instruction {
        program_id: TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*source.key, false),
            AccountMeta::new(*destination.key, false),
            AccountMeta::new_readonly(*authority.key, signer_seeds.is_none()),
        ],
        data,
    };
    let infos = [
        source.clone(),
        destination.clone(),
        authority.clone(),
        token_program.clone(),
    ];
    if let Some(seeds) = signer_seeds {
        invoke_signed(&ix, &infos, &[seeds])
    } else {
        invoke(&ix, &infos)
    }
}

fn load_custody(
    program_id: &Pubkey,
    market: &AccountInfo,
    custody: &AccountInfo,
) -> Result<hybrid_state::CustodyState, ProgramError> {
    if market.owner != program_id || custody.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (expected, _) =
        Pubkey::find_program_address(&[b"custody", market.key.as_ref()], program_id);
    if *custody.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let data = custody.try_borrow_data()?;
    let state = hybrid_state::CustodyState::decode_from(&data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if state.market != market.key.to_bytes() {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(state)
}

fn load_maker_balance(
    program_id: &Pubkey,
    market: &AccountInfo,
    balance: &AccountInfo,
    maker: &AccountInfo,
) -> Result<hybrid_state::MakerBalance, ProgramError> {
    if balance.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (expected, _) = Pubkey::find_program_address(
        &[b"maker-balance", market.key.as_ref(), maker.key.as_ref()],
        program_id,
    );
    if *balance.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let data = balance.try_borrow_data()?;
    let state = hybrid_state::MakerBalance::decode_from(&data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if state.market != market.key.to_bytes() || state.owner != maker.key.to_bytes() {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(state)
}

fn load_maker_balance_for_owner(
    program_id: &Pubkey,
    market: &AccountInfo,
    balance: &AccountInfo,
    owner: [u8; 32],
) -> Result<hybrid_state::MakerBalance, ProgramError> {
    if balance.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let owner_key = Pubkey::new_from_array(owner);
    let (expected, _) = Pubkey::find_program_address(
        &[b"maker-balance", market.key.as_ref(), owner_key.as_ref()],
        program_id,
    );
    if *balance.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let data = balance.try_borrow_data()?;
    let state = hybrid_state::MakerBalance::decode_from(&data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if state.market != market.key.to_bytes() || state.owner != owner {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(state)
}

fn store_maker_balance(
    account: &AccountInfo,
    balance: &hybrid_state::MakerBalance,
) -> ProgramResult {
    let mut data = account.try_borrow_mut_data()?;
    balance
        .encode_into(&mut data)
        .map_err(|_| ProgramError::InvalidAccountData)
}

fn store_custody(account: &AccountInfo, custody: &hybrid_state::CustodyState) -> ProgramResult {
    let mut data = account.try_borrow_mut_data()?;
    custody
        .encode_into(&mut data)
        .map_err(|_| ProgramError::InvalidAccountData)
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
    pages.push(hybrid_state::AskPage::default());
    hybrid_state::AskPage::decode_into(&data, pages.last_mut().unwrap())
        .map_err(|_| ProgramError::InvalidAccountData)?;
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

fn validate_owner_page_header(data: &[u8], len: usize, reserved: &[u8; 14]) -> ProgramResult {
    if data.len() != hybrid_state::ASK_OWNER_PAGE_BYTES {
        return Err(ProgramError::InvalidAccountData);
    }
    let owner_len = usize::from(u16::from_le_bytes([data[0], data[1]]));
    if owner_len != len || data[2..16] != *reserved {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

fn validate_owner_page_bytes(data: &[u8], asks: &hybrid_state::AskPage) -> ProgramResult {
    validate_owner_page_header(data, asks.len(), &asks.reserved)
}

fn validate_bid_owner_page_bytes(data: &[u8], bids: &hybrid_state::BidPage) -> ProgramResult {
    validate_owner_page_header(data, bids.len(), &bids.reserved)
}

fn owner_tag(key: &Pubkey) -> [u8; 16] {
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&key.as_ref()[..16]);
    tag
}

fn insert_slot(data: &mut [u8], index: usize, old_len: usize, width: usize) {
    let start = hybrid_state::PAGE_HEADER_BYTES + index * width;
    let end = hybrid_state::PAGE_HEADER_BYTES + old_len * width;
    data.copy_within(start..end, start + width);
    data[..2].copy_from_slice(&((old_len + 1) as u16).to_le_bytes());
}

fn remove_slot(data: &mut [u8], index: usize, old_len: usize, width: usize) {
    let start = hybrid_state::PAGE_HEADER_BYTES + index * width;
    let end = hybrid_state::PAGE_HEADER_BYTES + old_len * width;
    data.copy_within(start + width..end, start);
    let tail = hybrid_state::PAGE_HEADER_BYTES + (old_len - 1) * width;
    data[tail..tail + width].fill(0);
    data[..2].copy_from_slice(&((old_len - 1) as u16).to_le_bytes());
}

#[inline(never)]
fn load_active_ask_page(account: &AccountInfo) -> Result<Box<hybrid_state::AskPage>, ProgramError> {
    let data = account.try_borrow_data()?;
    let mut page = Box::new(hybrid_state::AskPage::default());
    hybrid_state::AskPage::decode_into(&data, &mut page)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(page)
}

#[inline(never)]
fn load_active_bid_page(account: &AccountInfo) -> Result<Box<hybrid_state::BidPage>, ProgramError> {
    let data = account.try_borrow_data()?;
    let mut page = Box::new(hybrid_state::BidPage::default());
    hybrid_state::BidPage::decode_into(&data, &mut page)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(page)
}

#[inline(never)]
fn process_active_order(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 4 && accounts.len() != 5 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let maker_index = accounts.len() - 1;
    if !accounts[maker_index].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if accounts[..maker_index]
        .iter()
        .any(|account| account.owner != program_id)
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if accounts[..maker_index]
        .iter()
        .any(|account| !account.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let index_bytes = 0u32.to_le_bytes();
    let (expected_ask, _) = Pubkey::find_program_address(
        &[b"ask-page", accounts[0].key.as_ref(), &index_bytes],
        program_id,
    );
    if *accounts[1].key != expected_ask {
        return Err(ProgramError::InvalidSeeds);
    }

    let market_data = accounts[0].try_borrow_data()?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let page = load_active_ask_page(&accounts[1])?;
    let links = page.links();
    if links.page_index != 0 || links.prev_page.is_some() || links.next_page.is_some() {
        return Err(ProgramError::InvalidAccountData);
    }

    let owner_data = accounts[2].try_borrow_data()?;
    let owner_page_uninitialized = market.reserved2 == [0; 32]
        && market.ask_count == 0
        && owner_data.iter().all(|byte| *byte == 0);
    if !owner_page_uninitialized {
        validate_owner_page_bytes(&owner_data, &page)?;
    }
    drop(owner_data);

    if market.ask_count as usize != page.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    let sidecar_key = accounts[2].key.to_bytes();
    if market.reserved2 != [0; 32] && market.reserved2 != sidecar_key {
        return Err(ProgramError::InvalidSeeds);
    }
    if market.reserved2 == [0; 32] && market.ask_count != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let collateralized = market.collateralized_active();
    if collateralized {
        if accounts.len() != 5 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
    } else {
        if accounts.len() != 4 || data.first().copied() != Some(7) {
            return Err(ProgramError::InvalidInstructionData);
        }
    }

    let mut maker_balance = if collateralized {
        Some(load_maker_balance(
            program_id,
            &accounts[0],
            &accounts[3],
            &accounts[4],
        )?)
    } else {
        None
    };

    match data.first().copied() {
        Some(6) => {
            if data.len() != 41 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let sequence = market.next_sequence;
            let base_qty = u64::from_le_bytes(
                data[33..41]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            );
            if sequence == 0 || base_qty == 0 || page.len() >= hybrid_state::ASKS_PER_PAGE {
                return Err(ProgramError::InvalidInstructionData);
            }
            let entry = hybrid_state::AskEntry {
                price_x64: u128::from_le_bytes(
                    data[1..17]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                ),
                sqrt_price_x64: u128::from_le_bytes(
                    data[17..33]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                ),
                base_qty,
                sequence,
            };
            hybrid_engine::validate_limit_ask(entry.as_limit_ask())
                .map_err(|_| ProgramError::InvalidInstructionData)?;
            let mut index = 0usize;
            while index < page.len() {
                let existing = page.entries[index];
                if existing.sequence == sequence {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if existing.price_x64 > entry.price_x64
                    || (existing.price_x64 == entry.price_x64
                        && existing.sequence >= entry.sequence)
                {
                    break;
                }
                index += 1;
            }
            let next_sequence = sequence
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let next_count = market
                .ask_count
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            maker_balance
                .as_mut()
                .ok_or(ProgramError::InvalidAccountData)?
                .lock_base(base_qty)
                .map_err(|_| ProgramError::InsufficientFunds)?;
            let old_len = page.len();

            {
                let mut ask_data = accounts[1].try_borrow_mut_data()?;
                insert_slot(&mut ask_data, index, old_len, hybrid_state::ASK_ENTRY_BYTES);
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_ENTRY_BYTES;
                ask_data[start..start + 16].copy_from_slice(&entry.price_x64.to_le_bytes());
                ask_data[start + 16..start + 32]
                    .copy_from_slice(&entry.sqrt_price_x64.to_le_bytes());
                ask_data[start + 32..start + 40].copy_from_slice(&entry.base_qty.to_le_bytes());
                ask_data[start + 40..start + 48].copy_from_slice(&entry.sequence.to_le_bytes());
            }
            {
                let mut owners = accounts[2].try_borrow_mut_data()?;
                if owner_page_uninitialized {
                    owners[2..16].copy_from_slice(&page.reserved);
                }
                insert_slot(&mut owners, index, old_len, hybrid_state::ASK_OWNER_BYTES);
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
                owners[start..start + hybrid_state::ASK_OWNER_BYTES]
                    .copy_from_slice(accounts[maker_index].key.as_ref());
            }
            {
                let mut market_bytes = accounts[0].try_borrow_mut_data()?;
                market_bytes[12..16].copy_from_slice(&next_count.to_le_bytes());
                market_bytes[32..40].copy_from_slice(&next_sequence.to_le_bytes());
                if market.reserved2 == [0; 32] {
                    market_bytes[96..128].copy_from_slice(&sidecar_key);
                }
            }
            store_maker_balance(
                &accounts[3],
                maker_balance
                    .as_ref()
                    .ok_or(ProgramError::InvalidAccountData)?,
            )
        }
        Some(7) => {
            if data.len() != 9 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let sequence = u64::from_le_bytes(
                data[1..9]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            );
            if sequence == 0 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let mut index = 0usize;
            while index < page.len() && page.entries[index].sequence != sequence {
                index += 1;
            }
            if index == page.len() {
                return Err(ProgramError::InvalidInstructionData);
            }
            {
                let owners = accounts[2]
                    .try_borrow_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
                if owners[start..start + hybrid_state::ASK_OWNER_BYTES]
                    != accounts[maker_index].key.to_bytes()
                {
                    return Err(ProgramError::InvalidArgument);
                }
            }
            let next_count = market
                .ask_count
                .checked_sub(1)
                .ok_or(ProgramError::InvalidAccountData)?;
            if let Some(balance) = maker_balance.as_mut() {
                balance
                    .unlock_base(page.entries[index].base_qty)
                    .map_err(|_| ProgramError::InvalidAccountData)?;
            }
            let old_len = page.len();

            {
                let mut ask_data = accounts[1].try_borrow_mut_data()?;
                remove_slot(&mut ask_data, index, old_len, hybrid_state::ASK_ENTRY_BYTES);
            }
            {
                let mut owners = accounts[2].try_borrow_mut_data()?;
                remove_slot(&mut owners, index, old_len, hybrid_state::ASK_OWNER_BYTES);
            }
            {
                let mut market_bytes = accounts[0].try_borrow_mut_data()?;
                market_bytes[12..16].copy_from_slice(&next_count.to_le_bytes());
            }
            if let Some(balance) = maker_balance.as_ref() {
                store_maker_balance(&accounts[3], balance)
            } else {
                Ok(())
            }
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

#[inline(never)]
fn process_active_bid_order(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 4 && accounts.len() != 5 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let maker_index = accounts.len() - 1;
    if !accounts[maker_index].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if accounts[..maker_index]
        .iter()
        .any(|account| account.owner != program_id)
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if accounts[..maker_index]
        .iter()
        .any(|account| !account.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let index_bytes = 0u32.to_le_bytes();
    let (expected_bid, _) = Pubkey::find_program_address(
        &[b"bid-page", accounts[0].key.as_ref(), &index_bytes],
        program_id,
    );
    if *accounts[1].key != expected_bid {
        return Err(ProgramError::InvalidSeeds);
    }

    let market_data = accounts[0].try_borrow_data()?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let page = load_active_bid_page(&accounts[1])?;
    let links = page.links();
    if links.page_index != 0 || links.prev_page.is_some() || links.next_page.is_some() {
        return Err(ProgramError::InvalidAccountData);
    }

    let bid_count = market.bid_count();
    let expected_tag = market.bid_owner_tag();
    let supplied_tag = owner_tag(accounts[2].key);
    let owner_data = accounts[2].try_borrow_data()?;
    let owner_page_uninitialized = expected_tag == [0; 16]
        && bid_count == 0
        && owner_data.len() == hybrid_state::BID_OWNER_PAGE_BYTES
        && owner_data.iter().all(|byte| *byte == 0);
    if !owner_page_uninitialized {
        validate_bid_owner_page_bytes(&owner_data, &page)?;
    }
    drop(owner_data);

    if bid_count as usize != page.len() {
        return Err(ProgramError::InvalidAccountData);
    }
    if expected_tag != [0; 16] && expected_tag != supplied_tag {
        return Err(ProgramError::InvalidSeeds);
    }
    if expected_tag == [0; 16] && bid_count != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let collateralized = market.collateralized_active();
    if collateralized {
        if accounts.len() != 5 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
    } else if accounts.len() != 4 || data.first().copied() != Some(9) {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut maker_balance = if collateralized {
        Some(load_maker_balance(
            program_id,
            &accounts[0],
            &accounts[3],
            &accounts[4],
        )?)
    } else {
        None
    };

    match data.first().copied() {
        Some(8) => {
            if data.len() != 41 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let sequence = market.next_sequence;
            let base_qty = u64::from_le_bytes(
                data[33..41]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            );
            if sequence == 0 || base_qty == 0 || page.len() >= hybrid_state::BIDS_PER_PAGE {
                return Err(ProgramError::InvalidInstructionData);
            }
            let entry = hybrid_state::BidEntry {
                price_x64: u128::from_le_bytes(
                    data[1..17]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                ),
                sqrt_price_x64: u128::from_le_bytes(
                    data[17..33]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                ),
                base_qty,
                sequence,
            };
            hybrid_engine::validate_limit_ask(entry.as_limit_ask())
                .map_err(|_| ProgramError::InvalidInstructionData)?;

            let mut index = 0usize;
            while index < page.len() {
                let existing = page.entries[index];
                if existing.sequence == sequence {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if existing.price_x64 < entry.price_x64
                    || (existing.price_x64 == entry.price_x64 && existing.sequence > entry.sequence)
                {
                    break;
                }
                index += 1;
            }

            let next_sequence = sequence
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let next_count = bid_count
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let quote_lock = hybrid_engine::quote_for_base_at_price(base_qty, entry.price_x64)
                .map_err(|_| ProgramError::InvalidInstructionData)?;
            maker_balance
                .as_mut()
                .ok_or(ProgramError::InvalidAccountData)?
                .lock_quote(quote_lock)
                .map_err(|_| ProgramError::InsufficientFunds)?;
            let old_len = page.len();

            {
                let mut bid_data = accounts[1].try_borrow_mut_data()?;
                insert_slot(&mut bid_data, index, old_len, hybrid_state::BID_ENTRY_BYTES);
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::BID_ENTRY_BYTES;
                bid_data[start..start + 16].copy_from_slice(&entry.price_x64.to_le_bytes());
                bid_data[start + 16..start + 32]
                    .copy_from_slice(&entry.sqrt_price_x64.to_le_bytes());
                bid_data[start + 32..start + 40].copy_from_slice(&entry.base_qty.to_le_bytes());
                bid_data[start + 40..start + 48].copy_from_slice(&entry.sequence.to_le_bytes());
            }
            {
                let mut owners = accounts[2].try_borrow_mut_data()?;
                if owner_page_uninitialized {
                    owners[2..16].copy_from_slice(&page.reserved);
                }
                insert_slot(&mut owners, index, old_len, hybrid_state::BID_OWNER_BYTES);
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::BID_OWNER_BYTES;
                owners[start..start + hybrid_state::BID_OWNER_BYTES]
                    .copy_from_slice(accounts[maker_index].key.as_ref());
            }
            {
                let mut market_bytes = accounts[0].try_borrow_mut_data()?;
                market_bytes[20..24].copy_from_slice(&next_count.to_le_bytes());
                market_bytes[32..40].copy_from_slice(&next_sequence.to_le_bytes());
                if expected_tag == [0; 16] {
                    market_bytes[24..32].copy_from_slice(&supplied_tag[0..8]);
                    market_bytes[40..48].copy_from_slice(&supplied_tag[8..16]);
                }
            }
            store_maker_balance(
                &accounts[3],
                maker_balance
                    .as_ref()
                    .ok_or(ProgramError::InvalidAccountData)?,
            )
        }
        Some(9) => {
            if data.len() != 9 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let sequence = u64::from_le_bytes(
                data[1..9]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
            );
            if sequence == 0 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let mut index = 0usize;
            while index < page.len() && page.entries[index].sequence != sequence {
                index += 1;
            }
            if index == page.len() {
                return Err(ProgramError::InvalidInstructionData);
            }
            {
                let owners = accounts[2].try_borrow_data()?;
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::BID_OWNER_BYTES;
                if owners[start..start + hybrid_state::BID_OWNER_BYTES]
                    != accounts[maker_index].key.to_bytes()
                {
                    return Err(ProgramError::InvalidArgument);
                }
            }

            let next_count = bid_count
                .checked_sub(1)
                .ok_or(ProgramError::InvalidAccountData)?;
            let unlock_quote = hybrid_engine::quote_for_base_at_price(
                page.entries[index].base_qty,
                page.entries[index].price_x64,
            )
            .map_err(|_| ProgramError::InvalidAccountData)?;
            if let Some(balance) = maker_balance.as_mut() {
                balance
                    .unlock_quote(unlock_quote)
                    .map_err(|_| ProgramError::InvalidAccountData)?;
            }
            let old_len = page.len();
            {
                let mut bid_data = accounts[1].try_borrow_mut_data()?;
                remove_slot(&mut bid_data, index, old_len, hybrid_state::BID_ENTRY_BYTES);
            }
            {
                let mut owners = accounts[2].try_borrow_mut_data()?;
                remove_slot(&mut owners, index, old_len, hybrid_state::BID_OWNER_BYTES);
            }
            {
                let mut market_bytes = accounts[0].try_borrow_mut_data()?;
                market_bytes[20..24].copy_from_slice(&next_count.to_le_bytes());
            }
            if let Some(balance) = maker_balance.as_ref() {
                store_maker_balance(&accounts[3], balance)
            } else {
                Ok(())
            }
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

#[inline(never)]
fn process_state_backed_plan(accounts: &[AccountInfo]) -> ProgramResult {
    if accounts.len() != 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let market_data = accounts[0].try_borrow_data()?;
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

    hybrid_engine::plan_buy_exact_in_levels(
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
fn process_init_custody(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 3 || accounts.len() != 9 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody = &accounts[1];
    let payer = &accounts[2];
    let base_mint = &accounts[3];
    let quote_mint = &accounts[4];
    let base_vault = &accounts[5];
    let quote_vault = &accounts[6];
    let system_program = &accounts[7];
    let token_program = &accounts[8];

    if market.owner != program_id || *token_program.key != TOKEN_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !market.is_signer || !market.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut market_header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if market_header.ask_count != 0
        || market_header.bid_count() != 0
        || market_header.collateralized_active()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let (expected_custody, custody_bump) =
        Pubkey::find_program_address(&[b"custody", market.key.as_ref()], program_id);
    if *custody.key != expected_custody {
        return Err(ProgramError::InvalidSeeds);
    }
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);

    let base_decimals = mint_decimals(base_mint)?;
    let quote_decimals = mint_decimals(quote_mint)?;
    if base_decimals != data[1] || quote_decimals != data[2] || base_mint.key == quote_mint.key {
        return Err(ProgramError::InvalidInstructionData);
    }

    let (base_vault_mint, base_vault_owner) = token_account_fields(base_vault)?;
    let (quote_vault_mint, quote_vault_owner) = token_account_fields(quote_vault)?;
    if base_vault_mint != base_mint.key.to_bytes()
        || quote_vault_mint != quote_mint.key.to_bytes()
        || base_vault_owner != vault_authority.to_bytes()
        || quote_vault_owner != vault_authority.to_bytes()
        || base_vault.key == quote_vault.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    create_program_pda(
        program_id,
        payer,
        custody,
        system_program,
        &[b"custody", market.key.as_ref()],
        custody_bump,
        hybrid_state::CUSTODY_STATE_BYTES,
    )?;

    let state = hybrid_state::CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump: custody_bump,
        base_decimals,
        quote_decimals,
        reserved: [0; 4],
        market: market.key.to_bytes(),
        base_mint: base_mint.key.to_bytes(),
        quote_mint: quote_mint.key.to_bytes(),
        base_vault: base_vault.key.to_bytes(),
        quote_vault: quote_vault.key.to_bytes(),
        total_base: 0,
        total_quote: 0,
    };
    store_custody(custody, &state)?;
    market_header.enable_collateralized_active();
    let mut market_data = market.try_borrow_mut_data()?;
    market_header
        .encode_into(&mut market_data)
        .map_err(|_| ProgramError::InvalidAccountData)
}

#[inline(never)]
fn process_init_maker_balance(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    if accounts.len() != 6 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let market = &accounts[0];
    let custody = &accounts[1];
    let balance = &accounts[2];
    let maker = &accounts[3];
    let payer = &accounts[4];
    let system_program = &accounts[5];
    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    load_custody(program_id, market, custody)?;

    let (expected, bump) = Pubkey::find_program_address(
        &[b"maker-balance", market.key.as_ref(), maker.key.as_ref()],
        program_id,
    );
    if *balance.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    create_program_pda(
        program_id,
        payer,
        balance,
        system_program,
        &[b"maker-balance", market.key.as_ref(), maker.key.as_ref()],
        bump,
        hybrid_state::MAKER_BALANCE_BYTES,
    )?;
    let state = hybrid_state::MakerBalance::new(bump, market.key.to_bytes(), maker.key.to_bytes());
    store_maker_balance(balance, &state)
}

fn parse_asset_amount(data: &[u8], opcode: u8) -> Result<(u8, u64), ProgramError> {
    if data.len() != 10 || data[0] != opcode || data[1] > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if amount == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok((data[1], amount))
}

#[inline(never)]
fn process_deposit(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() != 8 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (asset, amount) = parse_asset_amount(data, 13)?;
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let balance_account = &accounts[2];
    let maker = &accounts[3];
    let source = &accounts[4];
    let vault = &accounts[5];
    let mint = &accounts[6];
    let token_program = &accounts[7];
    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut custody = load_custody(program_id, market, custody_account)?;
    let mut balance = load_maker_balance(program_id, market, balance_account, maker)?;
    let (source_mint, source_owner) = token_account_fields(source)?;
    if source_owner != maker.key.to_bytes() {
        return Err(ProgramError::InvalidAccountData);
    }

    let (expected_mint, expected_vault, decimals) = if asset == 0 {
        (custody.base_mint, custody.base_vault, custody.base_decimals)
    } else {
        (
            custody.quote_mint,
            custody.quote_vault,
            custody.quote_decimals,
        )
    };
    if mint.key.to_bytes() != expected_mint
        || vault.key.to_bytes() != expected_vault
        || source_mint != expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }

    transfer_checked(
        source,
        mint,
        vault,
        maker,
        token_program,
        (amount, decimals),
        None,
    )?;
    if asset == 0 {
        balance.free_base = balance
            .free_base
            .checked_add(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
        custody.total_base = custody
            .total_base
            .checked_add(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
    } else {
        balance.free_quote = balance
            .free_quote
            .checked_add(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
        custody.total_quote = custody
            .total_quote
            .checked_add(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
    }
    store_maker_balance(balance_account, &balance)?;
    store_custody(custody_account, &custody)
}

#[inline(never)]
fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() != 9 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (asset, amount) = parse_asset_amount(data, 14)?;
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let balance_account = &accounts[2];
    let maker = &accounts[3];
    let vault = &accounts[4];
    let destination = &accounts[5];
    let mint = &accounts[6];
    let vault_authority = &accounts[7];
    let token_program = &accounts[8];
    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let mut custody = load_custody(program_id, market, custody_account)?;
    let mut balance = load_maker_balance(program_id, market, balance_account, maker)?;
    let (destination_mint, destination_owner) = token_account_fields(destination)?;
    if destination_owner != maker.key.to_bytes() {
        return Err(ProgramError::InvalidAccountData);
    }
    let (expected_authority, authority_bump) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *vault_authority.key != expected_authority {
        return Err(ProgramError::InvalidSeeds);
    }

    let (expected_mint, expected_vault, decimals) = if asset == 0 {
        (custody.base_mint, custody.base_vault, custody.base_decimals)
    } else {
        (
            custody.quote_mint,
            custody.quote_vault,
            custody.quote_decimals,
        )
    };
    if mint.key.to_bytes() != expected_mint
        || vault.key.to_bytes() != expected_vault
        || destination_mint != expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }

    if asset == 0 {
        balance.free_base = balance
            .free_base
            .checked_sub(amount)
            .ok_or(ProgramError::InsufficientFunds)?;
        custody.total_base = custody
            .total_base
            .checked_sub(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
    } else {
        balance.free_quote = balance
            .free_quote
            .checked_sub(amount)
            .ok_or(ProgramError::InsufficientFunds)?;
        custody.total_quote = custody
            .total_quote
            .checked_sub(amount)
            .ok_or(ProgramError::InvalidAccountData)?;
    }

    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market.key.as_ref(), &bump];
    transfer_checked(
        vault,
        mint,
        destination,
        vault_authority,
        token_program,
        (amount, decimals),
        Some(seeds),
    )?;
    store_maker_balance(balance_account, &balance)?;
    store_custody(custody_account, &custody)
}

const MAX_SWAP_MAKER_ACCOUNTS: usize = 8;
const BUY_SWAP_FIXED_ACCOUNTS: usize = 11;

fn parse_buy_swap(data: &[u8]) -> Result<(u8, u64, u64), ProgramError> {
    if data.len() != 17 || (data[0] != 15 && data[0] != 16) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let limit = u64::from_le_bytes(
        data[9..17]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if amount == 0 || limit == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok((data[0], amount, limit))
}

#[inline(never)]
fn load_owner_page_box(
    account: &AccountInfo,
) -> Result<Box<hybrid_state::AskOwnerPage>, ProgramError> {
    let data = account.try_borrow_data()?;
    let mut page = Box::new(hybrid_state::AskOwnerPage::default());
    hybrid_state::AskOwnerPage::decode_into(&data, &mut page)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(page)
}

#[inline(never)]
fn plan_active_buy(
    opcode: u8,
    sqrt_price_x64: u128,
    asks: &[hybrid_engine::LimitAsk],
    amount: u64,
    limit: u64,
) -> Result<Box<hybrid_engine::ActiveOnlyBuyPlan>, ProgramError> {
    let plan = if opcode == 15 {
        let plan = hybrid_engine::plan_buy_active_exact_in(sqrt_price_x64, asks, amount)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.amount_in != amount || plan.amount_out < limit {
            return Err(ProgramError::InsufficientFunds);
        }
        plan
    } else {
        let plan = hybrid_engine::plan_buy_active_exact_out(sqrt_price_x64, asks, amount)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.amount_out != amount || plan.amount_in > limit {
            return Err(ProgramError::InsufficientFunds);
        }
        plan
    };
    if plan.fill_count == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(Box::new(plan))
}

fn process_buy_swap(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() <= BUY_SWAP_FIXED_ACCOUNTS
        || accounts.len() > BUY_SWAP_FIXED_ACCOUNTS + MAX_SWAP_MAKER_ACCOUNTS
    {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (opcode, amount, limit) = parse_buy_swap(data)?;
    let market_account = &accounts[0];
    let custody_account = &accounts[1];
    let ask_account = &accounts[2];
    let owner_account = &accounts[3];
    let taker = &accounts[4];
    let taker_quote = &accounts[5];
    let taker_base = &accounts[6];
    let quote_vault = &accounts[7];
    let base_vault = &accounts[8];
    let vault_authority = &accounts[9];
    let token_program = &accounts[10];
    let maker_accounts = &accounts[BUY_SWAP_FIXED_ACCOUNTS..];

    if !taker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market_account.owner != program_id
        || ask_account.owner != program_id
        || owner_account.owner != program_id
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !market_account.is_writable
        || !custody_account.is_writable
        || !ask_account.is_writable
        || !owner_account.is_writable
        || !taker_quote.is_writable
        || !taker_base.is_writable
        || !quote_vault.is_writable
        || !base_vault.is_writable
        || maker_accounts.iter().any(|account| !account.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let market_data = market_account.try_borrow_data()?;
    let mut market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);
    if !market.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }

    let index_bytes = 0u32.to_le_bytes();
    let (expected_ask, _) = Pubkey::find_program_address(
        &[b"ask-page", market_account.key.as_ref(), &index_bytes],
        program_id,
    );
    if *ask_account.key != expected_ask || market.reserved2 != owner_account.key.to_bytes() {
        return Err(ProgramError::InvalidSeeds);
    }

    let mut asks = load_active_ask_page(ask_account)?;
    let mut owners = load_owner_page_box(owner_account)?;
    owners
        .validate_parallel(&asks)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if market.ask_count as usize != asks.len() {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut custody = load_custody(program_id, market_account, custody_account)?;
    if quote_vault.key.to_bytes() != custody.quote_vault
        || base_vault.key.to_bytes() != custody.base_vault
        || *token_program.key != TOKEN_PROGRAM_ID
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

    let (taker_quote_mint, taker_quote_owner) = token_account_fields(taker_quote)?;
    let (taker_base_mint, taker_base_owner) = token_account_fields(taker_base)?;
    let (quote_vault_mint, quote_vault_owner) = token_account_fields(quote_vault)?;
    let (base_vault_mint, base_vault_owner) = token_account_fields(base_vault)?;
    if taker_quote_owner != taker.key.to_bytes()
        || taker_base_owner != taker.key.to_bytes()
        || taker_quote_mint != custody.quote_mint
        || taker_base_mint != custody.base_mint
        || quote_vault_mint != custody.quote_mint
        || base_vault_mint != custody.base_mint
        || quote_vault_owner != expected_authority.to_bytes()
        || base_vault_owner != expected_authority.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let ask_limits = asks
        .as_slice()
        .iter()
        .copied()
        .map(hybrid_state::AskEntry::as_limit_ask)
        .collect::<Vec<_>>();
    let plan = plan_active_buy(opcode, market.sqrt_price_x64, &ask_limits, amount, limit)?;
    let fills = &plan.fills[..usize::from(plan.fill_count)];

    let mut maker_states = Vec::with_capacity(maker_accounts.len());
    for account in maker_accounts {
        if maker_accounts
            .iter()
            .filter(|candidate| candidate.key == account.key)
            .count()
            != 1
        {
            return Err(ProgramError::InvalidAccountData);
        }
        let data = account.try_borrow_data()?;
        let state = hybrid_state::MakerBalance::decode_from(&data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        drop(data);
        if state.market != market_account.key.to_bytes() {
            return Err(ProgramError::InvalidAccountData);
        }
        let checked =
            load_maker_balance_for_owner(program_id, market_account, account, state.owner)?;
        maker_states.push(checked);
    }

    let mut fill_balance_indexes = Vec::with_capacity(fills.len());
    for fill in fills {
        let owner = owners.owners[usize::from(fill.ask_index)];
        let index = maker_states
            .iter()
            .position(|balance| balance.owner == owner)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        fill_balance_indexes.push(index);
    }

    let mut settled_states = maker_states.clone();
    for (fill, index) in fills.iter().zip(fill_balance_indexes.iter().copied()) {
        settled_states[index]
            .settle_ask(fill.base_qty, fill.quote_qty)
            .map_err(|_| ProgramError::InsufficientFunds)?;
    }

    let removed = hybrid_state::apply_active_fills_to_ask_page(&mut asks, &mut owners, fills)
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

    {
        let mut data = market_account.try_borrow_mut_data()?;
        market
            .encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut data = ask_account.try_borrow_mut_data()?;
        asks.encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut data = owner_account.try_borrow_mut_data()?;
        owners
            .encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    for (account, state) in maker_accounts.iter().zip(settled_states.iter()) {
        store_maker_balance(account, state)?;
    }
    store_custody(custody_account, &custody)
}

const SELL_SWAP_FIXED_ACCOUNTS: usize = 11;

/// Exact-in (17): base input and minimum quote output.
/// Exact-out (18): quote output and maximum base input.
/// Both use the same bounded account prefix as the buy execution path:
/// market, custody, bid page, bid-owner page, taker, taker-base,
/// taker-quote, base vault, quote vault, authority, token program,
/// then the maker-balance PDAs for the resting bids touched.
#[inline(never)]
fn plan_active_sell(
    opcode: u8,
    sqrt_price_x64: u128,
    bids: &[hybrid_engine::LimitAsk],
    amount: u64,
    limit: u64,
) -> Result<Box<hybrid_engine::ActiveOnlySellPlan>, ProgramError> {
    let plan = if opcode == 17 {
        let plan = hybrid_engine::plan_sell_active_exact_in(sqrt_price_x64, bids, amount)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.amount_in != amount || plan.amount_out < limit {
            return Err(ProgramError::InsufficientFunds);
        }
        plan
    } else {
        let plan = hybrid_engine::plan_sell_active_exact_out(sqrt_price_x64, bids, amount)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        if plan.amount_out < amount || plan.amount_in > limit {
            return Err(ProgramError::InsufficientFunds);
        }
        plan
    };
    if plan.fill_count == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(Box::new(plan))
}

#[inline(never)]
fn process_sell_swap(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if accounts.len() <= SELL_SWAP_FIXED_ACCOUNTS
        || accounts.len() > SELL_SWAP_FIXED_ACCOUNTS + MAX_SWAP_MAKER_ACCOUNTS
        || data.len() != 17
    {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let opcode = data[0];
    if opcode != 17 && opcode != 18 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(
        data[1..9]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let limit = u64::from_le_bytes(
        data[9..17]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if amount == 0 || limit == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let market_account = &accounts[0];
    let custody_account = &accounts[1];
    let bid_account = &accounts[2];
    let owner_account = &accounts[3];
    let taker = &accounts[4];
    let taker_base = &accounts[5];
    let taker_quote = &accounts[6];
    let base_vault = &accounts[7];
    let quote_vault = &accounts[8];
    let vault_authority = &accounts[9];
    let token_program = &accounts[10];
    let maker_accounts = &accounts[SELL_SWAP_FIXED_ACCOUNTS..];

    if !taker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market_account.owner != program_id
        || bid_account.owner != program_id
        || owner_account.owner != program_id
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !market_account.is_writable
        || !custody_account.is_writable
        || !bid_account.is_writable
        || !owner_account.is_writable
        || !taker_base.is_writable
        || !taker_quote.is_writable
        || !base_vault.is_writable
        || !quote_vault.is_writable
        || maker_accounts.iter().any(|account| !account.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let market_data = market_account.try_borrow_data()?;
    let mut market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);
    if !market.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let index_bytes = 0u32.to_le_bytes();
    let (expected_bid, _) = Pubkey::find_program_address(
        &[b"bid-page", market_account.key.as_ref(), &index_bytes],
        program_id,
    );
    if *bid_account.key != expected_bid || market.bid_owner_tag() != owner_tag(owner_account.key) {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut bids = load_active_bid_page(bid_account)?;
    let mut owners = load_owner_page_box(owner_account)?;
    if bids.len() != owners.len()
        || bids.links() != owners.links()
        || bids.len() != market.bid_count() as usize
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let links = bids.links();
    if links.page_index != 0 || links.prev_page.is_some() || links.next_page.is_some() {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut custody = load_custody(program_id, market_account, custody_account)?;
    if base_vault.key.to_bytes() != custody.base_vault
        || quote_vault.key.to_bytes() != custody.quote_vault
        || *token_program.key != TOKEN_PROGRAM_ID
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
    let (taker_base_mint, taker_base_owner) = token_account_fields(taker_base)?;
    let (taker_quote_mint, taker_quote_owner) = token_account_fields(taker_quote)?;
    let (base_vault_mint, base_vault_owner) = token_account_fields(base_vault)?;
    let (quote_vault_mint, quote_vault_owner) = token_account_fields(quote_vault)?;
    if taker_base_owner != taker.key.to_bytes()
        || taker_quote_owner != taker.key.to_bytes()
        || taker_base_mint != custody.base_mint
        || taker_quote_mint != custody.quote_mint
        || base_vault_mint != custody.base_mint
        || quote_vault_mint != custody.quote_mint
        || base_vault_owner != expected_authority.to_bytes()
        || quote_vault_owner != expected_authority.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let limits = bids
        .as_slice()
        .iter()
        .copied()
        .map(hybrid_state::BidEntry::as_limit_ask)
        .collect::<Vec<_>>();
    let plan = plan_active_sell(opcode, market.sqrt_price_x64, &limits, amount, limit)?;
    let fills = &plan.fills[..usize::from(plan.fill_count)];

    let mut maker_states = Vec::with_capacity(maker_accounts.len());
    for account in maker_accounts {
        if maker_accounts
            .iter()
            .filter(|candidate| candidate.key == account.key)
            .count()
            != 1
        {
            return Err(ProgramError::InvalidAccountData);
        }
        let data = account.try_borrow_data()?;
        let state = hybrid_state::MakerBalance::decode_from(&data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        drop(data);
        if state.market != market_account.key.to_bytes() {
            return Err(ProgramError::InvalidAccountData);
        }
        maker_states.push(load_maker_balance_for_owner(
            program_id,
            market_account,
            account,
            state.owner,
        )?);
    }
    let mut settled_states = maker_states.clone();
    for fill in fills {
        let index = usize::from(fill.bid_index);
        let owner = owners.owners[index];
        let maker_index = maker_states
            .iter()
            .position(|balance| balance.owner == owner)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let original = bids.entries[index].base_qty;
        let remaining = original
            .checked_sub(fill.base_qty)
            .ok_or(ProgramError::InvalidAccountData)?;
        settled_states[maker_index]
            .settle_bid_fill(
                original,
                remaining,
                bids.entries[index].price_x64,
                fill.base_qty,
                fill.quote_qty,
            )
            .map_err(|_| ProgramError::InsufficientFunds)?;
    }

    let removed = hybrid_state::apply_active_fills_to_bid_page(&mut bids, &mut owners, fills)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let next_count = market
        .bid_count()
        .checked_sub(u32::try_from(removed).map_err(|_| ProgramError::InvalidAccountData)?)
        .ok_or(ProgramError::InvalidAccountData)?;
    market.set_bid_count(next_count);

    custody.total_base = custody
        .total_base
        .checked_add(plan.amount_in)
        .ok_or(ProgramError::InvalidAccountData)?;
    custody.total_quote = custody
        .total_quote
        .checked_sub(plan.amount_out)
        .ok_or(ProgramError::InsufficientFunds)?;

    transfer_tokens(
        taker_base,
        base_vault,
        taker,
        token_program,
        plan.amount_in,
        None,
    )?;
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market_account.key.as_ref(), &bump];
    transfer_tokens(
        quote_vault,
        taker_quote,
        vault_authority,
        token_program,
        plan.amount_out,
        Some(seeds),
    )?;

    {
        let mut data = market_account.try_borrow_mut_data()?;
        market
            .encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut data = bid_account.try_borrow_mut_data()?;
        bids.encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    {
        let mut data = owner_account.try_borrow_mut_data()?;
        owners
            .encode_into(&mut data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    for (account, state) in maker_accounts.iter().zip(settled_states.iter()) {
        store_maker_balance(account, state)?;
    }
    store_custody(custody_account, &custody)
}

#[inline(never)]
fn process_init_passive_pool(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data != [19] || accounts.len() != 5 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody = &accounts[1];
    let pool_account = &accounts[2];
    let payer = &accounts[3];
    let system_program = &accounts[4];
    if market.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    load_custody(program_id, market, custody)?;
    let market_state = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market_state.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let (expected, bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    if *pool_account.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    create_program_pda(
        program_id,
        payer,
        pool_account,
        system_program,
        &[b"passive-pool", market.key.as_ref()],
        bump,
        hybrid_state::passive::PASSIVE_POOL_BYTES,
    )?;
    let state = hybrid_state::passive::PoolAccount {
        bump,
        market: market.key.to_bytes(),
        pool: hybrid_state::passive::PassivePool::default(),
    };
    state
        .encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}

/// Create a position PDA and transfer the owner's base and quote principal
/// into the existing SPL Token vaults. Positions cannot withdraw or trade
/// until fee attribution and position redemption are implemented.
/// Instruction: 20 | nonce:u64 | lower:u128 | upper:u128 | liquidity:u128
///                | base:u64 | quote:u64. Exactly 73 bytes.
#[inline(never)]
fn process_open_passive_position(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 73 || data[0] != 20 || accounts.len() != 13 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let pool_account = &accounts[2];
    let position_account = &accounts[3];
    let lp = &accounts[4];
    let source_base = &accounts[5];
    let source_quote = &accounts[6];
    let base_vault = &accounts[7];
    let quote_vault = &accounts[8];
    let base_mint = &accounts[9];
    let quote_mint = &accounts[10];
    let token_program = &accounts[11];
    let system_program = &accounts[12];
    if !lp.is_signer || !lp.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if pool_account.owner != program_id || !pool_account.is_writable {
        return Err(ProgramError::IncorrectProgramId);
    }
    let custody = load_custody(program_id, market, custody_account)?;
    let mut pool =
        hybrid_state::passive::PoolAccount::decode_from(&pool_account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    let (expected_pool, pool_bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    if *pool_account.key != expected_pool
        || pool.market != market.key.to_bytes()
        || pool.bump != pool_bump
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let nonce_bytes: [u8; 8] = data[1..9]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let nonce = u64::from_le_bytes(nonce_bytes);
    let u128_field = |start: usize| -> Result<u128, ProgramError> {
        Ok(u128::from_le_bytes(
            data[start..start + 16]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ))
    };
    let u64_field = |start: usize| -> Result<u64, ProgramError> {
        Ok(u64::from_le_bytes(
            data[start..start + 8]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ))
    };
    let position = hybrid_state::passive::PassivePosition {
        owner: lp.key.to_bytes(),
        lower_sqrt_price_x64: u128_field(9)?,
        upper_sqrt_price_x64: u128_field(25)?,
        liquidity: u128_field(41)?,
        base_principal: u64_field(57)?,
        quote_principal: u64_field(65)?,
    };
    position
        .validate()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let (expected_position, bump) = Pubkey::find_program_address(
        &[
            b"passive-position",
            market.key.as_ref(),
            lp.key.as_ref(),
            &nonce_bytes,
        ],
        program_id,
    );
    if *position_account.key != expected_position {
        return Err(ProgramError::InvalidSeeds);
    }
    let (expected_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *base_vault.key != Pubkey::new_from_array(custody.base_vault)
        || *quote_vault.key != Pubkey::new_from_array(custody.quote_vault)
        || *base_mint.key != Pubkey::new_from_array(custody.base_mint)
        || *quote_mint.key != Pubkey::new_from_array(custody.quote_mint)
        || *token_program.key != TOKEN_PROGRAM_ID
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let (source_base_mint, source_base_owner) = token_account_fields(source_base)?;
    let (source_quote_mint, source_quote_owner) = token_account_fields(source_quote)?;
    let (vault_base_mint, vault_base_owner) = token_account_fields(base_vault)?;
    let (vault_quote_mint, vault_quote_owner) = token_account_fields(quote_vault)?;
    if source_base_owner != lp.key.to_bytes()
        || source_quote_owner != lp.key.to_bytes()
        || source_base_mint != custody.base_mint
        || source_quote_mint != custody.quote_mint
        || vault_base_mint != custody.base_mint
        || vault_quote_mint != custody.quote_mint
        || vault_base_owner != expected_authority.to_bytes()
        || vault_quote_owner != expected_authority.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    pool.pool
        .deposit(&position)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    create_program_pda(
        program_id,
        lp,
        position_account,
        system_program,
        &[
            b"passive-position",
            market.key.as_ref(),
            lp.key.as_ref(),
            &nonce_bytes,
        ],
        bump,
        hybrid_state::passive::PASSIVE_POSITION_BYTES,
    )?;
    if position.base_principal > 0 {
        transfer_checked(
            source_base,
            base_mint,
            base_vault,
            lp,
            token_program,
            (position.base_principal, custody.base_decimals),
            None,
        )?;
    }
    if position.quote_principal > 0 {
        transfer_checked(
            source_quote,
            quote_mint,
            quote_vault,
            lp,
            token_program,
            (position.quote_principal, custody.quote_decimals),
            None,
        )?;
    }
    let state = hybrid_state::passive::PositionAccount {
        bump,
        market: market.key.to_bytes(),
        nonce,
        position,
    };
    state
        .encode_into(&mut position_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}

#[inline(never)]
fn process_close_passive_position(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 9 || data[0] != 21 || accounts.len() != 13 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let pool_account = &accounts[2];
    let position_account = &accounts[3];
    let lp = &accounts[4];
    let base_vault = &accounts[5];
    let quote_vault = &accounts[6];
    let dest_base = &accounts[7];
    let dest_quote = &accounts[8];
    let base_mint = &accounts[9];
    let quote_mint = &accounts[10];
    let vault_authority = &accounts[11];
    let token_program = &accounts[12];

    if !lp.is_signer || !lp.is_writable {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if pool_account.owner != program_id
        || position_account.owner != program_id
        || !pool_account.is_writable
        || !position_account.is_writable
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let custody = load_custody(program_id, market, custody_account)?;
    let mut pool =
        hybrid_state::passive::PoolAccount::decode_from(&pool_account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    let state =
        hybrid_state::passive::PositionAccount::decode_from(&position_account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;

    let (expected_pool, pool_bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    let nonce_bytes: [u8; 8] = data[1..9]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let (expected_position, position_bump) = Pubkey::find_program_address(
        &[
            b"passive-position",
            market.key.as_ref(),
            lp.key.as_ref(),
            &nonce_bytes,
        ],
        program_id,
    );
    if *pool_account.key != expected_pool
        || *position_account.key != expected_position
        || pool.bump != pool_bump
        || state.bump != position_bump
        || state.nonce != u64::from_le_bytes(nonce_bytes)
        || pool.market != market.key.to_bytes()
        || state.market != market.key.to_bytes()
        || state.position.owner != lp.key.to_bytes()
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let (expected_authority, authority_bump) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *vault_authority.key != expected_authority
        || *base_vault.key != Pubkey::new_from_array(custody.base_vault)
        || *quote_vault.key != Pubkey::new_from_array(custody.quote_vault)
        || *base_mint.key != Pubkey::new_from_array(custody.base_mint)
        || *quote_mint.key != Pubkey::new_from_array(custody.quote_mint)
        || *token_program.key != TOKEN_PROGRAM_ID
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let (base_mint_id, base_owner) = token_account_fields(base_vault)?;
    let (quote_mint_id, quote_owner) = token_account_fields(quote_vault)?;
    let (dest_base_mint, dest_base_owner) = token_account_fields(dest_base)?;
    let (dest_quote_mint, dest_quote_owner) = token_account_fields(dest_quote)?;
    if base_mint_id != custody.base_mint
        || quote_mint_id != custody.quote_mint
        || base_owner != expected_authority.to_bytes()
        || quote_owner != expected_authority.to_bytes()
        || dest_base_mint != custody.base_mint
        || dest_quote_mint != custody.quote_mint
        || dest_base_owner != lp.key.to_bytes()
        || dest_quote_owner != lp.key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // The vaults must cover all active maker claims and passive LP claims
    // before a position can draw down its principal.
    let vault_amount = |account: &AccountInfo| -> Result<u64, ProgramError> {
        let bytes = account.try_borrow_data()?;
        Ok(u64::from_le_bytes(
            bytes[64..72]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        ))
    };
    pool.pool
        .verify_vault_coverage(
            custody.total_base,
            custody.total_quote,
            vault_amount(base_vault)?,
            vault_amount(quote_vault)?,
        )
        .map_err(|_| ProgramError::InsufficientFunds)?;
    // Swaps and fee distributions are not enabled for passive positions.
    // Only the exact deposited principal can be redeemed.
    pool.pool
        .withdraw(&state.position)
        .map_err(|_| ProgramError::InsufficientFunds)?;

    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market.key.as_ref(), &bump];
    if state.position.base_principal > 0 {
        transfer_checked(
            base_vault,
            base_mint,
            dest_base,
            vault_authority,
            token_program,
            (state.position.base_principal, custody.base_decimals),
            Some(seeds),
        )?;
    }
    if state.position.quote_principal > 0 {
        transfer_checked(
            quote_vault,
            quote_mint,
            dest_quote,
            vault_authority,
            token_program,
            (state.position.quote_principal, custody.quote_decimals),
            Some(seeds),
        )?;
    }

    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    // A zeroed position cannot decode again. A second withdrawal fails.
    position_account.try_borrow_mut_data()?.fill(0);
    // Return the position account's rent to the LP.
    let rent = position_account.lamports();
    **position_account.try_borrow_mut_lamports()? = 0;
    **lp.try_borrow_mut_lamports()? = lp
        .lamports()
        .checked_add(rent)
        .ok_or(ProgramError::InvalidAccountData)?;
    Ok(())
}
