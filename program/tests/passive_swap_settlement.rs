#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::passive::{
    PassivePool, PassivePosition, PoolAccount, PositionAccount, PASSIVE_POOL_BYTES,
    PASSIVE_POSITION_BYTES,
};
use hybrid_state::{
    CustodyState, MarketHeader, CUSTODY_MAGIC, CUSTODY_STATE_BYTES,
    MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES, STATE_VERSION,
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

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Account {
    let mut data = vec![0u8; 165];
    data[..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1;
    account(data, TOKEN_PROGRAM_ID)
}

fn token_mint() -> Account {
    let mut data = vec![0u8; 82];
    data[36..44].copy_from_slice(&1_000_000_000u64.to_le_bytes());
    data[44] = 6;
    data[45] = 1;
    account(data, TOKEN_PROGRAM_ID)
}

fn amount(account: &Account) -> u64 {
    u64::from_le_bytes(account.data[64..72].try_into().unwrap())
}

struct Fixture {
    program: ProgramTest,
    market: Pubkey,
    custody: Pubkey,
    pool: Pubkey,
    position: Pubkey,
    lp: Keypair,
    taker: Keypair,
    taker_base: Pubkey,
    taker_quote: Pubkey,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    authority: Pubkey,
}

fn fixture() -> Fixture {
    let market = Pubkey::new_unique();
    let lp = Keypair::new();
    let taker = Keypair::new();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();
    let taker_base = Pubkey::new_unique();
    let taker_quote = Pubkey::new_unique();
    let (custody, custody_bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (pool, pool_bump) = Pubkey::find_program_address(&[b"passive-pool", market.as_ref()], &ID);
    let nonce = 42u64;
    let (position, position_bump) = Pubkey::find_program_address(
        &[
            b"passive-position",
            market.as_ref(),
            lp.pubkey().as_ref(),
            &nonce.to_le_bytes(),
        ],
        &ID,
    );
    let (authority, _) = Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let mut header = MarketHeader::new(1, Q64, 0, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    let mut header_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut header_data).unwrap();
    let custody_state = CustodyState {
        magic: CUSTODY_MAGIC,
        version: STATE_VERSION,
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
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();
    let pool_state = PoolAccount {
        bump: pool_bump,
        market: market.to_bytes(),
        pool: PassivePool {
            base_reserve: 10_000,
            quote_reserve: 10_000,
            total_liquidity: 1_000,
            ..PassivePool::default()
        },
    };
    let mut pool_data = vec![0; PASSIVE_POOL_BYTES];
    pool_state.encode_into(&mut pool_data).unwrap();
    let position_state = PositionAccount {
        bump: position_bump,
        market: market.to_bytes(),
        nonce,
        position: PassivePosition {
            owner: lp.pubkey().to_bytes(),
            lower_sqrt_price_x64: Q64 / 2,
            upper_sqrt_price_x64: Q64 * 2,
            liquidity: 1_000,
            base_principal: 10_000,
            quote_principal: 10_000,
        },
    };
    let mut position_data = vec![0; PASSIVE_POSITION_BYTES];
    position_state.encode_into(&mut position_data).unwrap();
    let mut program = ProgramTest::new("hybrid_program", ID, None);
    program.prefer_bpf(false);
    program.add_program(
        "spl_token",
        TOKEN_PROGRAM_ID,
        processor!(spl_token::processor::Processor::process),
    );
    program.prefer_bpf(true);
    program.add_account(market, account(header_data, ID));
    program.add_account(custody, account(custody_data, ID));
    program.add_account(pool, account(pool_data, ID));
    program.add_account(position, account(position_data, ID));
    program.add_account(base_mint, token_mint());
    program.add_account(quote_mint, token_mint());
    program.add_account(base_vault, token_account(base_mint, authority, 10_000));
    program.add_account(quote_vault, token_account(quote_mint, authority, 10_000));
    program.add_account(taker_base, token_account(base_mint, taker.pubkey(), 1_000));
    program.add_account(
        taker_quote,
        token_account(quote_mint, taker.pubkey(), 1_000),
    );
    program.add_account(lp.pubkey(), account(Vec::new(), Pubkey::default()));
    program.add_account(taker.pubkey(), account(Vec::new(), Pubkey::default()));
    Fixture {
        program,
        market,
        custody,
        pool,
        position,
        lp,
        taker,
        taker_base,
        taker_quote,
        base_vault,
        quote_vault,
        base_mint,
        quote_mint,
        authority,
    }
}

fn swap(f: &Fixture, side: u8, gross_in: u64, min_out: u64, vault: Pubkey) -> Instruction {
    let mut data = vec![33, side];
    data.extend_from_slice(&gross_in.to_le_bytes());
    data.extend_from_slice(&min_out.to_le_bytes());
    data.extend_from_slice(&100u16.to_le_bytes());
    data.extend_from_slice(&42u64.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new(f.pool, false),
            AccountMeta::new(f.position, false),
            AccountMeta::new_readonly(f.lp.pubkey(), false),
            AccountMeta::new_readonly(f.taker.pubkey(), true),
            AccountMeta::new(f.taker_base, false),
            AccountMeta::new(f.taker_quote, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new_readonly(f.base_mint, false),
            AccountMeta::new_readonly(f.quote_mint, false),
            AccountMeta::new_readonly(f.authority, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data,
    }
}

async fn send(
    ctx: &mut solana_program_test::ProgramTestContext,
    ix: Instruction,
    signer: &Keypair,
) -> bool {
    let hash = ctx.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, signer],
        hash,
    );
    ctx.banks_client.process_transaction(tx).await.is_ok()
}

#[tokio::test]
async fn funded_buy_sell_then_fee_aware_lp_close_conserves_tokens() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    assert!(send(&mut ctx, swap(&f, 0, 100, 1, f.base_vault), &f.taker).await);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let pool = PoolAccount::decode_from(&pool.data).unwrap();
    assert_eq!(pool.pool.accrued_quote_fees, 1);
    let market = ctx
        .banks_client
        .get_account(f.market)
        .await
        .unwrap()
        .unwrap();
    assert!(
        MarketHeader::decode_from(&market.data)
            .unwrap()
            .sqrt_price_x64
            > Q64
    );

    assert!(send(&mut ctx, swap(&f, 1, 100, 1, f.base_vault), &f.taker).await);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let pool = PoolAccount::decode_from(&pool.data).unwrap();
    assert_eq!(pool.pool.accrued_base_fees, 1);

    let mut data = vec![21];
    data.extend_from_slice(&42u64.to_le_bytes());
    let close = Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new(f.pool, false),
            AccountMeta::new(f.position, false),
            AccountMeta::new(f.lp.pubkey(), true),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new(f.taker_base, false),
            AccountMeta::new(f.taker_quote, false),
            AccountMeta::new_readonly(f.base_mint, false),
            AccountMeta::new_readonly(f.quote_mint, false),
            AccountMeta::new_readonly(f.authority, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data,
    };
    // Fee-aware redemption pays the LP: use its own token accounts instead
    // of the taker's, as the existing close handler enforces destination owner.
    let lp_base = Pubkey::new_unique();
    let lp_quote = Pubkey::new_unique();
    ctx.set_account(
        &lp_base,
        &account(
            {
                let mut d = vec![0u8; 165];
                d[0..32].copy_from_slice(f.base_mint.as_ref());
                d[32..64].copy_from_slice(f.lp.pubkey().as_ref());
                d[108] = 1;
                d
            },
            TOKEN_PROGRAM_ID,
        )
        .into(),
    );
    ctx.set_account(
        &lp_quote,
        &account(
            {
                let mut d = vec![0u8; 165];
                d[0..32].copy_from_slice(f.quote_mint.as_ref());
                d[32..64].copy_from_slice(f.lp.pubkey().as_ref());
                d[108] = 1;
                d
            },
            TOKEN_PROGRAM_ID,
        )
        .into(),
    );
    let mut close = close;
    close.accounts[7] = AccountMeta::new(lp_base, false);
    close.accounts[8] = AccountMeta::new(lp_quote, false);
    assert!(send(&mut ctx, close, &f.lp).await);
    let base = ctx
        .banks_client
        .get_account(lp_base)
        .await
        .unwrap()
        .unwrap();
    let quote = ctx
        .banks_client
        .get_account(lp_quote)
        .await
        .unwrap()
        .unwrap();
    assert!(amount(&base) > 0);
    assert!(amount(&quote) > 0);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    assert_eq!(
        PoolAccount::decode_from(&pool.data).unwrap().pool,
        PassivePool::default()
    );
}

#[tokio::test]
async fn counterfeit_base_vault_is_rejected_atomically() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    let fake = Pubkey::new_unique();
    let result = send(&mut ctx, swap(&f, 0, 100, 1, fake), &f.taker).await;
    assert!(!result);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    assert_eq!(
        PoolAccount::decode_from(&pool.data)
            .unwrap()
            .pool
            .base_reserve,
        10_000
    );
}

#[tokio::test]
async fn excessive_slippage_rejects_without_mutating_lp_or_vaults() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    let before_base = ctx
        .banks_client
        .get_account(f.base_vault)
        .await
        .unwrap()
        .unwrap();
    let before_quote = ctx
        .banks_client
        .get_account(f.quote_vault)
        .await
        .unwrap()
        .unwrap();
    assert!(!send(&mut ctx, swap(&f, 0, 100, 1_000, f.base_vault), &f.taker).await);
    let after_base = ctx
        .banks_client
        .get_account(f.base_vault)
        .await
        .unwrap()
        .unwrap();
    let after_quote = ctx
        .banks_client
        .get_account(f.quote_vault)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(amount(&before_base), amount(&after_base));
    assert_eq!(amount(&before_quote), amount(&after_quote));
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let state = PoolAccount::decode_from(&pool.data).unwrap();
    assert_eq!(state.pool.base_reserve, 10_000);
    assert_eq!(state.pool.accrued_quote_fees, 0);
}

#[tokio::test]
async fn wrong_position_nonce_cannot_take_lp_reserves() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    let mut ix = swap(&f, 0, 100, 1, f.base_vault);
    ix.data[20..28].copy_from_slice(&43u64.to_le_bytes());
    assert!(!send(&mut ctx, ix, &f.taker).await);
    let position = ctx
        .banks_client
        .get_account(f.position)
        .await
        .unwrap()
        .unwrap();
    let state = PositionAccount::decode_from(&position.data).unwrap();
    assert_eq!(state.position.base_principal, 10_000);
    assert_eq!(state.position.quote_principal, 10_000);
}

#[tokio::test]
async fn exact_out_buy_uses_bounded_gross_input_and_accrues_lp_fee() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    let mut instruction = swap(&f, 0, 300, 10, f.base_vault);
    instruction.data[0] = 34;
    assert!(send(&mut ctx, instruction, &f.taker).await);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let pool = PoolAccount::decode_from(&pool.data).unwrap();
    assert!(pool.pool.base_reserve <= 9_990);
    assert!(pool.pool.accrued_quote_fees > 0);
    assert!(pool.pool.quote_reserve <= 10_300);
}

#[tokio::test]
async fn exact_out_swap_exceeding_max_gross_input_rejects_atomically() {
    let f = fixture();
    let mut ctx = f.program.start_with_context().await;
    let mut instruction = swap(&f, 0, 1, 100, f.base_vault);
    instruction.data[0] = 34;
    assert!(!send(&mut ctx, instruction, &f.taker).await);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let pool = PoolAccount::decode_from(&pool.data).unwrap();
    assert_eq!(pool.pool.base_reserve, 10_000);
    assert_eq!(pool.pool.quote_reserve, 10_000);
    assert_eq!(pool.pool.accrued_quote_fees, 0);
}

fn multi_instruction(
    f: &Fixture,
    opcode: u8,
    side: u8,
    input: u64,
    output: u64,
    positions: &[Pubkey],
) -> Instruction {
    let mut data = vec![opcode, side];
    data.extend_from_slice(&input.to_le_bytes());
    data.extend_from_slice(&output.to_le_bytes());
    data.extend_from_slice(&100u16.to_le_bytes());
    data.push(positions.len() as u8);
    let mut accounts = vec![
        AccountMeta::new(f.market, false),
        AccountMeta::new_readonly(f.custody, false),
        AccountMeta::new(f.pool, false),
        AccountMeta::new_readonly(f.taker.pubkey(), true),
        AccountMeta::new(f.taker_base, false),
        AccountMeta::new(f.taker_quote, false),
        AccountMeta::new(f.base_vault, false),
        AccountMeta::new(f.quote_vault, false),
        AccountMeta::new_readonly(f.base_mint, false),
        AccountMeta::new_readonly(f.quote_mint, false),
        AccountMeta::new_readonly(f.authority, false),
        AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
    ];
    accounts.extend(positions.iter().map(|key| AccountMeta::new(*key, false)));
    Instruction { program_id: ID, accounts, data }
}

fn multi_fixture() -> (Fixture, Pubkey) {
    let mut f = fixture();
    let second_lp = Keypair::new();
    let nonce = 43u64;
    let (second_position, bump) = Pubkey::find_program_address(
        &[b"passive-position", f.market.as_ref(), second_lp.pubkey().as_ref(), &nonce.to_le_bytes()],
        &ID,
    );
    let (pool_key, pool_bump) = Pubkey::find_program_address(
        &[b"passive-pool", f.market.as_ref()], &ID
    );
    assert_eq!(pool_key, f.pool);
    let pool = PoolAccount {
        bump: pool_bump,
        market: f.market.to_bytes(),
        pool: PassivePool {
            base_reserve: 20_000,
            quote_reserve: 20_000,
            total_liquidity: 2_000,
            ..PassivePool::default()
        },
    };
    let mut pool_data = vec![0u8; PASSIVE_POOL_BYTES];
    pool.encode_into(&mut pool_data).unwrap();
    f.program.add_account(f.pool, account(pool_data, ID));
    let state = PositionAccount {
        bump,
        market: f.market.to_bytes(),
        nonce,
        position: PassivePosition {
            owner: second_lp.pubkey().to_bytes(),
            lower_sqrt_price_x64: Q64 / 2,
            upper_sqrt_price_x64: Q64 * 2,
            liquidity: 1_000,
            base_principal: 10_000,
            quote_principal: 10_000,
        },
    };
    let mut data = vec![0u8; PASSIVE_POSITION_BYTES];
    state.encode_into(&mut data).unwrap();
    f.program.add_account(second_position, account(data, ID));
    f.program.add_account(f.base_vault, token_account(f.base_mint, f.authority, 20_000));
    f.program.add_account(f.quote_vault, token_account(f.quote_mint, f.authority, 20_000));
    (f, second_position)
}

#[tokio::test]
async fn funded_multi_lp_buy_sell_and_exact_out_preserve_all_claims() {
    let (f, second) = multi_fixture();
    let mut positions = vec![f.position, second];
    positions.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    let mut ctx = f.program.start_with_context().await;
    assert!(send(&mut ctx, multi_instruction(&f, 35, 0, 100, 1, &positions), &f.taker).await);
    assert!(send(&mut ctx, multi_instruction(&f, 35, 1, 100, 1, &positions), &f.taker).await);
    assert!(send(&mut ctx, multi_instruction(&f, 36, 0, 250, 10, &positions), &f.taker).await);
    let pool_account = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    let pool = PoolAccount::decode_from(&pool_account.data).unwrap();
    assert_eq!(pool.pool.accrued_base_fees, 0);
    assert_eq!(pool.pool.accrued_quote_fees, 0);
    let mut base = 0u64;
    let mut quote = 0u64;
    for key in positions {
        let a = ctx.banks_client.get_account(key).await.unwrap().unwrap();
        let p = PositionAccount::decode_from(&a.data).unwrap();
        base += p.position.base_principal;
        quote += p.position.quote_principal;
    }
    assert_eq!(base, pool.pool.base_reserve);
    assert_eq!(quote, pool.pool.quote_reserve);
    assert!(quote > 20_000);
}

#[tokio::test]
async fn omitted_multi_lp_claim_rejected_without_vault_changes() {
    let (f, _) = multi_fixture();
    let mut ctx = f.program.start_with_context().await;
    assert!(!send(&mut ctx, multi_instruction(&f, 35, 0, 100, 1, &[f.position]), &f.taker).await);
    let pool = ctx.banks_client.get_account(f.pool).await.unwrap().unwrap();
    assert_eq!(PoolAccount::decode_from(&pool.data).unwrap().pool.base_reserve, 20_000);
    let vault = ctx.banks_client.get_account(f.base_vault).await.unwrap().unwrap();
    assert_eq!(amount(&vault), 20_000);
}
