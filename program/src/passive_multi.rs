//! Authenticated, funded multi-position passive settlement, one price segment.
//! LP fee claims remain segregated from current inventory.
use crate::{
    check_passive_vault_coverage, load_custody, token_account_fields, transfer_checked,
    TOKEN_PROGRAM_ID,
};
use hybrid_engine::{
    passive_exact_out::{buy_exact_out, sell_exact_out},
    passive_sell_to_target, passive_to_target, quote_base_in_for_quote_out,
    quote_quote_in_for_base_out, PassiveState,
};
use hybrid_state::{
    passive::{PoolAccount, PositionAccount},
    passive_segments::{RangeClaim, SegmentTrade, SegmentedPool},
};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

/// Opcode 35 exact-in / 36 exact-out. Data: opcode, side, input-or-max:u64,
/// minimum-output:u64, fee_bps:u16, number_of_positions:u8 (21 bytes).
/// Accounts: market, custody, pool, taker signer, taker base/quote, base/quote
/// vault, base/quote mint, vault authority, SPL Token program, then ALL
/// position PDAs in ascending Pubkey byte order, each writable.
/// Execute up to eight separately verified segments atomically. Any failed
/// leg causes Solana to roll back all earlier CPI transfers and state writes.
pub fn process_batch(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 2 || data[0] != 37 || data[1] == 0 || data[1] > 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let count = usize::from(data[1]);
    if data.len() < 23 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let positions = usize::from(data[22]);
    if !(1..=8).contains(&positions) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let guard_start = 2 + count * 21;
    if data.len() != guard_start + 24 + 8 * positions {
        return Err(ProgramError::InvalidInstructionData);
    }
    let guard = &data[guard_start..];
    for (i, segment) in data[2..guard_start].as_chunks::<21>().0.iter().enumerate() {
        if usize::from(segment[20]) != positions {
            return Err(ProgramError::InvalidInstructionData);
        }
        process_segment(
            program_id,
            accounts,
            segment,
            if i == 0 { Some(guard) } else { None },
        )?;
    }
    Ok(())
}

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 21 || data.len() != 45 + 8 * usize::from(data[20]) {
        return Err(ProgramError::InvalidInstructionData);
    }
    process_segment(program_id, accounts, &data[..21], Some(&data[21..]))
}

fn process_segment(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
    guard: Option<&[u8]>,
) -> ProgramResult {
    if data.len() != 21
        || !matches!(data[0], 35 | 36)
        || data[1] > 1
        || data[20] == 0
        || data[20] > 8
        || accounts.len() < 12 + usize::from(data[20])
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let exact_out = data[0] == 36;
    let buy = data[1] == 0;
    let input = u64::from_le_bytes(
        data[2..10]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let output = u64::from_le_bytes(
        data[10..18]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let bps = u16::from_le_bytes(
        data[18..20]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if input == 0 || output == 0 || bps != crate::PASSIVE_FEE_BPS {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let pool_account = &accounts[2];
    let taker = &accounts[3];
    let taker_base = &accounts[4];
    let taker_quote = &accounts[5];
    let base_vault = &accounts[6];
    let quote_vault = &accounts[7];
    let base_mint = &accounts[8];
    let quote_mint = &accounts[9];
    let authority = &accounts[10];
    let token_program = &accounts[11];
    let (positions, book) = accounts[12..].split_at(usize::from(data[20]));
    if !taker.is_signer
        || !market.is_writable
        || !pool_account.is_writable
        || !taker_base.is_writable
        || !taker_quote.is_writable
        || !base_vault.is_writable
        || !quote_vault.is_writable
        || market.owner != program_id
        || pool_account.owner != program_id
        || positions
            .iter()
            .any(|a| a.owner != program_id || !a.is_writable)
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let custody = load_custody(program_id, market, custody_account)?;
    let mut header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !header.collateralized_active() || header.boundary_count != 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    let active_limit = active_price_limit(program_id, market.key, &header, buy, book)?;
    if let Some(snapshot) = guard {
        if header.sqrt_price_x64 != u128::from_le_bytes(snapshot[..16].try_into().unwrap())
            || header.next_sequence != u64::from_le_bytes(snapshot[16..24].try_into().unwrap())
        {
            return Err(ProgramError::InvalidInstructionData);
        }
    }
    let mut pool = PoolAccount::decode_from(&pool_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    let (pool_key, pool_bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    let (authority_key, authority_bump) =
        Pubkey::find_program_address(&[b"vault-authority", market.key.as_ref()], program_id);
    if *pool_account.key != pool_key
        || pool.bump != pool_bump
        || pool.market != market.key.to_bytes()
        || *authority.key != authority_key
        || *token_program.key != TOKEN_PROGRAM_ID
        || *base_vault.key != Pubkey::new_from_array(custody.base_vault)
        || *quote_vault.key != Pubkey::new_from_array(custody.quote_vault)
        || *base_mint.key != Pubkey::new_from_array(custody.base_mint)
        || *quote_mint.key != Pubkey::new_from_array(custody.quote_mint)
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let all_keys = [
        taker_base.key,
        taker_quote.key,
        base_vault.key,
        quote_vault.key,
    ];
    for i in 0..4 {
        for j in (i + 1)..4 {
            if all_keys[i] == all_keys[j] {
                return Err(ProgramError::InvalidAccountData);
            }
        }
    }
    let (tb_mint, tb_owner) = token_account_fields(taker_base)?;
    let (tq_mint, tq_owner) = token_account_fields(taker_quote)?;
    let (bv_mint, bv_owner) = token_account_fields(base_vault)?;
    let (qv_mint, qv_owner) = token_account_fields(quote_vault)?;
    if tb_owner != taker.key.to_bytes()
        || tq_owner != taker.key.to_bytes()
        || tb_mint != custody.base_mint
        || tq_mint != custody.quote_mint
        || bv_mint != custody.base_mint
        || qv_mint != custody.quote_mint
        || bv_owner != authority_key.to_bytes()
        || qv_owner != authority_key.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut states = Vec::with_capacity(positions.len());
    let mut claims = Vec::with_capacity(positions.len());
    for (i, account) in positions.iter().enumerate() {
        if i > 0 && positions[i - 1].key.as_ref() >= account.key.as_ref() {
            return Err(ProgramError::InvalidAccountData);
        }
        let state = PositionAccount::decode_from(&account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if let Some(snapshot) = guard {
            if state.revision
                != u64::from_le_bytes(snapshot[24 + i * 8..32 + i * 8].try_into().unwrap())
            {
                return Err(ProgramError::InvalidInstructionData);
            }
        }
        let owner = Pubkey::new_from_array(state.position.owner);
        let (expected, bump) = Pubkey::find_program_address(
            &[
                b"passive-position",
                market.key.as_ref(),
                owner.as_ref(),
                &state.nonce.to_le_bytes(),
            ],
            program_id,
        );
        if expected != *account.key || state.bump != bump || state.market != market.key.to_bytes() {
            return Err(ProgramError::InvalidSeeds);
        }
        claims.push(RangeClaim {
            range: hybrid_engine::range_position::RangePosition {
                lower_sqrt_price_x64: state.position.lower_sqrt_price_x64,
                upper_sqrt_price_x64: state.position.upper_sqrt_price_x64,
                liquidity: state.position.liquidity,
            },
            base: state.position.base_principal,
            quote: state.position.quote_principal,
            base_fees: state.base_fees,
            quote_fees: state.quote_fees,
        });
        states.push(state);
    }
    let mut ledger = SegmentedPool {
        pool: pool.pool,
        positions: claims,
    };
    ledger
        .verify()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    let mut price = header.sqrt_price_x64;
    let mut gross = 0u64;
    let mut total_out = 0u64;
    // A monotone route crosses at most two endpoints per supplied LP.
    for _ in 0..=2 * positions.len() {
        let remaining = input - gross;
        if remaining == 0 || (exact_out && total_out >= output) {
            break;
        }
        let segment_price = if buy {
            price
        } else {
            price
                .checked_sub(1)
                .ok_or(ProgramError::InvalidInstructionData)?
        };
        let mut liquidity = 0u128;
        if active_limit.is_some_and(|edge| if buy { price >= edge } else { price <= edge }) {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut boundary = active_limit;
        for claim in &ledger.positions {
            if claim
                .range
                .active_at(segment_price)
                .map_err(|_| ProgramError::InvalidAccountData)?
            {
                liquidity = liquidity
                    .checked_add(claim.range.liquidity)
                    .ok_or(ProgramError::InvalidAccountData)?;
            }
            for edge in [
                claim.range.lower_sqrt_price_x64,
                claim.range.upper_sqrt_price_x64,
            ] {
                if buy && edge > price && boundary.is_none_or(|b| edge < b)
                    || !buy && edge < price && boundary.is_none_or(|b| edge > b)
                {
                    boundary = Some(edge);
                }
            }
        }
        if liquidity == 0 {
            return Err(ProgramError::InsufficientFunds);
        }
        let state = PassiveState {
            sqrt_price_x64: price,
            liquidity,
        };
        let fee_for = |amount: u64| -> u64 {
            ((u128::from(amount) * u128::from(bps)).div_ceil(10_000)) as u64
        };
        let gross_for = |net: u64| -> Result<u64, ProgramError> {
            u64::try_from((u128::from(net) * 10_000).div_ceil(10_000 - u128::from(bps)))
                .map_err(|_| ProgramError::InvalidInstructionData)
        };
        let net = remaining - fee_for(remaining);
        let mut leg_gross = remaining;
        // Compute the boundary cost before attempting an unbounded quote.
        // If affordable, stop exactly there, not one rounded atom beyond.
        let boundary_quote = boundary.and_then(|edge| {
            if buy {
                passive_to_target(state, edge).ok()
            } else {
                passive_sell_to_target(state, edge).ok()
            }
        });
        let mut quote = if let Some(target) = boundary_quote.filter(|q| q.amount_in <= net) {
            leg_gross = gross_for(target.amount_in)?;
            target
        } else {
            if buy {
                quote_quote_in_for_base_out(state, net)
            } else {
                quote_base_in_for_quote_out(state, net)
            }
            .map_err(|_| ProgramError::InvalidInstructionData)?
        };
        if exact_out && quote.amount_out >= output - total_out {
            let target = output - total_out;
            // A boundary payment is already the conservative quote at that edge.
            // Use the ordinary exact-out solver only if it stays in this segment.
            let exact = if buy {
                buy_exact_out(state, target)
            } else {
                sell_exact_out(state, target)
            };
            if let Ok(exact) = exact {
                let cost = gross_for(exact.amount_in)?;
                if cost <= leg_gross
                    && boundary.is_none_or(|edge| {
                        if buy {
                            exact.next_sqrt_price_x64 <= edge
                        } else {
                            exact.next_sqrt_price_x64 >= edge
                        }
                    })
                {
                    quote = exact;
                    leg_gross = cost;
                }
            }
        }
        if let Some(edge) = boundary {
            if buy && quote.next_sqrt_price_x64 > edge || !buy && quote.next_sqrt_price_x64 < edge {
                return Err(ProgramError::InvalidInstructionData);
            }
        }
        let fee = fee_for(leg_gross);
        if leg_gross == 0 || fee == 0 || quote.amount_out == 0 || quote.next_sqrt_price_x64 == price
        {
            return Err(ProgramError::InvalidInstructionData);
        }
        let trade = if buy {
            SegmentTrade::Buy {
                quote_in: leg_gross,
                base_out: quote.amount_out,
                quote_fee: fee,
            }
        } else {
            SegmentTrade::Sell {
                base_in: leg_gross,
                quote_out: quote.amount_out,
                base_fee: fee,
            }
        };
        ledger
            .apply_segment(segment_price, trade)
            .map_err(|_| ProgramError::InsufficientFunds)?;
        gross = gross
            .checked_add(leg_gross)
            .ok_or(ProgramError::InvalidInstructionData)?;
        total_out = total_out
            .checked_add(quote.amount_out)
            .ok_or(ProgramError::InvalidInstructionData)?;
        price = quote.next_sqrt_price_x64;
    }
    if total_out < output || (!exact_out && gross != input) {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bump = [authority_bump];
    let seeds: &[&[u8]] = &[b"vault-authority", market.key.as_ref(), &bump];
    if buy {
        transfer_checked(
            taker_quote,
            quote_mint,
            quote_vault,
            taker,
            token_program,
            (gross, custody.quote_decimals),
            None,
        )?;
        transfer_checked(
            base_vault,
            base_mint,
            taker_base,
            authority,
            token_program,
            (total_out, custody.base_decimals),
            Some(seeds),
        )?;
    } else {
        transfer_checked(
            taker_base,
            base_mint,
            base_vault,
            taker,
            token_program,
            (gross, custody.base_decimals),
            None,
        )?;
        transfer_checked(
            quote_vault,
            quote_mint,
            taker_quote,
            authority,
            token_program,
            (total_out, custody.quote_decimals),
            Some(seeds),
        )?;
    }
    for (i, account) in positions.iter().enumerate() {
        states[i].position.base_principal = ledger.positions[i].base;
        states[i].position.quote_principal = ledger.positions[i].quote;
        states[i].base_fees = ledger.positions[i].base_fees;
        states[i].quote_fees = ledger.positions[i].quote_fees;
        states[i].revision = states[i]
            .revision
            .checked_add(1)
            .ok_or(ProgramError::InvalidAccountData)?;
        states[i]
            .encode_into(&mut account.try_borrow_mut_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    }
    pool.pool = ledger.pool;
    check_passive_vault_coverage(&custody, &pool.pool, base_vault, quote_vault)?;
    header.sqrt_price_x64 = price;
    header
        .encode_into(&mut market.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.encode_into(&mut pool_account.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}

/// Opcode 46: one atomic, same-direction route of up to eight legs.
/// Each 21-byte leg uses the passive ABI or an active exact-in/out opcode
/// (23..26), side, amount, limit, zero fee, and page count.
/// Prefix: normal passive accounts and LP roster, canonical lazy reserve,
/// then page/owner pairs and maker balances. Header: 46, legs, LPs, pages.
/// The initial snapshot follows the legs; each leg observes live state.
#[inline(never)]
pub fn process_hybrid(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 4
        || !(1..=8).contains(&data[1])
        || !(1..=8).contains(&data[2])
        || !(1..=8).contains(&data[3])
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let n = usize::from(data[2]);
    let pages = usize::from(data[3]);
    let end = 4 + usize::from(data[1]) * 21;
    let fixed = 12 + n;
    if data.len() != end + 24 + 8 * n || accounts.len() <= fixed + 2 * pages + 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let reserve = crate::lazy_reserve::load_lazy_reserve(program_id, market, &accounts[fixed])?;
    let book = &accounts[fixed + 1..fixed + 1 + 2 * pages];
    let makers = &accounts[fixed + 1 + 2 * pages..];
    let side = data[5];
    if side > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let buy = side == 0;
    let mut passive = accounts[..fixed].to_vec();
    passive.extend(book.iter().step_by(2).cloned());
    let snapshot = &data[end..];
    let header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if header.sqrt_price_x64 != u128::from_le_bytes(snapshot[..16].try_into().unwrap())
        || header.next_sequence != u64::from_le_bytes(snapshot[16..24].try_into().unwrap())
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    for (i, account) in accounts[12..fixed].iter().enumerate() {
        let state = PositionAccount::decode_from(&account.try_borrow_data()?)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if state.revision
            != u64::from_le_bytes(snapshot[24 + i * 8..32 + i * 8].try_into().unwrap())
        {
            return Err(ProgramError::InvalidInstructionData);
        }
    }
    let mut active = Vec::new();
    let indices = if buy {
        [0, 1, 3, 5, 4, 7, 6, 10, 11]
    } else {
        [0, 1, 3, 4, 5, 6, 7, 10, 11]
    };
    active.extend(indices.iter().map(|&i| accounts[i].clone()));
    active.extend(book.iter().cloned());
    active.extend(makers.iter().cloned());
    for leg in data[4..end].as_chunks::<21>().0 {
        if leg[1] != side {
            return Err(ProgramError::InvalidInstructionData);
        }
        check_hybrid_coverage(program_id, accounts, &reserve)?;
        if matches!(leg[0], 35 | 36) && usize::from(leg[20]) == n {
            process_segment(program_id, &passive, leg, None)?;
        } else if matches!((buy, leg[0]), (true, 23 | 24) | (false, 25 | 26))
            && leg[18..20] == [0, 0]
            && usize::from(leg[20]) == pages
        {
            let h = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
                .map_err(|_| ProgramError::InvalidAccountData)?;
            let edge = active_price_limit(program_id, market.key, &h, buy, &passive[fixed..])?
                .ok_or(ProgramError::InvalidInstructionData)?;
            // Only execute the active book at its best price. Do not skip
            // passive liquidity or allow an active leg to sweep worse ticks.
            if edge != h.sqrt_price_x64 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let mut instruction = [0u8; 18];
            instruction[0] = leg[0];
            instruction[1] = data[3];
            instruction[2..].copy_from_slice(&leg[2..18]);
            // The existing active handler authenticates owners and all makers.
            if buy {
                crate::multipage_buy::process_multipage_buy_bounded(
                    program_id,
                    &active,
                    &instruction,
                    Some(edge),
                )?;
            } else {
                crate::multipage_sell::process_multipage_sell_bounded(
                    program_id,
                    &active,
                    &instruction,
                    Some(edge),
                )?;
            }
        } else {
            return Err(ProgramError::InvalidInstructionData);
        }
    }
    check_hybrid_coverage(program_id, accounts, &reserve)
}

/// Verify the sum of all three liability classes against canonical vaults.
#[inline(never)]
fn check_hybrid_coverage(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    reserve: &hybrid_state::lazy_reserve::LazyReserve,
) -> ProgramResult {
    let market = &accounts[0];
    let custody = load_custody(program_id, market, &accounts[1])?;
    let pool_account = &accounts[2];
    let (key, bump) =
        Pubkey::find_program_address(&[b"passive-pool", market.key.as_ref()], program_id);
    let pool = PoolAccount::decode_from(&pool_account.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if pool_account.owner != program_id
        || *pool_account.key != key
        || pool.bump != bump
        || pool.market != market.key.to_bytes()
        || accounts[6].key.to_bytes() != custody.base_vault
        || accounts[7].key.to_bytes() != custody.quote_vault
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let (base, quote) = reserve
        .vault_floor(custody.total_base, custody.total_quote)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    pool.pool
        .verify_vault_coverage(
            base,
            quote,
            crate::checked_token_amount(&accounts[6])?,
            crate::checked_token_amount(&accounts[7])?,
        )
        .map_err(|_| ProgramError::InsufficientFunds)
}

/// Authenticate all pages of the relevant side, including cross-page priority.
fn active_price_limit(
    program_id: &Pubkey,
    market: &Pubkey,
    header: &hybrid_state::MarketHeader,
    buy: bool,
    book: &[AccountInfo],
) -> Result<Option<u128>, ProgramError> {
    let count = if buy {
        header.ask_count as usize
    } else {
        header.bid_count() as usize
    };
    if book.is_empty() {
        return if count == 0 {
            Ok(None)
        } else {
            Err(ProgramError::InvalidAccountData)
        };
    }
    if book.len() > 8 {
        return Err(ProgramError::InvalidAccountData);
    }
    let seed: &[u8] = if buy { b"ask-page" } else { b"bid-page" };
    let mut total = 0usize;
    let mut best = None;
    let mut previous: Option<(u128, u64)> = None;
    for (index, account) in book.iter().enumerate() {
        let (expected, _) = Pubkey::find_program_address(
            &[seed, market.as_ref(), &(index as u32).to_le_bytes()],
            program_id,
        );
        if account.owner != program_id || *account.key != expected {
            return Err(ProgramError::InvalidSeeds);
        }
        let (len, links, first, last) = if buy {
            let page = crate::load_active_ask_page(account)?;
            (
                page.len(),
                page.links(),
                page.as_slice()
                    .first()
                    .map(|e| (e.price_x64, e.sequence, e.sqrt_price_x64)),
                page.as_slice().last().map(|e| (e.price_x64, e.sequence)),
            )
        } else {
            let page = crate::load_active_bid_page(account)?;
            (
                page.len(),
                page.links(),
                page.as_slice()
                    .first()
                    .map(|e| (e.price_x64, e.sequence, e.sqrt_price_x64)),
                page.as_slice().last().map(|e| (e.price_x64, e.sequence)),
            )
        };
        if links.page_index != index as u32
            || links.prev_page != index.checked_sub(1).map(|i| i as u32)
            || links.next_page != (index + 1 < book.len()).then_some((index + 1) as u32)
        {
            return Err(ProgramError::InvalidAccountData);
        }
        if let Some((price, sequence, sqrt)) = first {
            if let Some((prior, seq)) = previous {
                if (buy && prior > price)
                    || (!buy && prior < price)
                    || (prior == price && seq >= sequence)
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            if best.is_none() {
                best = Some(sqrt);
            }
            previous = last;
        }
        total += len;
    }
    if total != count {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(best)
}
