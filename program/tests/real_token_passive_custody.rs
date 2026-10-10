#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::{
    passive::{
        PassivePool, PassivePosition, PoolAccount, PositionAccount, PASSIVE_POOL_BYTES,
        PASSIVE_POSITION_BYTES,
    },
    CustodyState, MarketHeader, CUSTODY_STATE_BYTES, MARKET_FLAG_COLLATERALIZED_ACTIVE,
    MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::pubkey::Pubkey;
use solana_program_test::{processor, ProgramTest};
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: 2_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn token(mint: Pubkey, authority: Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(authority.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    data
}

fn token_amount(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

fn initialized_mint() -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[44] = 6;
    data[45] = 1;
    data
}

struct Fixture {
    test: Option<ProgramTest>,
    owner: Keypair,
    market: Pubkey,
    custody: Pubkey,
    pool: Pubkey,
    position: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    base_dest: Pubkey,
    quote_dest: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    authority: Pubkey,
    nonce: u64,
}

fn fixture(available_base: u64, available_quote: u64) -> Fixture {
    let market = Pubkey::new_unique();
    let owner = Keypair::new();
    let (custody, bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (pool, pool_bump) = Pubkey::find_program_address(&[b"passive-pool", market.as_ref()], &ID);
    let nonce = 9u64;
    let (position, position_bump) = Pubkey::find_program_address(
        &[
            b"passive-position",
            market.as_ref(),
            owner.pubkey().as_ref(),
            &nonce.to_le_bytes(),
        ],
        &ID,
    );
    let (authority, _) = Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();
    let base_dest = Pubkey::new_unique();
    let quote_dest = Pubkey::new_unique();

    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    let mut market_bytes = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_bytes).unwrap();
    let custody_state = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: market.to_bytes(),
        base_mint: base_mint.to_bytes(),
        quote_mint: quote_mint.to_bytes(),
        base_vault: base_vault.to_bytes(),
        quote_vault: quote_vault.to_bytes(),
        total_base: 20,
        total_quote: 30,
    };
    let mut custody_bytes = vec![0; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_bytes).unwrap();
    let p = PassivePosition {
        owner: owner.pubkey().to_bytes(),
        lower_sqrt_price_x64: Q64 / 2,
        upper_sqrt_price_x64: Q64 * 2,
        liquidity: 40,
        base_principal: 100,
        quote_principal: 150,
    };
    let pool_state = PoolAccount {
        bump: pool_bump,
        market: market.to_bytes(),
        pool: PassivePool {
            base_reserve: 100,
            quote_reserve: 150,
            total_liquidity: 40,
            accrued_base_fees: 0,
            accrued_quote_fees: 0,
        },
    };
    let position_state = PositionAccount {
        bump: position_bump,
        market: market.to_bytes(),
        nonce,
        position: p,
    };
    let mut pool_bytes = vec![0; PASSIVE_POOL_BYTES];
    pool_state.encode_into(&mut pool_bytes).unwrap();
    let mut position_bytes = vec![0; PASSIVE_POSITION_BYTES];
    position_state.encode_into(&mut position_bytes).unwrap();

    let mut test = ProgramTest::new("hybrid_program", ID, None);
    test.prefer_bpf(false);
    test.add_program(
        "spl_token",
        TOKEN_ID,
        processor!(spl_token::processor::Processor::process),
    );
    test.prefer_bpf(true);
    test.add_account(market, account(ID, market_bytes));
    test.add_account(custody, account(ID, custody_bytes));
    test.add_account(pool, account(ID, pool_bytes));
    test.add_account(position, account(ID, position_bytes));
    test.add_account(owner.pubkey(), account(Pubkey::default(), vec![]));
    test.add_account(base_mint, account(TOKEN_ID, initialized_mint()));
    test.add_account(quote_mint, account(TOKEN_ID, initialized_mint()));
    test.add_account(
        base_vault,
        account(TOKEN_ID, token(base_mint, authority, available_base)),
    );
    test.add_account(
        quote_vault,
        account(TOKEN_ID, token(quote_mint, authority, available_quote)),
    );
    test.add_account(
        base_dest,
        account(TOKEN_ID, token(base_mint, owner.pubkey(), 0)),
    );
    test.add_account(
        quote_dest,
        account(TOKEN_ID, token(quote_mint, owner.pubkey(), 0)),
    );

    Fixture {
        test: Some(test),
        owner,
        market,
        custody,
        pool,
        position,
        base_vault,
        quote_vault,
        base_dest,
        quote_dest,
        base_mint,
        quote_mint,
        authority,
        nonce,
    }
}

fn close_ix(f: &Fixture, owner: Pubkey, base_destination: Pubkey) -> Instruction {
    let mut data = vec![21];
    data.extend_from_slice(&f.nonce.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(f.market, false),
            AccountMeta::new_readonly(f.custody, false),
            AccountMeta::new(f.pool, false),
            AccountMeta::new(f.position, false),
            AccountMeta::new(owner, true),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new(base_destination, false),
            AccountMeta::new(f.quote_dest, false),
            AccountMeta::new_readonly(f.base_mint, false),
            AccountMeta::new_readonly(f.quote_mint, false),
            AccountMeta::new_readonly(f.authority, false),
            AccountMeta::new_readonly(TOKEN_ID, false),
        ],
        data,
    }
}

async fn snapshot(ctx: &mut solana_program_test::ProgramTestContext, f: &Fixture) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    for key in [
        f.market,
        f.custody,
        f.pool,
        f.position,
        f.base_vault,
        f.quote_vault,
        f.base_dest,
        f.quote_dest,
    ] {
        let account = ctx.banks_client.get_account(key).await.unwrap().unwrap();
        result.push(account.data);
    }
    result
}

#[tokio::test]
async fn close_redeems_principal_without_spending_active_collateral() {
    let mut f = fixture(120, 180);
    let ix = close_ix(&f, f.owner.pubkey(), f.base_dest);
    let mut ctx = f.test.take().unwrap().start_with_context().await;
    let hash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, &f.owner],
        hash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let decoded = PoolAccount::decode_from(&pool.data).unwrap();
    assert_eq!(decoded.pool, PassivePool::default());
    let base = ctx
        .banks_client
        .get_account(f.base_vault)
        .await
        .unwrap()
        .unwrap();
    let quote = ctx
        .banks_client
        .get_account(f.quote_vault)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_amount(&base.data), 20);
    assert_eq!(token_amount(&quote.data), 30);
    assert_eq!(
        token_amount(
            &ctx.banks_client
                .get_account(f.base_dest)
                .await
                .unwrap()
                .unwrap()
                .data
        ),
        100
    );
    assert_eq!(
        token_amount(
            &ctx.banks_client
                .get_account(f.quote_dest)
                .await
                .unwrap()
                .unwrap()
                .data
        ),
        150
    );
}

#[tokio::test]
async fn short_vault_rejects_close_and_preserves_position() {
    let mut f = fixture(119, 180);
    let ix = close_ix(&f, f.owner.pubkey(), f.base_dest);
    let mut ctx = f.test.take().unwrap().start_with_context().await;
    let before = snapshot(&mut ctx, &f).await;
    let hash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, &f.owner],
        hash,
    );
    assert!(ctx.banks_client.process_transaction(tx).await.is_err());
    assert_eq!(snapshot(&mut ctx, &f).await, before);

    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    assert_eq!(
        PoolAccount::decode_from(&pool.data)
            .unwrap()
            .pool
            .base_reserve,
        100
    );
    let position = ctx
        .banks_client
        .get_account(f.position)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        PositionAccount::decode_from(&position.data)
            .unwrap()
            .position
            .base_principal,
        100
    );
}

#[tokio::test]
async fn aliased_destination_rejects_close_without_mutation() {
    let mut f = fixture(120, 180);
    let ix = close_ix(&f, f.owner.pubkey(), f.base_vault);
    let mut ctx = f.test.take().unwrap().start_with_context().await;
    let before = snapshot(&mut ctx, &f).await;
    let hash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, &f.owner],
        hash,
    );
    assert!(ctx.banks_client.process_transaction(tx).await.is_err());
    assert_eq!(snapshot(&mut ctx, &f).await, before);

    let position = ctx
        .banks_client
        .get_account(f.position)
        .await
        .unwrap()
        .unwrap();
    assert!(PositionAccount::decode_from(&position.data).is_ok());
}
