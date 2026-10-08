#![cfg(feature = "test-sbf")]

use hybrid_engine::{spot_price_x64, Q64};
use hybrid_program::ID;
use hybrid_state::{
    AskEntry, AskOwnerPage, AskPage, BidEntry, BidOwnerPage, BidPage, BoundaryEntry, BoundaryPage,
    CustodyState, MakerBalance, MarketHeader, PageLinks, ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES,
    BID_OWNER_PAGE_BYTES, BID_PAGE_BYTES, BOUNDARY_PAGE_BYTES, CUSTODY_STATE_BYTES,
    MAKER_BALANCE_BYTES, MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};
use solana_program_test::{processor, ProgramTest};
use solana_signer::Signer;
use solana_transaction::Transaction;

async fn units_for(data: Vec<u8>) -> u64 {
    let mut context = ProgramTest::new("hybrid_program", ID, None)
        .start_with_context()
        .await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_noop_cu() {
    let units = units_for(vec![0]).await;
    println!("HYBRID_CU noop {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_passive_quote_cu() {
    let units = units_for(vec![1]).await;
    println!("HYBRID_CU passive_quote {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_hybrid_match_cu() {
    let units = units_for(vec![2]).await;
    println!("HYBRID_CU hybrid_match {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_multilevel_match_cu() {
    let units = units_for(vec![3]).await;
    println!("HYBRID_CU multilevel_match {units}");
    assert!(units > 0);
}

async fn units_for_state_backed_quote(opcode: u8) -> u64 {
    let market_key = Pubkey::new_unique();
    let asks_key = Pubkey::new_unique();
    let boundaries_key = Pubkey::new_unique();

    let market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    asks.insert(AskEntry {
        price_x64: Q64,
        sqrt_price_x64: Q64,
        base_qty: 1_000,
        sequence: 1,
    })
    .unwrap();
    let higher_sqrt = Q64 + Q64 / 100;
    asks.insert(AskEntry {
        price_x64: spot_price_x64(higher_sqrt).unwrap(),
        sqrt_price_x64: higher_sqrt,
        base_qty: 2_000,
        sequence: 2,
    })
    .unwrap();
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut boundaries = BoundaryPage::default();
    boundaries.set_links(PageLinks::new(0, None, None));
    boundaries
        .insert(BoundaryEntry {
            sqrt_price_x64: Q64 + Q64 / 200,
            liquidity_after: 1_500_000,
        })
        .unwrap();
    let mut boundary_data = vec![0u8; BOUNDARY_PAGE_BYTES];
    boundaries.encode_into(&mut boundary_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    for (key, data) in [
        (market_key, market_data),
        (asks_key, ask_data),
        (boundaries_key, boundary_data),
    ] {
        program_test.add_account(
            key,
            Account {
                lamports: 1_000_000,
                data,
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    let mut context = program_test.start_with_context().await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(market_key, false),
            AccountMeta::new_readonly(asks_key, false),
            AccountMeta::new_readonly(boundaries_key, false),
        ],
        data: vec![opcode],
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_state_backed_match_cu() {
    let units = units_for_state_backed_quote(4).await;
    println!("HYBRID_CU state_backed_match {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_state_backed_plan_cu() {
    let units = units_for_state_backed_quote(10).await;
    println!("HYBRID_CU state_backed_plan {units}");
    assert!(units > 0);
}

async fn units_for_multipage_state_backed_quote() -> u64 {
    let market_key = Pubkey::new_unique();
    let (ask_0_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let (ask_1_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &1u32.to_le_bytes()],
        &ID,
    );
    let (boundary_0_key, _) = Pubkey::find_program_address(
        &[b"boundary-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let (boundary_1_key, _) = Pubkey::find_program_address(
        &[b"boundary-page", market_key.as_ref(), &1u32.to_le_bytes()],
        &ID,
    );

    let market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_data).unwrap();

    let mut ask_0 = AskPage::default();
    ask_0.set_links(PageLinks::new(0, None, Some(1)));
    let mut ask_1 = AskPage::default();
    ask_1.set_links(PageLinks::new(1, Some(0), None));
    for index in 0..16u64 {
        let sqrt_price_x64 = Q64 + u128::from(index) * 1_000_000_000_000u128;
        let entry = AskEntry {
            price_x64: spot_price_x64(sqrt_price_x64).unwrap(),
            sqrt_price_x64,
            base_qty: 1_000,
            sequence: index + 1,
        };
        if index < 8 {
            ask_0.insert(entry).unwrap();
        } else {
            ask_1.insert(entry).unwrap();
        }
    }

    let mut boundary_0 = BoundaryPage::default();
    boundary_0.set_links(PageLinks::new(0, None, Some(1)));
    let mut boundary_1 = BoundaryPage::default();
    boundary_1.set_links(PageLinks::new(1, Some(0), None));
    for index in 0..16u64 {
        let entry = BoundaryEntry {
            sqrt_price_x64: Q64 + (u128::from(index) + 1) * 500_000_000_000u128,
            liquidity_after: 1_500_000 + u128::from(index),
        };
        if index < 8 {
            boundary_0.insert(entry).unwrap();
        } else {
            boundary_1.insert(entry).unwrap();
        }
    }

    let mut ask_0_data = vec![0u8; ASK_PAGE_BYTES];
    let mut ask_1_data = vec![0u8; ASK_PAGE_BYTES];
    let mut boundary_0_data = vec![0u8; BOUNDARY_PAGE_BYTES];
    let mut boundary_1_data = vec![0u8; BOUNDARY_PAGE_BYTES];
    ask_0.encode_into(&mut ask_0_data).unwrap();
    ask_1.encode_into(&mut ask_1_data).unwrap();
    boundary_0.encode_into(&mut boundary_0_data).unwrap();
    boundary_1.encode_into(&mut boundary_1_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    for (key, data) in [
        (market_key, market_data),
        (ask_0_key, ask_0_data),
        (ask_1_key, ask_1_data),
        (boundary_0_key, boundary_0_data),
        (boundary_1_key, boundary_1_data),
    ] {
        program_test.add_account(
            key,
            Account {
                lamports: 1_000_000,
                data,
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    let mut context = program_test.start_with_context().await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(market_key, false),
            AccountMeta::new_readonly(ask_0_key, false),
            AccountMeta::new_readonly(ask_1_key, false),
            AccountMeta::new_readonly(boundary_0_key, false),
            AccountMeta::new_readonly(boundary_1_key, false),
        ],
        data: vec![5, 2, 2],
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_multipage_state_backed_match_cu() {
    let units = units_for_multipage_state_backed_quote().await;
    println!("HYBRID_CU multipage_state_backed_match {units}");
    assert!(units > 0);
}

async fn units_for_active_order_mutation(cancel: bool) -> u64 {
    let market_key = Pubkey::new_unique();
    let (ask_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let owner_key = Pubkey::new_unique();
    let maker = Keypair::new();
    let (balance_key, balance_bump) = Pubkey::find_program_address(
        &[
            b"maker-balance",
            market_key.as_ref(),
            maker.pubkey().as_ref(),
        ],
        &ID,
    );

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    market.reserved2 = owner_key.to_bytes();
    market.enable_collateralized_active();
    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));

    if cancel {
        market.ask_count = 1;
        market.next_sequence = 2;
        hybrid_state::insert_owned_ask(
            &mut asks,
            &mut owners,
            AskEntry {
                price_x64: Q64,
                sqrt_price_x64: Q64,
                base_qty: 1_000,
                sequence: 1,
            },
            maker.pubkey().to_bytes(),
        )
        .unwrap();
    }

    let mut balance = MakerBalance::new(
        balance_bump,
        market_key.to_bytes(),
        maker.pubkey().to_bytes(),
    );
    balance.free_base = 10_000;
    if cancel {
        balance.free_base = 9_000;
        balance.locked_base = 1_000;
    }
    let mut balance_data = vec![0u8; MAKER_BALANCE_BYTES];
    balance.encode_into(&mut balance_data).unwrap();

    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    let mut owner_data = vec![0u8; ASK_OWNER_PAGE_BYTES];
    market.encode_into(&mut market_data).unwrap();
    asks.encode_into(&mut ask_data).unwrap();
    owners.encode_into(&mut owner_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    for (key, data) in [
        (market_key, market_data),
        (ask_key, ask_data),
        (owner_key, owner_data),
        (balance_key, balance_data),
    ] {
        program_test.add_account(
            key,
            Account {
                lamports: 1_000_000,
                data,
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    }
    program_test.add_account(
        maker.pubkey(),
        Account {
            lamports: 1_000_000,
            data: Vec::new(),
            owner: Pubkey::default(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut data = if cancel {
        let mut bytes = vec![7];
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes
    } else {
        let mut bytes = vec![6];
        bytes.extend_from_slice(&Q64.to_le_bytes());
        bytes.extend_from_slice(&Q64.to_le_bytes());
        bytes.extend_from_slice(&1_000u64.to_le_bytes());
        bytes
    };

    let mut context = program_test.start_with_context().await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(ask_key, false),
            AccountMeta::new(owner_key, false),
            AccountMeta::new(balance_key, false),
            AccountMeta::new_readonly(maker.pubkey(), true),
        ],
        data: core::mem::take(&mut data),
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_place_ask_cu() {
    let units = units_for_active_order_mutation(false).await;
    println!("HYBRID_CU place_ask {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_cancel_ask_cu() {
    let units = units_for_active_order_mutation(true).await;
    println!("HYBRID_CU cancel_ask {units}");
    assert!(units > 0);
}

async fn units_for_bid_order_mutation(cancel: bool) -> u64 {
    let market_key = Pubkey::new_unique();
    let (bid_key, _) = Pubkey::find_program_address(
        &[b"bid-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let owner_key = Pubkey::new_unique();
    let maker = Keypair::new();
    let (balance_key, balance_bump) = Pubkey::find_program_address(
        &[
            b"maker-balance",
            market_key.as_ref(),
            maker.pubkey().as_ref(),
        ],
        &ID,
    );

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    market.enable_collateralized_active();
    let mut bids = BidPage::default();
    bids.set_links(PageLinks::new(0, None, None));
    let mut owners = BidOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));

    let owner_bytes = owner_key.to_bytes();
    let mut owner_tag = [0u8; 16];
    owner_tag.copy_from_slice(&owner_bytes[..16]);
    market.set_bid_owner_tag(owner_tag);

    if cancel {
        market.set_bid_count(1);
        market.next_sequence = 2;
        hybrid_state::insert_owned_bid(
            &mut bids,
            &mut owners,
            BidEntry {
                price_x64: Q64,
                sqrt_price_x64: Q64,
                base_qty: 1_000,
                sequence: 1,
            },
            maker.pubkey().to_bytes(),
        )
        .unwrap();
    }

    let mut balance = MakerBalance::new(
        balance_bump,
        market_key.to_bytes(),
        maker.pubkey().to_bytes(),
    );
    balance.free_quote = 10_000;
    if cancel {
        balance.free_quote = 9_000;
        balance.locked_quote = 1_000;
    }
    let mut balance_data = vec![0u8; MAKER_BALANCE_BYTES];
    balance.encode_into(&mut balance_data).unwrap();

    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    let mut bid_data = vec![0u8; BID_PAGE_BYTES];
    let mut owner_data = vec![0u8; BID_OWNER_PAGE_BYTES];
    market.encode_into(&mut market_data).unwrap();
    bids.encode_into(&mut bid_data).unwrap();
    owners.encode_into(&mut owner_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    for (key, data) in [
        (market_key, market_data),
        (bid_key, bid_data),
        (owner_key, owner_data),
        (balance_key, balance_data),
    ] {
        program_test.add_account(
            key,
            Account {
                lamports: 1_000_000,
                data,
                owner: ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    }
    program_test.add_account(
        maker.pubkey(),
        Account {
            lamports: 1_000_000,
            data: Vec::new(),
            owner: Pubkey::default(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let data = if cancel {
        let mut bytes = vec![9];
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes
    } else {
        let mut bytes = vec![8];
        bytes.extend_from_slice(&Q64.to_le_bytes());
        bytes.extend_from_slice(&Q64.to_le_bytes());
        bytes.extend_from_slice(&1_000u64.to_le_bytes());
        bytes
    };

    let mut context = program_test.start_with_context().await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(bid_key, false),
            AccountMeta::new(owner_key, false),
            AccountMeta::new(balance_key, false),
            AccountMeta::new_readonly(maker.pubkey(), true),
        ],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_place_bid_cu() {
    let units = units_for_bid_order_mutation(false).await;
    println!("HYBRID_CU place_bid {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_cancel_bid_cu() {
    let units = units_for_bid_order_mutation(true).await;
    println!("HYBRID_CU cancel_bid {units}");
    assert!(units > 0);
}

const TOKEN_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn cu_token_account_data(mint: Pubkey, authority: Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(authority.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    data
}

fn cu_token_amount(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

fn cu_mock_token(_program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 9 || data[0] != 3 || accounts.len() < 3 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let mut source = accounts[0].try_borrow_mut_data()?;
    let mut destination = accounts[1].try_borrow_mut_data()?;
    let next_source = cu_token_amount(&source)
        .checked_sub(amount)
        .ok_or(ProgramError::InsufficientFunds)?;
    let next_destination = cu_token_amount(&destination)
        .checked_add(amount)
        .ok_or(ProgramError::InvalidAccountData)?;
    source[64..72].copy_from_slice(&next_source.to_le_bytes());
    destination[64..72].copy_from_slice(&next_destination.to_le_bytes());
    Ok(())
}

async fn units_for_buy_swap(opcode: u8) -> u64 {
    let market = Pubkey::new_unique();
    let (custody, custody_bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (ask_page, _) =
        Pubkey::find_program_address(&[b"ask-page", market.as_ref(), &0u32.to_le_bytes()], &ID);
    let owner_page = Pubkey::new_unique();
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();
    let taker = Keypair::new();
    let taker_quote = Pubkey::new_unique();
    let taker_base = Pubkey::new_unique();
    let maker = Pubkey::new_unique();
    let (maker_balance, maker_bump) =
        Pubkey::find_program_address(&[b"maker-balance", market.as_ref(), maker.as_ref()], &ID);

    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.ask_count = 1;
    header.next_sequence = 2;
    header.reserved2 = owner_page.to_bytes();
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    asks.insert(AskEntry {
        price_x64: Q64,
        sqrt_price_x64: Q64,
        base_qty: 1_000,
        sequence: 1,
    })
    .unwrap();
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));
    owners.len = 1;
    owners.owners[0] = maker.to_bytes();
    let mut owner_data = vec![0u8; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();

    let custody_state = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump: custody_bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: market.to_bytes(),
        base_mint: base_mint.to_bytes(),
        quote_mint: quote_mint.to_bytes(),
        base_vault: base_vault.to_bytes(),
        quote_vault: quote_vault.to_bytes(),
        total_base: 2_000,
        total_quote: 0,
    };
    let mut custody_data = vec![0u8; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();

    let mut maker_state = MakerBalance::new(maker_bump, market.to_bytes(), maker.to_bytes());
    maker_state.locked_base = 1_000;
    let mut maker_data = vec![0u8; MAKER_BALANCE_BYTES];
    maker_state.encode_into(&mut maker_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.prefer_bpf(false);
    program_test.add_program("mock_token", TOKEN_PROGRAM_ID, processor!(cu_mock_token));
    program_test.prefer_bpf(true);
    for (key, data, owner) in [
        (market, market_data, ID),
        (custody, custody_data, ID),
        (ask_page, ask_data, ID),
        (owner_page, owner_data, ID),
        (maker_balance, maker_data, ID),
        (
            base_vault,
            cu_token_account_data(base_mint, vault_authority, 2_000),
            TOKEN_PROGRAM_ID,
        ),
        (
            quote_vault,
            cu_token_account_data(quote_mint, vault_authority, 0),
            TOKEN_PROGRAM_ID,
        ),
        (
            taker_quote,
            cu_token_account_data(quote_mint, taker.pubkey(), 2_000),
            TOKEN_PROGRAM_ID,
        ),
        (
            taker_base,
            cu_token_account_data(base_mint, taker.pubkey(), 0),
            TOKEN_PROGRAM_ID,
        ),
    ] {
        program_test.add_account(
            key,
            Account {
                lamports: 1_000_000,
                data,
                owner,
                executable: false,
                rent_epoch: 0,
            },
        );
    }
    program_test.add_account(
        taker.pubkey(),
        Account {
            lamports: 1_000_000,
            data: Vec::new(),
            owner: Pubkey::default(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut context = program_test.start_with_context().await;
    let mut data = vec![opcode];
    data.extend_from_slice(&100u64.to_le_bytes());
    data.extend_from_slice(&100u64.to_le_bytes());
    let ix = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market, false),
            AccountMeta::new(custody, false),
            AccountMeta::new(ask_page, false),
            AccountMeta::new(owner_page, false),
            AccountMeta::new_readonly(taker.pubkey(), true),
            AccountMeta::new(taker_quote, false),
            AccountMeta::new(taker_base, false),
            AccountMeta::new(quote_vault, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new(maker_balance, false),
        ],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &taker],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_swap_buy_exact_in_cu() {
    let units = units_for_buy_swap(15).await;
    println!("HYBRID_CU swap_buy_exact_in {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_swap_buy_exact_out_cu() {
    let units = units_for_buy_swap(16).await;
    println!("HYBRID_CU swap_buy_exact_out {units}");
    assert!(units > 0);
}
