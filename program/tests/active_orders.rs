#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::{
    AskOwnerPage, AskPage, MarketHeader, PageLinks, ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES,
    MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::ProgramTest;
use solana_signer::Signer;
use solana_transaction::Transaction;

fn place_data(price_x64: u128, sqrt_price_x64: u128, base_qty: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(41);
    data.push(6);
    data.extend_from_slice(&price_x64.to_le_bytes());
    data.extend_from_slice(&sqrt_price_x64.to_le_bytes());
    data.extend_from_slice(&base_qty.to_le_bytes());
    data
}

fn cancel_data(sequence: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(9);
    data.push(7);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

fn account(data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports: 1_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

#[tokio::test]
async fn place_and_cancel_enforce_owner_and_persist_counts() {
    let market_key = Pubkey::new_unique();
    let (ask_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let (owner_key, _) = Pubkey::find_program_address(
        &[b"ask-owner-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let maker = Keypair::new();
    let other = Keypair::new();

    let market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.add_account(market_key, account(market_data, ID));
    program_test.add_account(ask_key, account(ask_data, ID));
    program_test.add_account(
        maker.pubkey(),
        account(Vec::new(), solana_system_interface::program::ID),
    );
    program_test.add_account(
        other.pubkey(),
        account(Vec::new(), solana_system_interface::program::ID),
    );

    let mut context = program_test.start_with_context().await;

    let init = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(market_key, false),
            AccountMeta::new(owner_key, false),
            AccountMeta::new(context.payer.pubkey(), true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![8],
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let initialized_owner = context
        .banks_client
        .get_account(owner_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(initialized_owner.owner, ID);
    assert_eq!(initialized_owner.data.len(), ASK_OWNER_PAGE_BYTES);
    assert_eq!(
        AskOwnerPage::decode_from(&initialized_owner.data)
            .unwrap()
            .len(),
        0
    );

    let common = |maker_key, signer| Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(ask_key, false),
            AccountMeta::new(owner_key, false),
            if signer {
                AccountMeta::new_readonly(maker_key, true)
            } else {
                AccountMeta::new_readonly(maker_key, false)
            },
        ],
        data: Vec::new(),
    };

    let mut place = common(maker.pubkey(), true);
    place.data = place_data(Q64, Q64, 100);
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[place],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let stored_market = context
        .banks_client
        .get_account(market_key)
        .await
        .unwrap()
        .unwrap();
    let market = MarketHeader::decode_from(&stored_market.data).unwrap();
    assert_eq!(market.ask_count, 1);
    assert_eq!(market.next_sequence, 2);

    let stored_asks = context
        .banks_client
        .get_account(ask_key)
        .await
        .unwrap()
        .unwrap();
    let asks = AskPage::decode_from(&stored_asks.data).unwrap();
    assert_eq!(asks.as_slice()[0].sequence, 1);
    assert_eq!(asks.as_slice()[0].base_qty, 100);

    let stored_owners = context
        .banks_client
        .get_account(owner_key)
        .await
        .unwrap()
        .unwrap();
    let owners = AskOwnerPage::decode_from(&stored_owners.data).unwrap();
    assert_eq!(owners.as_slice()[0], maker.pubkey().to_bytes());

    let mut unauthorized = common(other.pubkey(), true);
    unauthorized.data = cancel_data(1);
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[unauthorized],
        Some(&context.payer.pubkey()),
        &[&context.payer, &other],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());

    let mut cancel = common(maker.pubkey(), true);
    cancel.data = cancel_data(1);
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[cancel],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let stored_market = context
        .banks_client
        .get_account(market_key)
        .await
        .unwrap()
        .unwrap();
    let market = MarketHeader::decode_from(&stored_market.data).unwrap();
    assert_eq!(market.ask_count, 0);
    assert_eq!(market.next_sequence, 2);

    let stored_asks = context
        .banks_client
        .get_account(ask_key)
        .await
        .unwrap()
        .unwrap();
    assert!(AskPage::decode_from(&stored_asks.data).unwrap().is_empty());

    let stored_owners = context
        .banks_client
        .get_account(owner_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        AskOwnerPage::decode_from(&stored_owners.data)
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn place_rejects_wrong_sidecar_pda_and_missing_signature() {
    let market_key = Pubkey::new_unique();
    let (ask_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let wrong_owner_key = Pubkey::new_unique();
    let maker = Keypair::new();

    let market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));
    let mut owner_data = vec![0u8; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.add_account(market_key, account(market_data, ID));
    program_test.add_account(ask_key, account(ask_data, ID));
    program_test.add_account(wrong_owner_key, account(owner_data, ID));
    program_test.add_account(
        maker.pubkey(),
        account(Vec::new(), solana_system_interface::program::ID),
    );
    let mut context = program_test.start_with_context().await;

    let wrong_pda = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(ask_key, false),
            AccountMeta::new(wrong_owner_key, false),
            AccountMeta::new_readonly(maker.pubkey(), true),
        ],
        data: place_data(Q64, Q64, 10),
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[wrong_pda],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());

    let missing_signature = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(ask_key, false),
            AccountMeta::new(wrong_owner_key, false),
            AccountMeta::new_readonly(maker.pubkey(), false),
        ],
        data: place_data(Q64, Q64, 10),
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[missing_signature],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());
}
