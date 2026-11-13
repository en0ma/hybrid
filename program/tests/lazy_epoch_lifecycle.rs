#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::frozen_epoch::{FrozenEpochView, FROZEN_EPOCH_BYTES};
use hybrid_state::lazy_reserve::{LazyReserve, LAZY_RESERVE_BYTES};
use hybrid_state::{
    AskEntry, AskOwnerPage, AskPage, CustodyState, MakerBalance, MarketHeader, PageLinks,
    ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES, CUSTODY_STATE_BYTES, MAKER_BALANCE_BYTES,
    MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{hash::hashv, pubkey::Pubkey};
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_signer::Signer;
use solana_transaction::Transaction;

const TOKEN: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const GENESIS: [u8; 32] = [3; 32];

fn account(data: Vec<u8>, owner: Pubkey) -> Account {
    Account {
        lamports: 5_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}
fn mint_data() -> Vec<u8> {
    let mut data = vec![0; 82];
    data[44] = 6;
    data[45] = 1;
    data
}
fn token_data(mint: Pubkey, owner: Pubkey, balance: u64, frozen: bool) -> Vec<u8> {
    let mut data = vec![0; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&balance.to_le_bytes());
    data[108] = if frozen { 2 } else { 1 };
    data
}
fn amount(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

struct Fixture {
    test: Option<ProgramTest>,
    market_signer: Keypair,
    market: Pubkey,
    custody: Pubkey,
    reserve: Pubkey,
    epoch: Pubkey,
    ask: Pubkey,
    owners: Pubkey,
    maker_a: Keypair,
    maker_b: Keypair,
    balance_a: Pubkey,
    balance_b: Pubkey,
    destination_a: Pubkey,
    destination_b: Pubkey,
    taker: Keypair,
    taker_quote: Pubkey,
    taker_base: Pubkey,
    quote_vault: Pubkey,
    base_vault: Pubkey,
    quote_mint: Pubkey,
    base_mint: Pubkey,
    vault_authority: Pubkey,
}
fn fixture(freeze_destination: bool) -> Fixture {
    let market_signer = Keypair::new();
    let market = market_signer.pubkey();
    let maker_a = Keypair::new();
    let maker_b = Keypair::new();
    let taker = Keypair::new();
    let (custody, custody_bump) = Pubkey::find_program_address(&[b"custody", market.as_ref()], &ID);
    let (reserve, _) = Pubkey::find_program_address(&[b"lazy-reserve", market.as_ref()], &ID);
    let (epoch, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", market.as_ref(), &0u64.to_le_bytes()],
        &ID,
    );
    let (ask, _) =
        Pubkey::find_program_address(&[b"ask-page", market.as_ref(), &0u32.to_le_bytes()], &ID);
    let (balance_a, bump_a) = Pubkey::find_program_address(
        &[b"maker-balance", market.as_ref(), maker_a.pubkey().as_ref()],
        &ID,
    );
    let (balance_b, bump_b) = Pubkey::find_program_address(
        &[b"maker-balance", market.as_ref(), maker_b.pubkey().as_ref()],
        &ID,
    );
    let (vault_authority, _) =
        Pubkey::find_program_address(&[b"vault-authority", market.as_ref()], &ID);
    let owners = Pubkey::new_unique();
    let base_mint = Pubkey::new_unique();
    let quote_mint = Pubkey::new_unique();
    let base_vault = Pubkey::new_unique();
    let quote_vault = Pubkey::new_unique();
    let taker_base = Pubkey::new_unique();
    let taker_quote = Pubkey::new_unique();
    let destination_a = Pubkey::new_unique();
    let destination_b = Pubkey::new_unique();

    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.ask_count = 2;
    header.next_sequence = 3;
    header.reserved2 = owners.to_bytes();
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
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
    asks.insert(AskEntry {
        price_x64: Q64,
        sqrt_price_x64: Q64,
        base_qty: 200,
        sequence: 2,
    })
    .unwrap();
    let mut ask_data = vec![0; ASK_PAGE_BYTES];
    asks.encode_into(&mut ask_data).unwrap();

    let mut sidecar = AskOwnerPage::default();
    sidecar.set_links(PageLinks::new(0, None, None));
    sidecar.len = 2;
    sidecar.owners[0] = maker_a.pubkey().to_bytes();
    sidecar.owners[1] = maker_b.pubkey().to_bytes();
    let mut owner_data = vec![0; ASK_OWNER_PAGE_BYTES];
    sidecar.encode_into(&mut owner_data).unwrap();

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
        total_base: 300,
        total_quote: 0,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();

    let mut balance_a_state =
        MakerBalance::new(bump_a, market.to_bytes(), maker_a.pubkey().to_bytes());
    balance_a_state.locked_base = 100;
    let mut balance_a_data = vec![0; MAKER_BALANCE_BYTES];
    balance_a_state.encode_into(&mut balance_a_data).unwrap();
    let mut balance_b_state =
        MakerBalance::new(bump_b, market.to_bytes(), maker_b.pubkey().to_bytes());
    balance_b_state.locked_base = 200;
    let mut balance_b_data = vec![0; MAKER_BALANCE_BYTES];
    balance_b_state.encode_into(&mut balance_b_data).unwrap();

    let mut test = ProgramTest::new("hybrid_program", ID, None);
    test.prefer_bpf(false);
    test.add_program(
        "spl_token",
        TOKEN,
        processor!(spl_token::processor::Processor::process),
    );
    test.prefer_bpf(true);
    test.add_account(market, account(market_data, ID));
    test.add_account(custody, account(custody_data, ID));
    test.add_account(ask, account(ask_data, ID));
    test.add_account(owners, account(owner_data, ID));
    test.add_account(balance_a, account(balance_a_data, ID));
    test.add_account(balance_b, account(balance_b_data, ID));
    for key in [
        market_signer.pubkey(),
        maker_a.pubkey(),
        maker_b.pubkey(),
        taker.pubkey(),
    ] {
        // market account is already set above.
        if key != market {
            test.add_account(key, account(Vec::new(), Pubkey::default()));
        }
    }
    test.add_account(base_mint, account(mint_data(), TOKEN));
    test.add_account(quote_mint, account(mint_data(), TOKEN));
    test.add_account(
        base_vault,
        account(token_data(base_mint, vault_authority, 300, false), TOKEN),
    );
    test.add_account(
        quote_vault,
        account(token_data(quote_mint, vault_authority, 0, false), TOKEN),
    );
    test.add_account(
        taker_base,
        account(token_data(base_mint, taker.pubkey(), 0, false), TOKEN),
    );
    test.add_account(
        taker_quote,
        account(token_data(quote_mint, taker.pubkey(), 500, false), TOKEN),
    );
    test.add_account(
        destination_a,
        account(
            token_data(quote_mint, maker_a.pubkey(), 0, freeze_destination),
            TOKEN,
        ),
    );
    test.add_account(
        destination_b,
        account(token_data(quote_mint, maker_b.pubkey(), 0, false), TOKEN),
    );

    Fixture {
        test: Some(test),
        market_signer,
        market,
        custody,
        reserve,
        epoch,
        ask,
        owners,
        maker_a,
        maker_b,
        balance_a,
        balance_b,
        destination_a,
        destination_b,
        taker,
        taker_quote,
        taker_base,
        quote_vault,
        base_vault,
        quote_mint,
        base_mint,
        vault_authority,
    }
}

fn initialize(f: &Fixture, payer: Pubkey) -> Instruction {
    let mut data = vec![39];
    data.extend_from_slice(&GENESIS);
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(f.market, true),
            AccountMeta::new_readonly(f.custody, false),
            AccountMeta::new(f.reserve, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ],
        data,
    }
}
fn seal(f: &Fixture, payer: Pubkey, base: u64, limit: u64) -> Instruction {
    let mut data = vec![41];
    data.extend_from_slice(&base.to_le_bytes());
    data.extend_from_slice(&limit.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new(f.reserve, false),
            AccountMeta::new(f.epoch, false),
            AccountMeta::new(f.ask, false),
            AccountMeta::new(f.owners, false),
            AccountMeta::new_readonly(f.taker.pubkey(), true),
            AccountMeta::new(f.taker_quote, false),
            AccountMeta::new(f.taker_base, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new_readonly(f.quote_mint, false),
            AccountMeta::new_readonly(f.base_mint, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(Pubkey::default(), false),
        ],
        data,
    }
}

fn leaf(digest: &[u8; 32], seat: u32, maker: Pubkey, weight: u64, amounts: [u64; 6]) -> [u8; 32] {
    let index = seat.to_le_bytes();
    let weight_bytes = weight.to_le_bytes();
    let atoms = amounts.map(u64::to_le_bytes);
    hashv(&[
        b"HYBRID_MAKER_PUBKEY_CLAIM_V1\0",
        digest,
        &index,
        maker.as_ref(),
        &weight_bytes,
        &atoms[0],
        &atoms[1],
        &atoms[2],
        &atoms[3],
        &atoms[4],
        &atoms[5],
    ])
    .to_bytes()
}
fn claim(f: &Fixture, seat: u32, sibling: [u8; 32], destination: Pubkey) -> Instruction {
    let (maker, balance, weight, amounts) = if seat == 0 {
        (
            f.maker_a.pubkey(),
            f.balance_a,
            100u64,
            [50u64, 0, 0, 50, 0, 0],
        )
    } else {
        (
            f.maker_b.pubkey(),
            f.balance_b,
            200u64,
            [100u64, 0, 0, 100, 0, 1],
        )
    };
    let mut data = vec![40];
    data.extend_from_slice(&GENESIS);
    data.extend_from_slice(&seat.to_le_bytes());
    data.extend_from_slice(&weight.to_le_bytes());
    for amount in amounts {
        data.extend_from_slice(&amount.to_le_bytes());
    }
    data.push(1);
    data.extend_from_slice(&sibling);
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(f.market, false),
            AccountMeta::new(f.epoch, false),
            AccountMeta::new(balance, false),
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new_readonly(f.custody, false),
            AccountMeta::new(f.reserve, false),
            AccountMeta::new(f.quote_vault, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(f.quote_mint, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data,
    }
}

async fn tx(
    context: &mut ProgramTestContext,
    ix: Instruction,
    signers: &[&Keypair],
) -> Result<(), solana_program_test::BanksClientError> {
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let mut all_signers: Vec<&Keypair> = vec![&context.payer];
    all_signers.extend_from_slice(signers);
    // A Merkle-authenticated claim includes several PDA checks, SHA-256
    // syscalls, state updates and a checked SPL CPI. Declare a bounded
    // transaction compute budget explicitly rather than relying on the
    // 200_000-CU single-instruction default. Other opcodes retain the default.
    const CLAIM_CU_LIMIT: u32 = 350_000;
    let mut instructions = Vec::with_capacity(2);
    if ix.data.first() == Some(&40) {
        let mut data = vec![2u8]; // SetComputeUnitLimit, u32 little-endian
        data.extend_from_slice(&CLAIM_CU_LIMIT.to_le_bytes());
        instructions.push(Instruction {
            program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
            accounts: vec![],
            data,
        });
    }
    instructions.push(ix);
    let signed = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &all_signers,
        blockhash,
    );
    context.banks_client.process_transaction(signed).await
}
async fn bytes(context: &mut ProgramTestContext, pubkey: Pubkey) -> Vec<u8> {
    context
        .banks_client
        .get_account(pubkey)
        .await
        .unwrap()
        .unwrap()
        .data
}
async fn snapshot(context: &mut ProgramTestContext, f: &Fixture) -> Vec<Vec<u8>> {
    let mut accounts = Vec::new();
    for key in [
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        f.ask,
        f.owners,
        f.balance_a,
        f.balance_b,
        f.taker_quote,
        f.taker_base,
        f.quote_vault,
        f.base_vault,
        f.destination_a,
        f.destination_b,
    ] {
        accounts.push(bytes(context, key).await);
    }
    accounts
}

#[tokio::test]
async fn paid_execution_seals_on_chain_and_claims_conserve_fees_and_replay() {
    let mut f = fixture(false);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 0);
    assert_eq!(
        bytes(&mut context, f.reserve).await.len(),
        LAZY_RESERVE_BYTES
    );
    tx(&mut context, seal(&f, payer, 150, 151), &[&f.taker])
        .await
        .unwrap();
    let epoch_bytes = bytes(&mut context, f.epoch).await;
    assert_eq!(epoch_bytes.len(), FROZEN_EPOCH_BYTES);
    let epoch = FrozenEpochView::decode(&epoch_bytes).unwrap();
    assert_eq!(epoch.header.epoch_id, 0);
    assert_eq!(epoch.header.totals, [150, 0, 0, 150, 0, 1]);
    assert_eq!(epoch.remaining, epoch.header.totals);
    let a = leaf(
        &epoch.header.commitment,
        0,
        f.maker_a.pubkey(),
        100,
        [50, 0, 0, 50, 0, 0],
    );
    let b = leaf(
        &epoch.header.commitment,
        1,
        f.maker_b.pubkey(),
        200,
        [100, 0, 0, 100, 0, 1],
    );
    assert_eq!(
        hashv(&[b"HYBRID_MAKER_PUBKEY_NODE_V1\0", &a, &b]).to_bytes(),
        epoch.header.claim_root
    );
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(
        (
            reserve.next_epoch_id,
            reserve.maker_base_debit,
            reserve.maker_quote_credit,
            reserve.quote_fees
        ),
        (1, 150, 150, 1)
    );
    assert_eq!(amount(&bytes(&mut context, f.taker_quote).await), 349);
    assert_eq!(amount(&bytes(&mut context, f.taker_base).await), 150);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 151);
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 150);
    let bal_a = MakerBalance::decode_from(&bytes(&mut context, f.balance_a).await).unwrap();
    assert_eq!(bal_a.locked_base, 100);

    let claim_a = claim(&f, 0, b, f.destination_a);
    tx(&mut context, claim_a.clone(), &[&f.maker_a])
        .await
        .unwrap();
    assert_eq!(amount(&bytes(&mut context, f.destination_a).await), 50);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 101);
    let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    assert_eq!(epoch.remaining, [100, 0, 0, 100, 0, 1]);
    assert_eq!(epoch.claimed_count, 1);
    let before = snapshot(&mut context, &f).await;
    assert!(tx(&mut context, claim_a, &[&f.maker_a]).await.is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
    tx(
        &mut context,
        claim(&f, 1, a, f.destination_b),
        &[&f.maker_b],
    )
    .await
    .unwrap();
    assert_eq!(amount(&bytes(&mut context, f.destination_b).await), 100);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 1);
    let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    assert_eq!(epoch.remaining, [0; 6]);
    assert_eq!(epoch.claimed_count, 2);
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(
        (
            reserve.maker_base_debit,
            reserve.maker_quote_credit,
            reserve.quote_fees
        ),
        (0, 0, 1)
    );
}

#[tokio::test]
async fn failed_token_transfer_reverts_bitmap_balances_reserves_and_token_accounts() {
    let mut f = fixture(true);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    tx(&mut context, seal(&f, payer, 150, 151), &[&f.taker])
        .await
        .unwrap();
    let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    let sibling = leaf(
        &epoch.header.commitment,
        1,
        f.maker_b.pubkey(),
        200,
        [100, 0, 0, 100, 0, 1],
    );
    let before = snapshot(&mut context, &f).await;
    assert!(tx(
        &mut context,
        claim(&f, 0, sibling, f.destination_a),
        &[&f.maker_a]
    )
    .await
    .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
}
