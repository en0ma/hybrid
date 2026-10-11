#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::{
    AskOwnerPage, BidEntry, BidPage, CustodyState, MakerBalance, MarketHeader, PageLinks,
    ASK_OWNER_PAGE_BYTES, BID_PAGE_BYTES, CUSTODY_STATE_BYTES, MAKER_BALANCE_BYTES,
    MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::{processor, ProgramTest};
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn account(data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports: 1_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn token_account_data(mint: Pubkey, authority: Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(authority.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    data
}

fn token_amount(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

struct Fixture {
    program_test: ProgramTest,
    market: Pubkey,
    market_signer: Keypair,
    custody: Pubkey,
    ask_page: Pubkey,
    owner_page: Pubkey,
    bid_page_1: Pubkey,
    owner_page_1: Pubkey,
    taker: Keypair,
    taker_quote: Pubkey,
    taker_base: Pubkey,
    quote_vault: Pubkey,
    base_vault: Pubkey,
    vault_authority: Pubkey,
    maker_a_balance: Pubkey,
    maker_b_balance: Pubkey,
    maker_a_signer: Keypair,
    maker_b_signer: Keypair,
}

fn sell_fixture() -> Fixture {
    linked_sell_fixture(false)
}

fn linked_sell_fixture(linked: bool) -> Fixture {
    linked_sell_fixture_priced(linked, false)
}

fn linked_sell_fixture_priced(linked: bool, priced: bool) -> Fixture {
    let market_signer = Keypair::new();
    let market = market_signer.pubkey();
    let (custody, custody_bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (ask_page, _) =
        Pubkey::find_program_address(&[b"bid-page", market.as_ref(), &0u32.to_le_bytes()], &ID);
    let owner_page = Pubkey::new_unique();
    let (bid_page_1, _) =
        Pubkey::find_program_address(&[b"bid-page", market.as_ref(), &1u32.to_le_bytes()], &ID);
    let (owner_page_1, _) = Pubkey::find_program_address(
        &[b"bid-owner-page", market.as_ref(), &1u32.to_le_bytes()],
        &ID,
    );
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();
    let taker = Keypair::new();
    let taker_quote = Pubkey::new_unique();
    let taker_base = Pubkey::new_unique();
    let maker_a_signer = Keypair::new();
    let maker_b_signer = Keypair::new();
    let maker_a = maker_a_signer.pubkey();
    let maker_b = maker_b_signer.pubkey();
    let (maker_a_balance, maker_a_bump) =
        Pubkey::find_program_address(&[b"maker-balance", market.as_ref(), maker_a.as_ref()], &ID);
    let (maker_b_balance, maker_b_bump) =
        Pubkey::find_program_address(&[b"maker-balance", market.as_ref(), maker_b.as_ref()], &ID);

    let mut market_header =
        MarketHeader::new(1, if priced { Q64 / 2 } else { Q64 }, 1_000_000, 1, 1);
    market_header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    market_header.set_bid_count(2);
    market_header.next_sequence = 3;
    market_header.set_bid_owner_tag(owner_page.to_bytes()[..16].try_into().unwrap());
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    market_header.encode_into(&mut market_data).unwrap();

    let mut asks = BidPage::default();
    asks.set_links(PageLinks::new(0, None, linked.then_some(1)));
    asks.insert(BidEntry {
        price_x64: Q64,
        sqrt_price_x64: Q64,
        base_qty: 100,
        sequence: 1,
    })
    .unwrap();
    if !linked {
        asks.insert(BidEntry {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: 200,
            sequence: 2,
        })
        .unwrap();
    }
    let mut ask_data = vec![0u8; BID_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, linked.then_some(1)));
    owners.len = if linked { 1 } else { 2 };
    owners.owners[0] = maker_a.to_bytes();
    if !linked {
        owners.owners[1] = maker_b.to_bytes();
    }
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
        total_base: 0,
        total_quote: 1_000,
    };
    let mut custody_data = vec![0u8; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();

    let mut maker_a_state = MakerBalance::new(maker_a_bump, market.to_bytes(), maker_a.to_bytes());
    maker_a_state.locked_quote = 100;
    let mut maker_a_data = vec![0u8; MAKER_BALANCE_BYTES];
    maker_a_state.encode_into(&mut maker_a_data).unwrap();

    let mut maker_b_state = MakerBalance::new(maker_b_bump, market.to_bytes(), maker_b.to_bytes());
    maker_b_state.locked_quote = 200;
    let mut maker_b_data = vec![0u8; MAKER_BALANCE_BYTES];
    maker_b_state.encode_into(&mut maker_b_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.prefer_bpf(false);
    program_test.add_program(
        "spl_token",
        TOKEN_PROGRAM_ID,
        processor!(spl_token::processor::Processor::process),
    );
    program_test.prefer_bpf(true);
    program_test.add_account(market, account(market_data, ID));
    program_test.add_account(custody, account(custody_data, ID));
    program_test.add_account(ask_page, account(ask_data, ID));
    program_test.add_account(owner_page, account(owner_data, ID));
    if linked {
        let mut page_1 = BidPage::default();
        page_1.set_links(PageLinks::new(1, Some(0), None));
        page_1
            .insert(BidEntry {
                price_x64: if priced { Q64 / 4 } else { Q64 },
                sqrt_price_x64: if priced { Q64 / 2 } else { Q64 },
                base_qty: 200,
                sequence: 2,
            })
            .unwrap();
        let mut data = vec![0u8; BID_PAGE_BYTES];
        page_1.encode_into(&mut data).unwrap();
        program_test.add_account(bid_page_1, account(data, ID));
        let mut owner_1 = AskOwnerPage::default();
        owner_1.set_links(PageLinks::new(1, Some(0), None));
        owner_1.len = 1;
        owner_1.owners[0] = maker_b.to_bytes();
        let mut data = vec![0u8; ASK_OWNER_PAGE_BYTES];
        owner_1.encode_into(&mut data).unwrap();
        program_test.add_account(owner_page_1, account(data, ID));
    }
    program_test.add_account(maker_a_balance, account(maker_a_data, ID));
    program_test.add_account(maker_b_balance, account(maker_b_data, ID));
    program_test.add_account(
        base_vault,
        account(
            token_account_data(base_mint, vault_authority, 0),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        quote_vault,
        account(
            token_account_data(quote_mint, vault_authority, 1_000),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        taker_quote,
        account(
            token_account_data(quote_mint, taker.pubkey(), 0),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        taker_base,
        account(
            token_account_data(base_mint, taker.pubkey(), 1_000),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(taker.pubkey(), account(Vec::new(), Pubkey::default()));

    Fixture {
        program_test,
        market,
        market_signer,
        custody,
        ask_page,
        owner_page,
        bid_page_1,
        owner_page_1,
        taker,
        taker_quote,
        taker_base,
        quote_vault,
        base_vault,
        vault_authority,
        maker_a_balance,
        maker_b_balance,
        maker_a_signer,
        maker_b_signer,
    }
}

fn sell_instruction(f: &Fixture, opcode: u8, amount: u64, limit: u64) -> Instruction {
    let mut data = Vec::with_capacity(17);
    data.push(opcode);
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&limit.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new(f.ask_page, false),
            AccountMeta::new(f.owner_page, false),
            AccountMeta::new_readonly(f.taker.pubkey(), true),
            AccountMeta::new(f.taker_base, false),
            AccountMeta::new(f.taker_quote, false),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new(f.maker_a_balance, false),
            AccountMeta::new(f.maker_b_balance, false),
        ],
        data,
    }
}

#[tokio::test]
async fn exact_in_sell_transfers_tokens_and_compacts_bids() {
    let f = sell_fixture();
    let ix = sell_instruction(&f, 17, 250, 250);
    let mut context = f.program_test.start_with_context().await;
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let market = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap();
    let market = MarketHeader::decode_from(&market.data).unwrap();
    assert_eq!(market.bid_count(), 1);
    let page = context
        .banks_client
        .get_account(f.ask_page)
        .await
        .unwrap()
        .unwrap();
    let page = BidPage::decode_from(&page.data).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page.as_slice()[0].sequence, 2);
    assert_eq!(page.as_slice()[0].base_qty, 50);
    let maker_a = context
        .banks_client
        .get_account(f.maker_a_balance)
        .await
        .unwrap()
        .unwrap();
    let maker_a = MakerBalance::decode_from(&maker_a.data).unwrap();
    assert_eq!(maker_a.locked_quote, 0);
    assert_eq!(maker_a.free_base, 100);
    let maker_b = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    let maker_b = MakerBalance::decode_from(&maker_b.data).unwrap();
    assert_eq!(maker_b.locked_quote, 50);
    assert_eq!(maker_b.free_base, 150);
    let custody = context
        .banks_client
        .get_account(f.custody)
        .await
        .unwrap()
        .unwrap();
    let custody = CustodyState::decode_from(&custody.data).unwrap();
    assert_eq!(custody.total_base, 250);
    assert_eq!(custody.total_quote, 750);
    let taker_base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let taker_quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&taker_base.data), 750);
    assert_eq!(token_amount(&taker_quote.data), 250);
}

#[tokio::test]
async fn exact_out_sell_honors_input_limit() {
    let f = sell_fixture();
    let ix = sell_instruction(&f, 18, 150, 150);
    let mut context = f.program_test.start_with_context().await;
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let taker_base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let taker_quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&taker_base.data), 850);
    assert_eq!(token_amount(&taker_quote.data), 150);
}

#[tokio::test]
async fn sell_rejects_insufficient_depth_without_transfers() {
    let f = sell_fixture();
    let ix = sell_instruction(&f, 17, 350, 300);
    let mut context = f.program_test.start_with_context().await;
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());
    let taker_base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let taker_quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&taker_base.data), 1_000);
    assert_eq!(token_amount(&taker_quote.data), 0);
}

fn linked_sell_instruction(
    f: &Fixture,
    opcode: u8,
    amount: u64,
    limit: u64,
    legitimate_owner: bool,
) -> Instruction {
    let mut data = vec![opcode, 2];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&limit.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new_readonly(f.taker.pubkey(), true),
            AccountMeta::new(f.taker_base, false),
            AccountMeta::new(f.taker_quote, false),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new(f.ask_page, false),
            AccountMeta::new(f.owner_page, false),
            AccountMeta::new(f.bid_page_1, false),
            AccountMeta::new(
                if legitimate_owner {
                    f.owner_page_1
                } else {
                    f.owner_page
                },
                false,
            ),
            AccountMeta::new(f.maker_a_balance, false),
            AccountMeta::new(f.maker_b_balance, false),
        ],
        data,
    }
}

#[tokio::test]
async fn linked_sell_crosses_bid_page_boundary_with_real_spl_token() {
    let f = linked_sell_fixture(true);
    let ix = linked_sell_instruction(&f, 25, 250, 250, true);
    let mut context = f.program_test.start_with_context().await;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&base.data), 750);
    assert_eq!(token_amount(&quote.data), 250);
    let a = context
        .banks_client
        .get_account(f.maker_a_balance)
        .await
        .unwrap()
        .unwrap();
    let b = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(MakerBalance::decode_from(&a.data).unwrap().locked_quote, 0);
    assert_eq!(MakerBalance::decode_from(&b.data).unwrap().locked_quote, 50);
}

#[tokio::test]
async fn linked_sell_rejects_forged_bid_sidecar_without_tokens() {
    let f = linked_sell_fixture(true);
    let ix = linked_sell_instruction(&f, 25, 250, 250, false);
    let mut context = f.program_test.start_with_context().await;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());
    let base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&base.data), 1_000);
    assert_eq!(token_amount(&quote.data), 0);
}

#[tokio::test]
async fn linked_exact_out_sell_retires_page_and_preserves_owner_mapping() {
    let f = linked_sell_fixture_priced(true, true);
    let ix = linked_sell_instruction(&f, 26, 125, 200, true);
    let mut context = f.program_test.start_with_context().await;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let head = context
        .banks_client
        .get_account(f.ask_page)
        .await
        .unwrap()
        .unwrap();
    let head = BidPage::decode_from(&head.data).unwrap();
    assert_eq!(head.len(), 1);
    assert_eq!(head.links(), PageLinks::new(0, None, None));
    assert_eq!(head.as_slice()[0].sequence, 2);
    assert_eq!(head.as_slice()[0].base_qty, 100);

    let owners = context
        .banks_client
        .get_account(f.owner_page)
        .await
        .unwrap()
        .unwrap();
    let owners = AskOwnerPage::decode_from(&owners.data).unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners.links(), PageLinks::new(0, None, None));
    let maker_b = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        owners.as_slice()[0],
        MakerBalance::decode_from(&maker_b.data).unwrap().owner
    );

    let retired = context
        .banks_client
        .get_account(f.bid_page_1)
        .await
        .unwrap()
        .unwrap();
    let retired = BidPage::decode_from(&retired.data).unwrap();
    assert_eq!(retired.len(), 0);
    assert_eq!(retired.links(), PageLinks::new(1, None, None));

    let retired_owners = context
        .banks_client
        .get_account(f.owner_page_1)
        .await
        .unwrap()
        .unwrap();
    let retired_owners = AskOwnerPage::decode_from(&retired_owners.data).unwrap();
    assert_eq!(retired_owners.len(), 0);
    assert_eq!(retired_owners.links(), PageLinks::new(1, None, None));

    let base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&base.data), 800);
    assert_eq!(token_amount(&quote.data), 125);
}

#[tokio::test]
async fn linked_sell_then_legacy_sell_consumes_compacted_head_with_real_tokens() {
    let f = linked_sell_fixture(true);
    let first = linked_sell_instruction(&f, 25, 250, 250, true);
    let second = sell_instruction(&f, 17, 50, 50);
    let mut context = f.program_test.start_with_context().await;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[first],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let head = context
        .banks_client
        .get_account(f.ask_page)
        .await
        .unwrap()
        .unwrap();
    let head = BidPage::decode_from(&head.data).unwrap();
    assert_eq!(head.links(), PageLinks::new(0, None, None));
    assert_eq!(head.len(), 1);
    assert_eq!(head.as_slice()[0].base_qty, 50);

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[second],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let market = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        MarketHeader::decode_from(&market.data).unwrap().bid_count(),
        0
    );
    let head = context
        .banks_client
        .get_account(f.ask_page)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(BidPage::decode_from(&head.data).unwrap().len(), 0);
    let maker = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        MakerBalance::decode_from(&maker.data).unwrap().locked_quote,
        0
    );
    let base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&base.data), 700);
    assert_eq!(token_amount(&quote.data), 300);
}

fn lifecycle_close_instruction(f: &Fixture) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.bid_page_1, false),
            AccountMeta::new(f.owner_page_1, false),
        ],
        data: vec![27, 1, 1, 0, 0, 0],
    }
}

fn lifecycle_recreate_instruction(f: &Fixture, payer: Pubkey) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.ask_page, false),
            AccountMeta::new(f.owner_page, false),
            AccountMeta::new(f.bid_page_1, false),
            AccountMeta::new(f.owner_page_1, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ],
        data: vec![28, 1],
    }
}

#[tokio::test]
async fn detached_sell_page_closes_to_market_and_recreates_canonical_tail() {
    let f = linked_sell_fixture(true);
    let bad = lifecycle_close_instruction(&f);
    let swap = linked_sell_instruction(&f, 25, 250, 250, true);
    let close = lifecycle_close_instruction(&f);
    let mut recreate = lifecycle_recreate_instruction(&f, Pubkey::default());
    let mut context = f.program_test.start_with_context().await;

    // A live linked tail must never be eligible for rent recovery.
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[bad],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[swap],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let before = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    let tail_rent = context
        .banks_client
        .get_account(f.bid_page_1)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    let owner_rent = context
        .banks_client
        .get_account(f.owner_page_1)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[close],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let after = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    assert_eq!(after, before + tail_rent + owner_rent);
    let closed_tail = context
        .banks_client
        .get_account(f.bid_page_1)
        .await
        .unwrap();
    assert!(closed_tail.is_none() || closed_tail.unwrap().lamports == 0);

    recreate.accounts[5] = AccountMeta::new(context.payer.pubkey(), true);
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[recreate],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let head = context
        .banks_client
        .get_account(f.ask_page)
        .await
        .unwrap()
        .unwrap();
    let head = BidPage::decode_from(&head.data).unwrap();
    assert_eq!(head.links(), PageLinks::new(0, None, None));
    let tail = context
        .banks_client
        .get_account(f.bid_page_1)
        .await
        .unwrap()
        .unwrap();
    let tail = BidPage::decode_from(&tail.data).unwrap();
    assert_eq!(tail.len(), 0);
    assert_eq!(tail.links(), PageLinks::new(1, None, None));
    let owners = context
        .banks_client
        .get_account(f.owner_page_1)
        .await
        .unwrap()
        .unwrap();
    let owners = AskOwnerPage::decode_from(&owners.data).unwrap();
    assert_eq!(owners.len(), 0);
    assert_eq!(owners.links(), PageLinks::new(1, None, None));
}

fn linked_maker_ix(
    f: &Fixture,
    opcode: u8,
    page_index: u8,
    owner: Option<Pubkey>,
    data: &[u8],
) -> Instruction {
    let mut bytes = vec![opcode, 1];
    if opcode == 30 || opcode == 31 {
        bytes.push(page_index);
    }
    bytes.extend_from_slice(data);
    let mut accounts = vec![
        AccountMeta::new(f.market, false),
        AccountMeta::new(f.ask_page, false),
        AccountMeta::new(f.owner_page, false),
        AccountMeta::new(f.bid_page_1, false),
        AccountMeta::new(f.owner_page_1, false),
    ];
    if let Some(maker) = owner {
        let balance = if maker == f.maker_a_signer.pubkey() {
            f.maker_a_balance
        } else {
            f.maker_b_balance
        };
        accounts.push(AccountMeta::new(balance, false));
        accounts.push(AccountMeta::new_readonly(maker, true));
    }
    Instruction {
        program_id: ID,
        accounts,
        data: bytes,
    }
}

#[tokio::test]
async fn linked_sell_maker_cancel_detach_reattach_add_and_swap_with_real_tokens() {
    let f = linked_sell_fixture(true);
    let maker_a = f.maker_a_signer.pubkey();
    let maker_b = f.maker_b_signer.pubkey();

    let cancel_tail = linked_maker_ix(&f, 31, 1, Some(maker_b), &2u64.to_le_bytes());
    let forged_cancel = linked_maker_ix(&f, 31, 1, Some(maker_a), &2u64.to_le_bytes());
    let detach = linked_maker_ix(&f, 32, 0, None, &[]);
    let cancel_head = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.ask_page, false),
            AccountMeta::new(f.owner_page, false),
            AccountMeta::new(f.maker_a_balance, false),
            AccountMeta::new_readonly(maker_a, true),
        ],
        data: {
            let mut d = vec![9];
            d.extend_from_slice(&1u64.to_le_bytes());
            d
        },
    };
    let attach = linked_maker_ix(&f, 29, 0, None, &[]);
    let order_payload = {
        let mut p = Vec::new();
        p.extend_from_slice(&Q64.to_le_bytes());
        p.extend_from_slice(&Q64.to_le_bytes());
        p.extend_from_slice(&50u64.to_le_bytes());
        p
    };
    let add_head = linked_maker_ix(&f, 30, 0, Some(maker_a), &order_payload);
    let add_tail = linked_maker_ix(&f, 30, 1, Some(maker_b), &order_payload);
    let swap = linked_sell_instruction(&f, 25, 100, 100, true);

    let mut context = f.program_test.start_with_context().await;
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[forged_cancel],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker_a_signer],
        hash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[cancel_tail],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker_b_signer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let balance = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    let balance = MakerBalance::decode_from(&balance.data).unwrap();
    assert_eq!(balance.locked_quote, 0);

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[detach],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[cancel_head],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker_a_signer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[attach],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[add_head],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker_a_signer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[add_tail],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker_b_signer],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let market = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap();
    let market = MarketHeader::decode_from(&market.data).unwrap();
    assert_eq!(market.bid_count(), 2);
    let tail = context
        .banks_client
        .get_account(f.bid_page_1)
        .await
        .unwrap()
        .unwrap();
    let tail = BidPage::decode_from(&tail.data).unwrap();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail.links(), PageLinks::new(1, Some(0), None));

    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[swap],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.taker],
        hash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let market = context
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap();
    let market = MarketHeader::decode_from(&market.data).unwrap();
    assert_eq!(market.bid_count(), 0);
    let maker_a_balance = context
        .banks_client
        .get_account(f.maker_a_balance)
        .await
        .unwrap()
        .unwrap();
    let maker_b_balance = context
        .banks_client
        .get_account(f.maker_b_balance)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        MakerBalance::decode_from(&maker_a_balance.data)
            .unwrap()
            .locked_quote,
        0
    );
    assert_eq!(
        MakerBalance::decode_from(&maker_b_balance.data)
            .unwrap()
            .locked_quote,
        0
    );
    let taker_base = context
        .banks_client
        .get_account(f.taker_base)
        .await
        .unwrap()
        .unwrap();
    let taker_quote = context
        .banks_client
        .get_account(f.taker_quote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&taker_base.data), 900);
    assert_eq!(token_amount(&taker_quote.data), 100);
}
