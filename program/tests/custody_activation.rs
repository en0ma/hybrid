#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::{
    AskEntry, AskOwnerPage, AskPage, CustodyState, MakerBalance, MarketHeader, PageLinks,
    ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES, CUSTODY_STATE_BYTES, MAKER_BALANCE_BYTES,
    MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::ProgramTest;
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN_PROGRAM_ID: Pubkey =
    solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn account(lamports: u64, data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn mint_data(decimals: u8) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[44] = decimals;
    data[45] = 1;
    data
}

fn token_account_data(mint: Pubkey, authority: Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(authority.as_ref());
    data[108] = 1;
    data
}

fn custody_instruction(
    market: Pubkey,
    custody: Pubkey,
    payer: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    market_signer: bool,
) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market, market_signer),
            AccountMeta::new(custody, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(base_mint, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new_readonly(base_vault, false),
            AccountMeta::new_readonly(quote_vault, false),
            AccountMeta::new_readonly(Pubkey::default(), false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data: vec![11, 6, 6],
    }
}

fn cancel_ask_data(sequence: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(9);
    data.push(7);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

#[tokio::test]
async fn custody_init_requires_market_signature() {
    let market = Pubkey::new_unique();
    let (custody, _) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();

    let header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.add_account(market, account(1_000_000, market_data, ID));
    program_test.add_account(
        base_mint,
        account(1_000_000, mint_data(6), TOKEN_PROGRAM_ID),
    );
    program_test.add_account(
        quote_mint,
        account(1_000_000, mint_data(6), TOKEN_PROGRAM_ID),
    );
    program_test.add_account(
        base_vault,
        account(
            1_000_000,
            token_account_data(base_mint, vault_authority),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        quote_vault,
        account(
            1_000_000,
            token_account_data(quote_mint, vault_authority),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        TOKEN_PROGRAM_ID,
        account(1_000_000, Vec::new(), Pubkey::default()),
    );

    let mut context = program_test.start_with_context().await;
    let ix = custody_instruction(
        market,
        custody,
        context.payer.pubkey(),
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        false,
    );
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());
    assert!(context
        .banks_client
        .get_account(custody)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn legacy_orders_cancel_before_prefunded_custody_activation() {
    let market = Keypair::new();
    let market_key = market.pubkey();
    let maker = Keypair::new();
    let (ask_key, _) = Pubkey::find_program_address(
        &[b"ask-page", market_key.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let owner_key = Pubkey::new_unique();
    let (custody_key, _) = Pubkey::find_program_address(&[b"custody", market_key.as_ref()], &ID);
    let (balance_key, _) = Pubkey::find_program_address(
        &[
            b"maker-balance",
            market_key.as_ref(),
            maker.pubkey().as_ref(),
        ],
        &ID,
    );
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market_key.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();

    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.ask_count = 1;
    header.next_sequence = 2;
    header.reserved2 = owner_key.to_bytes();
    let mut market_data = vec![0u8; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    asks.insert(AskEntry {
        price_x64: Q64,
        sqrt_price_x64: Q64,
        base_qty: 100,
        sequence: 1,
    })
    .unwrap();
    let mut ask_data = vec![0u8; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));
    owners.len = 1;
    owners.owners[0] = maker.pubkey().to_bytes();
    let mut owner_data = vec![0u8; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();

    let mut program_test = ProgramTest::new("hybrid_program", ID, None);
    program_test.add_account(market_key, account(1_000_000, market_data, ID));
    program_test.add_account(ask_key, account(1_000_000, ask_data, ID));
    program_test.add_account(owner_key, account(1_000_000, owner_data, ID));
    program_test.add_account(
        maker.pubkey(),
        account(1_000_000, Vec::new(), Pubkey::default()),
    );
    program_test.add_account(custody_key, account(1, Vec::new(), Pubkey::default()));
    program_test.add_account(balance_key, account(1, Vec::new(), Pubkey::default()));
    program_test.add_account(
        base_mint,
        account(1_000_000, mint_data(6), TOKEN_PROGRAM_ID),
    );
    program_test.add_account(
        quote_mint,
        account(1_000_000, mint_data(6), TOKEN_PROGRAM_ID),
    );
    program_test.add_account(
        base_vault,
        account(
            1_000_000,
            token_account_data(base_mint, vault_authority),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        quote_vault,
        account(
            1_000_000,
            token_account_data(quote_mint, vault_authority),
            TOKEN_PROGRAM_ID,
        ),
    );
    program_test.add_account(
        TOKEN_PROGRAM_ID,
        account(1_000_000, Vec::new(), Pubkey::default()),
    );

    let mut context = program_test.start_with_context().await;

    let init_before_cancel = custody_instruction(
        market_key,
        custody_key,
        context.payer.pubkey(),
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        true,
    );
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init_before_cancel],
        Some(&context.payer.pubkey()),
        &[&context.payer, &market],
        blockhash,
    );
    assert!(context.banks_client.process_transaction(tx).await.is_err());

    let legacy_cancel = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(market_key, false),
            AccountMeta::new(ask_key, false),
            AccountMeta::new(owner_key, false),
            AccountMeta::new_readonly(maker.pubkey(), true),
        ],
        data: cancel_ask_data(1),
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[legacy_cancel],
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
    let header = MarketHeader::decode_from(&stored_market.data).unwrap();
    assert_eq!(header.ask_count, 0);
    assert!(!header.collateralized_active());

    let init = custody_instruction(
        market_key,
        custody_key,
        context.payer.pubkey(),
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        true,
    );
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init],
        Some(&context.payer.pubkey()),
        &[&context.payer, &market],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let custody_account = context
        .banks_client
        .get_account(custody_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(custody_account.owner, ID);
    assert_eq!(custody_account.data.len(), CUSTODY_STATE_BYTES);
    CustodyState::decode_from(&custody_account.data).unwrap();

    let stored_market = context
        .banks_client
        .get_account(market_key)
        .await
        .unwrap()
        .unwrap();
    let header = MarketHeader::decode_from(&stored_market.data).unwrap();
    assert!(header.collateralized_active());

    let init_balance = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(market_key, false),
            AccountMeta::new_readonly(custody_key, false),
            AccountMeta::new(balance_key, false),
            AccountMeta::new_readonly(maker.pubkey(), true),
            AccountMeta::new(context.payer.pubkey(), true),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ],
        data: vec![12],
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init_balance],
        Some(&context.payer.pubkey()),
        &[&context.payer, &maker],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let balance_account = context
        .banks_client
        .get_account(balance_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(balance_account.owner, ID);
    assert_eq!(balance_account.data.len(), MAKER_BALANCE_BYTES);
    let balance = MakerBalance::decode_from(&balance_account.data).unwrap();
    assert_eq!(balance.owner, maker.pubkey().to_bytes());
}
