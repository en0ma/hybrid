#![cfg(feature = "test-sbf")]

use hybrid_engine::{spot_price_x64, Q64};
use hybrid_program::ID;
use hybrid_state::{
    AskEntry, AskOwnerPage, AskPage, BidEntry, BidOwnerPage, BidPage, BoundaryEntry, BoundaryPage,
    MarketHeader, PageLinks, ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES, BID_OWNER_PAGE_BYTES,
    BID_PAGE_BYTES, BOUNDARY_PAGE_BYTES, MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::ProgramTest;
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

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    market.reserved2 = owner_key.to_bytes();
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

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
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
