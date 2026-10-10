#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::{
    CustodyState, MakerBalance, MarketHeader, CUSTODY_STATE_BYTES, MAKER_BALANCE_BYTES,
    MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::{processor, ProgramTest};
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn account(data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports: 2_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn mint_data() -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[44] = 6;
    data[45] = 1;
    data
}

fn token_data(mint: Pubkey, authority: Pubkey, balance: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(authority.as_ref());
    data[64..72].copy_from_slice(&balance.to_le_bytes());
    data[108] = 1;
    data
}

fn amount(account: &Account) -> u64 {
    u64::from_le_bytes(account.data[64..72].try_into().unwrap())
}

struct Fixture {
    test: Option<ProgramTest>,
    market: Pubkey,
    custody: Pubkey,
    balance: Pubkey,
    maker: Keypair,
    vault_authority: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    base_source: Pubkey,
    quote_source: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
}

fn fixture() -> Fixture {
    let market = Pubkey::new_unique();
    let (custody, custody_bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let maker = Keypair::new();
    let (balance, balance_bump) = Pubkey::find_program_address(
        &[b"maker-balance", market.as_ref(), maker.pubkey().as_ref()],
        &ID,
    );
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_source = Pubkey::new_unique();
    let quote_source = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();

    let header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    let mut header_bytes = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut header_bytes).unwrap();

    let state = CustodyState {
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
        total_quote: 0,
    };
    let mut custody_bytes = vec![0; CUSTODY_STATE_BYTES];
    state.encode_into(&mut custody_bytes).unwrap();

    let maker_state = MakerBalance::new(balance_bump, market.to_bytes(), maker.pubkey().to_bytes());
    let mut balance_bytes = vec![0; MAKER_BALANCE_BYTES];
    maker_state.encode_into(&mut balance_bytes).unwrap();

    let mut test = ProgramTest::new("hybrid_program", ID, None);
    test.prefer_bpf(false);
    test.add_program(
        "spl_token",
        TOKEN,
        processor!(spl_token::processor::Processor::process),
    );
    test.prefer_bpf(true);
    test.add_account(market, account(header_bytes, ID));
    test.add_account(custody, account(custody_bytes, ID));
    test.add_account(balance, account(balance_bytes, ID));
    test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
    test.add_account(base_mint, account(mint_data(), TOKEN));
    test.add_account(quote_mint, account(mint_data(), TOKEN));
    test.add_account(
        base_source,
        account(token_data(base_mint, maker.pubkey(), 500), TOKEN),
    );
    test.add_account(
        quote_source,
        account(token_data(quote_mint, maker.pubkey(), 800), TOKEN),
    );
    test.add_account(
        base_vault,
        account(token_data(base_mint, vault_authority, 0), TOKEN),
    );
    test.add_account(
        quote_vault,
        account(token_data(quote_mint, vault_authority, 0), TOKEN),
    );

    Fixture {
        test: Some(test),
        market,
        custody,
        balance,
        maker,
        vault_authority,
        base_mint,
        quote_mint,
        base_source,
        quote_source,
        base_vault,
        quote_vault,
    }
}

fn instruction(f: &Fixture, opcode: u8, asset: u8, quantity: u64) -> Instruction {
    let (source, vault, mint) = if asset == 0 {
        (f.base_source, f.base_vault, f.base_mint)
    } else {
        (f.quote_source, f.quote_vault, f.quote_mint)
    };
    let mut data = vec![opcode, asset];
    data.extend_from_slice(&quantity.to_le_bytes());
    let mut accounts = vec![
        AccountMeta::new_readonly(f.market, false),
        AccountMeta::new(f.custody, false),
        AccountMeta::new(f.balance, false),
        AccountMeta::new_readonly(f.maker.pubkey(), true),
    ];
    if opcode == 13 {
        accounts.extend([
            AccountMeta::new(source, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(TOKEN, false),
        ]);
    } else {
        accounts.extend([
            AccountMeta::new(vault, false),
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN, false),
        ]);
    }
    Instruction {
        program_id: ID,
        accounts,
        data,
    }
}

async fn read(
    context: &mut solana_program_test::ProgramTestContext,
    f: &Fixture,
) -> (CustodyState, MakerBalance, [u64; 4]) {
    let client = &mut context.banks_client;
    let custody = client.get_account(f.custody).await.unwrap().unwrap();
    let balance = client.get_account(f.balance).await.unwrap().unwrap();
    let base_source = client.get_account(f.base_source).await.unwrap().unwrap();
    let quote_source = client.get_account(f.quote_source).await.unwrap().unwrap();
    let base_vault = client.get_account(f.base_vault).await.unwrap().unwrap();
    let quote_vault = client.get_account(f.quote_vault).await.unwrap().unwrap();
    (
        CustodyState::decode_from(&custody.data).unwrap(),
        MakerBalance::decode_from(&balance.data).unwrap(),
        [
            amount(&base_source),
            amount(&quote_source),
            amount(&base_vault),
            amount(&quote_vault),
        ],
    )
}

async fn execute(
    context: &mut solana_program_test::ProgramTestContext,
    f: &Fixture,
    opcode: u8,
    asset: u8,
    quantity: u64,
) -> Result<(), solana_program_test::BanksClientError> {
    let ix = instruction(f, opcode, asset, quantity);
    let hash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &f.maker],
        hash,
    );
    context.banks_client.process_transaction(tx).await
}

#[tokio::test]
async fn maker_deposits_and_withdraws_both_real_tokens() {
    let mut f = fixture();
    let mut context = f.test.take().unwrap().start_with_context().await;
    execute(&mut context, &f, 13, 0, 120).await.unwrap();
    execute(&mut context, &f, 13, 1, 230).await.unwrap();
    let (custody, maker, amounts) = read(&mut context, &f).await;
    assert_eq!((custody.total_base, custody.total_quote), (120, 230));
    assert_eq!((maker.free_base, maker.free_quote), (120, 230));
    assert_eq!(amounts, [380, 570, 120, 230]);

    execute(&mut context, &f, 14, 0, 45).await.unwrap();
    execute(&mut context, &f, 14, 1, 80).await.unwrap();
    let (custody, maker, amounts) = read(&mut context, &f).await;
    assert_eq!((custody.total_base, custody.total_quote), (75, 150));
    assert_eq!((maker.free_base, maker.free_quote), (75, 150));
    assert_eq!(amounts, [425, 650, 75, 150]);
    assert_eq!(amounts[0] + amounts[2], 500);
    assert_eq!(amounts[1] + amounts[3], 800);
}

#[tokio::test]
async fn insufficient_free_balance_rolls_back_without_token_movements() {
    let mut f = fixture();
    let mut context = f.test.take().unwrap().start_with_context().await;
    execute(&mut context, &f, 13, 0, 90).await.unwrap();
    let before = read(&mut context, &f).await;
    assert!(execute(&mut context, &f, 14, 0, 91).await.is_err());
    let after = read(&mut context, &f).await;
    assert_eq!(before, after);
}

#[tokio::test]
async fn insufficient_source_tokens_rolls_back_without_custody_credit() {
    let mut f = fixture();
    let mut context = f.test.take().unwrap().start_with_context().await;
    let before = read(&mut context, &f).await;
    assert!(execute(&mut context, &f, 13, 1, 801).await.is_err());
    let after = read(&mut context, &f).await;
    assert_eq!(before, after);
}
