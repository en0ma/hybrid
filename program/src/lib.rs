#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::AccountInfo, declare_id, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
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
fn process_active_order(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
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

    let index_bytes = 0u32.to_le_bytes();
    let (expected_ask, _) = Pubkey::find_program_address(
        &[b"ask-page", accounts[0].key.as_ref(), &index_bytes],
        program_id,
    );
    if *accounts[1].key != expected_ask {
        return Err(ProgramError::InvalidSeeds);
    }

    let market_data = accounts[0]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let market = hybrid_state::MarketHeader::decode_from(&market_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    drop(market_data);

    let ask_data = accounts[1]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let page =
        hybrid_state::AskPage::decode_from(&ask_data).map_err(|_| ProgramError::InvalidAccountData)?;
    drop(ask_data);
    let links = page.links();
    if links.page_index != 0 || links.prev_page.is_some() || links.next_page.is_some() {
        return Err(ProgramError::InvalidAccountData);
    }

    let owner_data = accounts[2]
        .try_borrow_data()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    validate_owner_page_bytes(&owner_data, &page)?;
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
            if page
                .as_slice()
                .iter()
                .any(|existing| existing.sequence == sequence)
            {
                return Err(ProgramError::InvalidInstructionData);
            }

            let mut index = 0usize;
            while index < page.len()
                && (page.entries[index].price_x64, page.entries[index].sequence)
                    < (entry.price_x64, entry.sequence)
            {
                index += 1;
            }
            let next_sequence = sequence
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let next_count = market
                .ask_count
                .checked_add(1)
                .ok_or(ProgramError::InvalidInstructionData)?;
            let old_len = page.len();

            {
                let mut ask_data = accounts[1]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                insert_slot(&mut ask_data, index, old_len, hybrid_state::ASK_ENTRY_BYTES);
                let start =
                    hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_ENTRY_BYTES;
                ask_data[start..start + 16].copy_from_slice(&entry.price_x64.to_le_bytes());
                ask_data[start + 16..start + 32]
                    .copy_from_slice(&entry.sqrt_price_x64.to_le_bytes());
                ask_data[start + 32..start + 40].copy_from_slice(&entry.base_qty.to_le_bytes());
                ask_data[start + 40..start + 48].copy_from_slice(&entry.sequence.to_le_bytes());
            }
            {
                let mut owners = accounts[2]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                insert_slot(&mut owners, index, old_len, hybrid_state::ASK_OWNER_BYTES);
                let start =
                    hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
                owners[start..start + hybrid_state::ASK_OWNER_BYTES]
                    .copy_from_slice(accounts[3].key.as_ref());
            }
            {
                let mut market_bytes = accounts[0]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                market_bytes[12..16].copy_from_slice(&next_count.to_le_bytes());
                market_bytes[32..40].copy_from_slice(&next_sequence.to_le_bytes());
                if market.reserved2 == [0; 32] {
                    market_bytes[96..128].copy_from_slice(&sidecar_key);
                }
            }
            Ok(())
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
            let index = page
                .as_slice()
                .iter()
                .position(|entry| entry.sequence == sequence)
                .ok_or(ProgramError::InvalidInstructionData)?;
            {
                let owners = accounts[2]
                    .try_borrow_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                let start = hybrid_state::PAGE_HEADER_BYTES + index * hybrid_state::ASK_OWNER_BYTES;
                if owners[start..start + hybrid_state::ASK_OWNER_BYTES]
                    != accounts[3].key.to_bytes()
                {
                    return Err(ProgramError::InvalidArgument);
                }
            }
            let next_count = market
                .ask_count
                .checked_sub(1)
                .ok_or(ProgramError::InvalidAccountData)?;
            let old_len = page.len();

            {
                let mut ask_data = accounts[1]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                remove_slot(&mut ask_data, index, old_len, hybrid_state::ASK_ENTRY_BYTES);
            }
            {
                let mut owners = accounts[2]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                remove_slot(&mut owners, index, old_len, hybrid_state::ASK_OWNER_BYTES);
            }
            {
                let mut market_bytes = accounts[0]
                    .try_borrow_mut_data()
                    .map_err(|_| ProgramError::AccountBorrowFailed)?;
                market_bytes[12..16].copy_from_slice(&next_count.to_le_bytes());
            }
            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
