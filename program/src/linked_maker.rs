use super::*;

/// Bounded two-page maker lifecycle.
///
/// 29: attach an empty, detached page 1 (requires empty canonical head).
/// 30: add collateralized maker order to page 0 or 1.
/// 31: cancel the maker's order on page 0 or 1 and unlock collateral.
/// 32: detach an empty page 1 without closing/rent reclaiming it.
///
/// Data: [opcode, side: 0 ask / 1 bid, page_index: 0 / 1 (30/31),
///        payload: add(price_x64:u128, sqrt_x64:u128, qty:u64) or cancel(sequence:u64)].
/// All accounts: market, head-page, head-owner, tail-page, tail-owner.
/// Add/cancel append maker-balance PDA, maker signer.
#[inline(never)]
pub(super) fn process_linked_maker(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 2 || data[1] > 1 || !(29..=32).contains(&data[0]) {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() != if data[0] == 30 || data[0] == 31 { 7 } else { 5 } {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if accounts[..5]
        .iter()
        .any(|a| a.owner != program_id || !a.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    for (i, a) in accounts.iter().enumerate() {
        if accounts[i + 1..].iter().any(|b| a.key == b.key) {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    // Linking or unlinking a market's canonical book requires the market's
    // original signing authority; an arbitrary payer cannot grief makers.
    if (data[0] == 29 || data[0] == 32) && !accounts[0].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let side = data[1];
    for (index, offset) in [(0u32, 1usize), (1u32, 3usize)] {
        let n = index.to_le_bytes();
        let seed = if side == 0 {
            b"ask-page".as_slice()
        } else {
            b"bid-page".as_slice()
        };
        let expected =
            Pubkey::find_program_address(&[seed, accounts[0].key.as_ref(), &n], program_id).0;
        if *accounts[offset].key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
        // The original head sidecar is committed in the market header,
        // not required to use a PDA. Only page-1 sidecars are canonical PDAs.
        if index == 1 {
            let owner_seed = if side == 0 {
                b"ask-owner-page".as_slice()
            } else {
                b"bid-owner-page".as_slice()
            };
            let owner = Pubkey::find_program_address(
                &[owner_seed, accounts[0].key.as_ref(), &n],
                program_id,
            )
            .0;
            if *accounts[offset + 1].key != owner {
                return Err(ProgramError::InvalidSeeds);
            }
        }
    }
    let mut market = hybrid_state::MarketHeader::decode_from(&accounts[0].try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    if side == 0 {
        if market.reserved2 != accounts[2].key.to_bytes() {
            return Err(ProgramError::InvalidSeeds);
        }
    } else if market.bid_owner_tag() != owner_tag(accounts[2].key) {
        return Err(ProgramError::InvalidSeeds);
    }
    if side == 0 {
        let mut head = load_active_ask_page(&accounts[1])?;
        let mut ho = load_owner_page_box(&accounts[2])?;
        let mut tail = load_active_ask_page(&accounts[3])?;
        let mut to = load_owner_page_box(&accounts[4])?;
        ho.validate_parallel(&head)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        to.validate_parallel(&tail)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if head.len().checked_add(tail.len()) != Some(market.ask_count as usize) {
            return Err(ProgramError::InvalidAccountData);
        }
        let linked = head.links() == hybrid_state::PageLinks::new(0, None, Some(1))
            && tail.links() == hybrid_state::PageLinks::new(1, Some(0), None);
        let detached = head.links() == hybrid_state::PageLinks::new(0, None, None)
            && tail.links() == hybrid_state::PageLinks::new(1, None, None);
        match data[0] {
            29 if data.len() == 2
                && detached
                && head.is_empty()
                && tail.is_empty()
                && market.ask_count == 0 =>
            {
                head.set_links(hybrid_state::PageLinks::new(0, None, Some(1)));
                ho.set_links(head.links());
                tail.set_links(hybrid_state::PageLinks::new(1, Some(0), None));
                to.set_links(tail.links());
            }
            32 if data.len() == 2 && linked && tail.is_empty() => {
                head.set_links(hybrid_state::PageLinks::new(0, None, None));
                ho.set_links(head.links());
                tail.set_links(hybrid_state::PageLinks::new(1, None, None));
                to.set_links(tail.links());
            }
            30 | 31 if linked => {
                if (data[0] == 30 && data.len() != 43) || (data[0] == 31 && data.len() != 11) {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if !accounts[6].is_signer || !accounts[5].is_writable {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                let mut balance =
                    load_maker_balance(program_id, &accounts[0], &accounts[5], &accounts[6])?;
                let index = data[2];
                if index > 1 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if data[0] == 30 {
                    let price = u128::from_le_bytes(
                        data[3..19]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let sqrt = u128::from_le_bytes(
                        data[19..35]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let qty = u64::from_le_bytes(
                        data[35..43]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let entry = hybrid_state::AskEntry {
                        price_x64: price,
                        sqrt_price_x64: sqrt,
                        base_qty: qty,
                        sequence: market.next_sequence,
                    };
                    if index == 0 {
                        hybrid_state::insert_owned_ask(
                            &mut head,
                            &mut ho,
                            entry,
                            accounts[6].key.to_bytes(),
                        )
                    } else {
                        hybrid_state::insert_owned_ask(
                            &mut tail,
                            &mut to,
                            entry,
                            accounts[6].key.to_bytes(),
                        )
                    }
                    .map_err(|_| ProgramError::InvalidInstructionData)?;
                    balance
                        .lock_base(qty)
                        .map_err(|_| ProgramError::InsufficientFunds)?;
                    market.ask_count = market
                        .ask_count
                        .checked_add(1)
                        .ok_or(ProgramError::InvalidAccountData)?;
                    market.next_sequence = market
                        .next_sequence
                        .checked_add(1)
                        .ok_or(ProgramError::InvalidAccountData)?;
                } else {
                    let sequence = u64::from_le_bytes(
                        data[3..11]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let entry = if index == 0 {
                        hybrid_state::cancel_owned_ask(
                            &mut head,
                            &mut ho,
                            sequence,
                            accounts[6].key.to_bytes(),
                        )
                    } else {
                        hybrid_state::cancel_owned_ask(
                            &mut tail,
                            &mut to,
                            sequence,
                            accounts[6].key.to_bytes(),
                        )
                    }
                    .map_err(|_| ProgramError::InvalidArgument)?;
                    balance
                        .unlock_base(entry.base_qty)
                        .map_err(|_| ProgramError::InvalidAccountData)?;
                    market.ask_count = market
                        .ask_count
                        .checked_sub(1)
                        .ok_or(ProgramError::InvalidAccountData)?;
                }
                store_maker_balance(&accounts[5], &balance)?;
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }
        head.validate_order()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        tail.validate_order()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if head.links().next_page == Some(1) {
            head.validate_next(&tail)
                .map_err(|_| ProgramError::InvalidAccountData)?;
        }
        head.encode_into(&mut accounts[1].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        ho.encode_into(&mut accounts[2].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        tail.encode_into(&mut accounts[3].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        to.encode_into(&mut accounts[4].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    } else {
        let mut head = load_active_bid_page(&accounts[1])?;
        let mut ho = load_owner_page_box(&accounts[2])?;
        let mut tail = load_active_bid_page(&accounts[3])?;
        let mut to = load_owner_page_box(&accounts[4])?;
        if ho.len() != head.len()
            || to.len() != tail.len()
            || ho.links() != head.links()
            || to.links() != tail.links()
        {
            return Err(ProgramError::InvalidAccountData);
        }
        if head.len().checked_add(tail.len()) != Some(market.bid_count() as usize) {
            return Err(ProgramError::InvalidAccountData);
        }
        let linked = head.links() == hybrid_state::PageLinks::new(0, None, Some(1))
            && tail.links() == hybrid_state::PageLinks::new(1, Some(0), None);
        let detached = head.links() == hybrid_state::PageLinks::new(0, None, None)
            && tail.links() == hybrid_state::PageLinks::new(1, None, None);
        match data[0] {
            29 if data.len() == 2
                && detached
                && head.is_empty()
                && tail.is_empty()
                && market.bid_count() == 0 =>
            {
                head.set_links(hybrid_state::PageLinks::new(0, None, Some(1)));
                ho.set_links(head.links());
                tail.set_links(hybrid_state::PageLinks::new(1, Some(0), None));
                to.set_links(tail.links());
            }
            32 if data.len() == 2 && linked && tail.is_empty() => {
                head.set_links(hybrid_state::PageLinks::new(0, None, None));
                ho.set_links(head.links());
                tail.set_links(hybrid_state::PageLinks::new(1, None, None));
                to.set_links(tail.links());
            }
            30 | 31 if linked => {
                if (data[0] == 30 && data.len() != 43) || (data[0] == 31 && data.len() != 11) {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if !accounts[6].is_signer || !accounts[5].is_writable {
                    return Err(ProgramError::MissingRequiredSignature);
                }
                let mut balance =
                    load_maker_balance(program_id, &accounts[0], &accounts[5], &accounts[6])?;
                let index = data[2];
                if index > 1 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                if data[0] == 30 {
                    let price = u128::from_le_bytes(
                        data[3..19]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let sqrt = u128::from_le_bytes(
                        data[19..35]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let qty = u64::from_le_bytes(
                        data[35..43]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let entry = hybrid_state::BidEntry {
                        price_x64: price,
                        sqrt_price_x64: sqrt,
                        base_qty: qty,
                        sequence: market.next_sequence,
                    };
                    if index == 0 {
                        hybrid_state::insert_owned_bid(
                            &mut head,
                            &mut ho,
                            entry,
                            accounts[6].key.to_bytes(),
                        )
                    } else {
                        hybrid_state::insert_owned_bid(
                            &mut tail,
                            &mut to,
                            entry,
                            accounts[6].key.to_bytes(),
                        )
                    }
                    .map_err(|_| ProgramError::InvalidInstructionData)?;
                    let lock = hybrid_engine::quote_for_base_at_price(qty, price)
                        .map_err(|_| ProgramError::InvalidInstructionData)?;
                    balance
                        .lock_quote(lock)
                        .map_err(|_| ProgramError::InsufficientFunds)?;
                    market.set_bid_count(
                        market
                            .bid_count()
                            .checked_add(1)
                            .ok_or(ProgramError::InvalidAccountData)?,
                    );
                    market.next_sequence = market
                        .next_sequence
                        .checked_add(1)
                        .ok_or(ProgramError::InvalidAccountData)?;
                } else {
                    let sequence = u64::from_le_bytes(
                        data[3..11]
                            .try_into()
                            .map_err(|_| ProgramError::InvalidInstructionData)?,
                    );
                    let entry = if index == 0 {
                        hybrid_state::cancel_owned_bid(
                            &mut head,
                            &mut ho,
                            sequence,
                            accounts[6].key.to_bytes(),
                        )
                    } else {
                        hybrid_state::cancel_owned_bid(
                            &mut tail,
                            &mut to,
                            sequence,
                            accounts[6].key.to_bytes(),
                        )
                    }
                    .map_err(|_| ProgramError::InvalidArgument)?;
                    let lock =
                        hybrid_engine::quote_for_base_at_price(entry.base_qty, entry.price_x64)
                            .map_err(|_| ProgramError::InvalidAccountData)?;
                    balance
                        .unlock_quote(lock)
                        .map_err(|_| ProgramError::InvalidAccountData)?;
                    market.set_bid_count(
                        market
                            .bid_count()
                            .checked_sub(1)
                            .ok_or(ProgramError::InvalidAccountData)?,
                    );
                }
                store_maker_balance(&accounts[5], &balance)?;
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }
        head.validate_order()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        tail.validate_order()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if head.links().next_page == Some(1) {
            head.validate_next(&tail)
                .map_err(|_| ProgramError::InvalidAccountData)?;
        }
        head.encode_into(&mut accounts[1].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        ho.encode_into(&mut accounts[2].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        tail.encode_into(&mut accounts[3].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        to.encode_into(&mut accounts[4].try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    market
        .encode_into(&mut accounts[0].try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)
}
