#![cfg(feature = "test-sbf")]

use hybrid_engine::Q64;
use hybrid_program::ID;
use hybrid_state::frozen_epoch::{FrozenEpochView, FROZEN_EPOCH_BYTES};
use hybrid_state::lazy_reserve::{LazyReserve, LAZY_RESERVE_BYTES};
use hybrid_state::{
    AskEntry, AskOwnerPage, AskPage, BidPage, CustodyState, MakerBalance, MarketHeader, PageLinks,
    ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES, BID_PAGE_BYTES, CUSTODY_STATE_BYTES, MAKER_BALANCE_BYTES,
    MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES,
};
use solana_account::Account;
use solana_address_lookup_table_interface::{
    program as lookup_table_program,
    state::{AddressLookupTable, LookupTableMeta},
};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::{v0, AddressLookupTableAccount, VersionedMessage};
use solana_program::{hash::hashv, pubkey::Pubkey};
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_signer::Signer;
use solana_transaction::{versioned::VersionedTransaction, Transaction};
use std::borrow::Cow;

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
    extra_makers: Vec<Keypair>,
    extra_balances: Vec<Pubkey>,
    extra_destinations: Vec<Pubkey>,
    lookup_table: Option<Pubkey>,
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
    fixture_for_side(freeze_destination, false, None)
}

fn fixture_bid(freeze_destination: bool) -> Fixture {
    fixture_for_side(freeze_destination, true, None)
}

fn fixture_bid_fractional(qty_per_maker: u64) -> Fixture {
    fixture_for_side(false, true, Some(qty_per_maker))
}

fn fixture_for_side(
    freeze_destination: bool,
    bid_side: bool,
    fractional_bid_qty: Option<u64>,
) -> Fixture {
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
    let page_seed = if bid_side {
        b"bid-page".as_slice()
    } else {
        b"ask-page".as_slice()
    };
    let (ask, _) =
        Pubkey::find_program_address(&[page_seed, market.as_ref(), &0u32.to_le_bytes()], &ID);
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

    let quantity_a = fractional_bid_qty.unwrap_or(100);
    let quantity_b = fractional_bid_qty.unwrap_or(200);
    let price = if fractional_bid_qty.is_some() {
        Q64 / 2
    } else {
        Q64
    };
    // Canonical floor(sqrt(price_x64 * Q64)) for price 1/2.
    let sqrt_price = if fractional_bid_qty.is_some() {
        13_043_817_825_332_782_213u128
    } else {
        Q64
    };
    let quote_a = hybrid_engine::quote_for_base_at_price(quantity_a, price).unwrap();
    let quote_b = hybrid_engine::quote_for_base_at_price(quantity_b, price).unwrap();
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.ask_count = if bid_side { 0 } else { 2 };
    if bid_side {
        header.set_bid_count(2);
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&owners.to_bytes()[..16]);
        header.set_bid_owner_tag(tag);
    } else {
        header.reserved2 = owners.to_bytes();
    }
    header.next_sequence = 3;
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let mut asks = AskPage::default();
    asks.set_links(PageLinks::new(0, None, None));
    asks.insert(AskEntry {
        price_x64: price,
        sqrt_price_x64: sqrt_price,
        base_qty: quantity_a,
        sequence: 1,
    })
    .unwrap();
    asks.insert(AskEntry {
        price_x64: price,
        sqrt_price_x64: sqrt_price,
        base_qty: quantity_b,
        sequence: 2,
    })
    .unwrap();
    let mut ask_data = vec![
        0;
        if bid_side {
            BID_PAGE_BYTES
        } else {
            ASK_PAGE_BYTES
        }
    ];
    if bid_side {
        let mut bids = BidPage::default();
        bids.set_links(PageLinks::new(0, None, None));
        for entry in asks.as_slice() {
            bids.insert(*entry).unwrap();
        }
        bids.encode_into(&mut ask_data).unwrap();
    } else {
        asks.encode_into(&mut ask_data).unwrap();
    }

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
        total_base: if bid_side { 0 } else { quantity_a + quantity_b },
        total_quote: if bid_side { quote_a + quote_b } else { 0 },
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();

    let mut balance_a_state =
        MakerBalance::new(bump_a, market.to_bytes(), maker_a.pubkey().to_bytes());
    if bid_side {
        balance_a_state.locked_quote = quote_a;
    } else {
        balance_a_state.locked_base = quantity_a;
    }
    let mut balance_a_data = vec![0; MAKER_BALANCE_BYTES];
    balance_a_state.encode_into(&mut balance_a_data).unwrap();
    let mut balance_b_state =
        MakerBalance::new(bump_b, market.to_bytes(), maker_b.pubkey().to_bytes());
    if bid_side {
        balance_b_state.locked_quote = quote_b;
    } else {
        balance_b_state.locked_base = quantity_b;
    }
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
        account(
            token_data(
                base_mint,
                vault_authority,
                if bid_side { 0 } else { quantity_a + quantity_b },
                false,
            ),
            TOKEN,
        ),
    );
    test.add_account(
        quote_vault,
        account(
            token_data(
                quote_mint,
                vault_authority,
                if bid_side { quote_a + quote_b } else { 0 },
                false,
            ),
            TOKEN,
        ),
    );
    test.add_account(
        taker_base,
        account(
            token_data(
                base_mint,
                taker.pubkey(),
                if bid_side { 500 } else { 0 },
                false,
            ),
            TOKEN,
        ),
    );
    test.add_account(
        taker_quote,
        account(
            token_data(
                quote_mint,
                taker.pubkey(),
                if bid_side { 0 } else { 500 },
                false,
            ),
            TOKEN,
        ),
    );
    test.add_account(
        destination_a,
        account(
            token_data(
                if bid_side { base_mint } else { quote_mint },
                maker_a.pubkey(),
                0,
                freeze_destination,
            ),
            TOKEN,
        ),
    );
    test.add_account(
        destination_b,
        account(
            token_data(
                if bid_side { base_mint } else { quote_mint },
                maker_b.pubkey(),
                0,
                false,
            ),
            TOKEN,
        ),
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
        extra_makers: Vec::new(),
        extra_balances: Vec::new(),
        extra_destinations: Vec::new(),
        lookup_table: None,
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

/// Rebuild the canonical page, sidecar and custody around 9 or 16 distinct
/// maker PDAs. The first two makers retain their original 100/200 base bids.
fn fixture_bid_extended(count: usize) -> Fixture {
    assert!((9..=16).contains(&count));
    let mut f = fixture_bid(false);
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.set_bid_count(count as u32);
    header.next_sequence = count as u64 + 1;
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&f.owners.to_bytes()[..16]);
    header.set_bid_owner_tag(tag);
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let mut bids = BidPage::default();
    bids.set_links(PageLinks::new(0, None, None));
    let mut owners = AskOwnerPage::default();
    owners.set_links(PageLinks::new(0, None, None));
    let mut owner_keys = vec![f.maker_a.pubkey(), f.maker_b.pubkey()];
    let mut weights = vec![100u64, 200u64];
    let test = f.test.as_mut().unwrap();
    for _ in 2..count {
        let maker = Keypair::new();
        let (balance_key, bump) = Pubkey::find_program_address(
            &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
            &ID,
        );
        let destination = Pubkey::new_unique();
        let mut balance = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
        balance.locked_quote = 100;
        let mut data = vec![0; MAKER_BALANCE_BYTES];
        balance.encode_into(&mut data).unwrap();
        test.add_account(balance_key, account(data, ID));
        test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
        test.add_account(
            destination,
            account(token_data(f.base_mint, maker.pubkey(), 0, false), TOKEN),
        );
        owner_keys.push(maker.pubkey());
        weights.push(100);
        f.extra_makers.push(maker);
        f.extra_balances.push(balance_key);
        f.extra_destinations.push(destination);
    }
    for (i, (owner, qty)) in owner_keys.iter().zip(&weights).enumerate() {
        bids.insert(AskEntry {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: *qty,
            sequence: i as u64 + 1,
        })
        .unwrap();
        owners.owners[i] = owner.to_bytes();
    }
    owners.len = count as u16;
    let mut bid_data = vec![0; BID_PAGE_BYTES];
    bids.encode_into(&mut bid_data).unwrap();
    let mut owner_data = vec![0; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();

    let collateral: u64 = weights.iter().sum();
    let (custody, bump) = Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    assert_eq!(custody, f.custody);
    let custody_state = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 0,
        total_quote: collateral,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody_state.encode_into(&mut custody_data).unwrap();
    test.add_account(f.market, account(market_data, ID));
    test.add_account(f.ask, account(bid_data, ID));
    test.add_account(f.owners, account(owner_data, ID));
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.quote_vault,
        account(
            token_data(f.quote_mint, f.vault_authority, collateral, false),
            TOKEN,
        ),
    );
    test.add_account(
        f.taker_base,
        account(
            token_data(f.base_mint, f.taker.pubkey(), 2_000, false),
            TOKEN,
        ),
    );
    if count == 16 {
        // Preload a real, active lookup-table account. All nonsigner BID
        // accounts are eligible for v0 lookup compression; signer keys remain
        // static as required by the Solana message compiler.
        let table_key = Pubkey::new_unique();
        let mut table_keys = vec![
            f.market,
            f.custody,
            f.reserve,
            f.epoch,
            f.ask,
            f.owners,
            f.taker_quote,
            f.taker_base,
            f.quote_vault,
            f.base_vault,
            f.quote_mint,
            f.base_mint,
            f.vault_authority,
            TOKEN,
            Pubkey::default(),
            f.balance_a,
            f.balance_b,
        ];
        table_keys.extend(&f.extra_balances);
        let table = AddressLookupTable {
            meta: LookupTableMeta::default(),
            addresses: Cow::Owned(table_keys),
        };
        test.add_account(
            table_key,
            Account {
                lamports: 50_000_000,
                data: table.serialize_for_tests().unwrap(),
                owner: lookup_table_program::id(),
                executable: false,
                rent_epoch: 0,
            },
        );
        f.lookup_table = Some(table_key);
    }
    f
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

fn seal_bid(f: &Fixture, payer: Pubkey, base: u64, min_quote: u64) -> Instruction {
    let mut instruction = seal(f, payer, base, min_quote);
    instruction.data[0] = 42;
    instruction
        .accounts
        .push(AccountMeta::new(f.balance_a, false));
    instruction
        .accounts
        .push(AccountMeta::new(f.balance_b, false));
    for balance in &f.extra_balances {
        instruction.accounts.push(AccountMeta::new(*balance, false));
    }
    instruction
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

fn claim_bid(f: &Fixture, seat: u32, sibling: [u8; 32], destination: Pubkey) -> Instruction {
    let mut instruction = claim(f, seat, sibling, destination);
    let amounts: [u64; 6] = if seat == 0 {
        [0, 50, 50, 0, 0, 0]
    } else {
        [0, 99, 100, 0, 1, 0]
    };
    for (index, amount) in amounts.iter().enumerate() {
        instruction.data[45 + index * 8..53 + index * 8].copy_from_slice(&amount.to_le_bytes());
    }
    instruction.accounts[6] = AccountMeta::new(f.base_vault, false);
    instruction.accounts[8] = AccountMeta::new_readonly(f.base_mint, false);
    instruction
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
    if ix.data.first() == Some(&40) || ix.data.first() == Some(&42) || ix.data.first() == Some(&44)
    {
        let mut data = vec![2u8]; // SetComputeUnitLimit, u32 little-endian
        let limit = if ix.data.first() == Some(&42) || ix.data.first() == Some(&44) {
            1_400_000u32
        } else {
            CLAIM_CU_LIMIT
        };
        data.extend_from_slice(&limit.to_le_bytes());
        instructions.push(Instruction {
            program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
            accounts: vec![],
            data,
        });
    }
    let ix_opcode = ix.data.first().copied();
    instructions.push(ix);
    let check_packet = ix_opcode == Some(44);
    let signed = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &all_signers,
        blockhash,
    );
    if check_packet {
        // Legacy transaction bytes must actually be network-sendable,
        // not merely accepted by the in-process BanksClient.
        assert!(bincode::serialize(&signed).unwrap().len() <= 1_232);
    }
    context.banks_client.process_transaction(signed).await
}
// A 16-seat BID cannot fit into a legacy network packet. Resolve the
// authenticated account list through a real v0 Address Lookup Table and
// reject any transaction exceeding the 1,232-byte Solana packet limit.
async fn tx_bid_v0(
    context: &mut ProgramTestContext,
    f: &Fixture,
    ix: Instruction,
) -> Result<(), solana_program_test::BanksClientError> {
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let mut data = vec![2u8]; // SetComputeUnitLimit
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let table_key = f
        .lookup_table
        .expect("16-seat fixture must have a real ALT");
    let raw_table = context
        .banks_client
        .get_account(table_key)
        .await
        .unwrap()
        .expect("lookup table must exist in the bank");
    assert_eq!(raw_table.owner, lookup_table_program::id());
    let table = AddressLookupTable::deserialize(&raw_table.data).unwrap();
    let address_table = AddressLookupTableAccount {
        key: table_key,
        addresses: table.addresses.to_vec(),
    };
    let message = v0::Message::try_compile(
        &context.payer.pubkey(),
        &[budget, ix],
        &[address_table],
        blockhash,
    )
    .unwrap();
    assert!(!message.address_table_lookups.is_empty());
    let signed =
        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&context.payer, &f.taker])
            .unwrap();
    const SOLANA_PACKET_BYTES: usize = 1_232;
    let wire_bytes = bincode::serialize(&signed).unwrap();
    assert!(
        wire_bytes.len() <= SOLANA_PACKET_BYTES,
        "16-seat BID v0 transaction exceeds network packet limit: {} bytes",
        wire_bytes.len()
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

async fn snapshot_bytes(context: &mut ProgramTestContext, pubkey: Pubkey) -> Vec<u8> {
    // Negative transaction fixtures run before the frozen epoch PDA is
    // created. Preserve its absence as an empty snapshot, without weakening
    // successful claim tests that require the account to exist.
    context
        .banks_client
        .get_account(pubkey)
        .await
        .unwrap()
        .map(|account| account.data)
        .unwrap_or_default()
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
        accounts.push(snapshot_bytes(context, key).await);
    }
    accounts
}

// New opcode 43 is a read-only authenticated cross-page planning gate.
// It does not create epochs or update book/custody/SPL accounts.
fn multilevel_bid_preflight(
    f: &Fixture,
    tail: Option<(Pubkey, Pubkey)>,
    base: u64,
    min_quote: u64,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new_readonly(f.market, false),
        AccountMeta::new_readonly(f.ask, false),
        AccountMeta::new_readonly(f.owners, false),
    ];
    if let Some((page, owner)) = tail {
        accounts.push(AccountMeta::new_readonly(page, false));
        accounts.push(AccountMeta::new_readonly(owner, false));
    }
    let mut data = vec![43, if accounts.len() == 3 { 1 } else { 2 }];
    data.extend_from_slice(&base.to_le_bytes());
    data.extend_from_slice(&min_quote.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts,
        data,
    }
}

fn fixture_funded_multipage_bid() -> (Fixture, Pubkey, Pubkey, Pubkey) {
    let (mut f, tail, tail_owner) = fixture_bid_two_linked_prices();
    let maker = Keypair::new();
    let (balance, bump) = Pubkey::find_program_address(
        &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
        &ID,
    );
    let destination = Pubkey::new_unique();
    let test = f.test.as_mut().unwrap();
    let mut state = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
    state.locked_quote = 50;
    let mut balance_data = vec![0; MAKER_BALANCE_BYTES];
    state.encode_into(&mut balance_data).unwrap();
    test.add_account(balance, account(balance_data, ID));
    test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
    test.add_account(
        destination,
        account(token_data(f.base_mint, maker.pubkey(), 0, false), TOKEN),
    );
    let mut sidecar = AskOwnerPage::default();
    sidecar.set_links(PageLinks::new(1, Some(0), None));
    sidecar.owners[0] = maker.pubkey().to_bytes();
    sidecar.len = 1;
    let mut bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    sidecar.encode_into(&mut bytes).unwrap();
    test.add_account(tail_owner, account(bytes, ID));
    // Add the tail order's 50-quote resting collateral to both custody
    // accounting and the actual SPL quote vault.
    let (custody_key, custody_bump) =
        Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    assert_eq!(custody_key, f.custody);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump: custody_bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 0,
        total_quote: 350,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut custody_data).unwrap();
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.quote_vault,
        account(
            token_data(f.quote_mint, f.vault_authority, 350, false),
            TOKEN,
        ),
    );
    f.extra_makers.push(maker);
    f.extra_balances.push(balance);
    f.extra_destinations.push(destination);
    (f, tail, tail_owner, balance)
}

fn seal_multipage_bid(
    f: &Fixture,
    payer: Pubkey,
    tail: Pubkey,
    tail_owner: Pubkey,
    epoch_one: Pubkey,
    base: u64,
    min_quote: u64,
) -> Instruction {
    let mut data = vec![44];
    data.extend_from_slice(&base.to_le_bytes());
    data.extend_from_slice(&min_quote.to_le_bytes());
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new(f.market, false),
            AccountMeta::new(f.custody, false),
            AccountMeta::new(f.reserve, false),
            AccountMeta::new(f.epoch, false),
            AccountMeta::new(epoch_one, false),
            AccountMeta::new(f.ask, false),
            AccountMeta::new(f.owners, false),
            AccountMeta::new(tail, false),
            AccountMeta::new(tail_owner, false),
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
            AccountMeta::new(f.balance_a, false),
            AccountMeta::new(f.balance_b, false),
            AccountMeta::new(f.extra_balances[0], false),
        ],
        data,
    }
}

fn multipage_bid_claim(
    f: &Fixture,
    epoch: Pubkey,
    maker: Pubkey,
    balance: Pubkey,
    destination: Pubkey,
    seat: u32,
    weight: u64,
    totals: [u64; 6],
    proof: &[[u8; 32]],
) -> Instruction {
    let mut data = vec![40];
    data.extend_from_slice(&GENESIS);
    data.extend_from_slice(&seat.to_le_bytes());
    data.extend_from_slice(&weight.to_le_bytes());
    for amount in totals {
        data.extend_from_slice(&amount.to_le_bytes());
    }
    data.push(proof.len() as u8);
    for sibling in proof {
        data.extend_from_slice(sibling);
    }
    Instruction {
        program_id: ID,
        accounts: vec![
            AccountMeta::new_readonly(f.market, false),
            AccountMeta::new(epoch, false),
            AccountMeta::new(balance, false),
            AccountMeta::new_readonly(maker, true),
            AccountMeta::new_readonly(f.custody, false),
            AccountMeta::new(f.reserve, false),
            AccountMeta::new(f.base_vault, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(f.base_mint, false),
            AccountMeta::new_readonly(f.vault_authority, false),
            AccountMeta::new_readonly(TOKEN, false),
        ],
        data,
    }
}

/// Both executable ticks must be sealed in one all-or-nothing transaction.
#[tokio::test]
async fn funded_eight_maker_multipage_bid_executes_within_budget() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.set_bid_count(8);
    header.next_sequence = 9;
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&f.owners.to_bytes()[..16]);
    header.set_bid_owner_tag(tag);
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let test = f.test.as_mut().unwrap();
    // Preserve the original three maker balances; add five lower-tick makers.
    for _ in 0..5 {
        let maker = Keypair::new();
        let (key, bump) = Pubkey::find_program_address(
            &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
            &ID,
        );
        let mut balance = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
        balance.locked_quote = 50;
        let mut data = vec![0; MAKER_BALANCE_BYTES];
        balance.encode_into(&mut data).unwrap();
        test.add_account(key, account(data, ID));
        f.extra_makers.push(maker);
        f.extra_balances.push(key);
    }
    let mut page = BidPage::default();
    let mut owners = AskOwnerPage::default();
    page.set_links(PageLinks::new(1, Some(0), None));
    owners.set_links(PageLinks::new(1, Some(0), None));
    for (index, maker) in f.extra_makers.iter().enumerate() {
        page.insert(AskEntry {
            price_x64: Q64 / 2,
            sqrt_price_x64: 13_043_817_825_332_782_213u128,
            base_qty: 100,
            sequence: index as u64 + 3,
        })
        .unwrap();
        owners.owners[index] = maker.pubkey().to_bytes();
    }
    owners.len = 6;
    let mut page_data = vec![0; BID_PAGE_BYTES];
    page.encode_into(&mut page_data).unwrap();
    let mut owner_data = vec![0; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();
    let (_, bump) = Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 0,
        total_quote: 600,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut custody_data).unwrap();
    test.add_account(f.market, account(market_data, ID));
    test.add_account(tail, account(page_data, ID));
    test.add_account(tail_owner, account(owner_data, ID));
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.quote_vault,
        account(
            token_data(f.quote_mint, f.vault_authority, 600, false),
            TOKEN,
        ),
    );
    test.add_account(
        f.taker_base,
        account(token_data(f.base_mint, f.taker.pubkey(), 900, false), TOKEN),
    );
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let mut instruction = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 900, 600);
    instruction.accounts.extend(
        f.extra_balances[1..]
            .iter()
            .map(|key| AccountMeta::new(*key, false)),
    );
    assert_eq!(instruction.accounts.len(), 28);
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let signed = Transaction::new_signed_with_payer(
        &[budget, instruction],
        Some(&payer),
        &[&context.payer, &f.taker],
        blockhash,
    );
    let wire_bytes = bincode::serialize(&signed).unwrap().len();
    assert!(wire_bytes <= 1_232);
    let outcome = context
        .banks_client
        .process_transaction_with_metadata(signed)
        .await
        .unwrap();
    outcome.result.unwrap();
    let units = outcome.metadata.unwrap().compute_units_consumed;
    eprintln!("opcode 44 eight-maker: {wire_bytes} bytes, {units} CU");
    assert!(units > 0 && units <= 1_400_000);
    let first = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    let second = FrozenEpochView::decode(&bytes(&mut context, epoch_one).await).unwrap();
    assert_eq!(first.header.maker_count, 2);
    assert_eq!(second.header.maker_count, 6);
    assert_eq!(first.header.totals[2] + second.header.totals[2], 600);
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 900);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 0);
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 2);
    assert_eq!(reserve.vault_floor(0, 0), Ok((900, 0)));
}

#[tokio::test]
async fn funded_sixteen_maker_multipage_bid_executes_and_claims() {
    exercise_sixteen_maker_multipage_bid(false).await;
}

#[tokio::test]
async fn sixteen_maker_multipage_bid_late_cpi_failure_rolls_back() {
    exercise_sixteen_maker_multipage_bid(true).await;
}

async fn exercise_sixteen_maker_multipage_bid(freeze_quote_destination: bool) {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.set_bid_count(16);
    header.next_sequence = 17;
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&f.owners.to_bytes()[..16]);
    header.set_bid_owner_tag(tag);
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let test = f.test.as_mut().unwrap();
    // Preserve the original three maker balances; add thirteen lower-tick makers.
    for _ in 0..13 {
        let maker = Keypair::new();
        let (key, bump) = Pubkey::find_program_address(
            &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
            &ID,
        );
        let mut balance = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
        balance.locked_quote = 50;
        let mut data = vec![0; MAKER_BALANCE_BYTES];
        balance.encode_into(&mut data).unwrap();
        test.add_account(key, account(data, ID));
        let destination = Pubkey::new_unique();
        test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
        test.add_account(
            destination,
            account(token_data(f.base_mint, maker.pubkey(), 0, false), TOKEN),
        );
        f.extra_destinations.push(destination);
        f.extra_makers.push(maker);
        f.extra_balances.push(key);
    }
    let mut page = BidPage::default();
    let mut owners = AskOwnerPage::default();
    page.set_links(PageLinks::new(1, Some(0), None));
    owners.set_links(PageLinks::new(1, Some(0), None));
    for (index, maker) in f.extra_makers.iter().enumerate() {
        page.insert(AskEntry {
            price_x64: Q64 / 2,
            sqrt_price_x64: 13_043_817_825_332_782_213u128,
            base_qty: 100,
            sequence: index as u64 + 3,
        })
        .unwrap();
        owners.owners[index] = maker.pubkey().to_bytes();
    }
    owners.len = 14;
    let mut page_data = vec![0; BID_PAGE_BYTES];
    page.encode_into(&mut page_data).unwrap();
    let mut owner_data = vec![0; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();
    let (_, bump) = Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 0,
        total_quote: 1000,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut custody_data).unwrap();
    test.add_account(f.market, account(market_data, ID));
    test.add_account(tail, account(page_data, ID));
    test.add_account(tail_owner, account(owner_data, ID));
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.quote_vault,
        account(
            token_data(f.quote_mint, f.vault_authority, 1000, false),
            TOKEN,
        ),
    );
    test.add_account(
        f.taker_base,
        account(
            token_data(f.base_mint, f.taker.pubkey(), 1700, false),
            TOKEN,
        ),
    );
    if freeze_quote_destination {
        test.add_account(
            f.taker_quote,
            account(token_data(f.quote_mint, f.taker.pubkey(), 0, true), TOKEN),
        );
    }
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let table_key = Pubkey::new_unique();
    let mut keys = vec![
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        epoch_one,
        f.ask,
        f.owners,
        tail,
        tail_owner,
        f.taker_quote,
        f.taker_base,
        f.quote_vault,
        f.base_vault,
        f.quote_mint,
        f.base_mint,
        f.vault_authority,
        TOKEN,
        Pubkey::default(),
        f.balance_a,
        f.balance_b,
    ];
    keys.extend(&f.extra_balances);
    let table = AddressLookupTable {
        meta: LookupTableMeta::default(),
        addresses: Cow::Owned(keys.clone()),
    };
    test.add_account(
        table_key,
        Account {
            lamports: 50_000_000,
            data: table.serialize_for_tests().unwrap(),
            owner: lookup_table_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let mut instruction = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 1700, 1000);
    instruction.accounts.extend(
        f.extra_balances[1..]
            .iter()
            .map(|key| AccountMeta::new(*key, false)),
    );
    assert_eq!(instruction.accounts.len(), 36);
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let address_table = AddressLookupTableAccount {
        key: table_key,
        addresses: keys,
    };
    // Reject malformed expanded account lists before consuming any collateral.
    let observed: Vec<_> = instruction
        .accounts
        .iter()
        .filter(|meta| meta.is_writable && meta.pubkey != payer)
        .map(|meta| meta.pubkey)
        .collect();
    let mut before = Vec::new();
    for key in &observed {
        before.push(context.banks_client.get_account(*key).await.unwrap());
    }
    for case in 0..5 {
        let mut invalid = instruction.clone();
        match case {
            0 => invalid.accounts.swap(20, 35),
            1 => invalid.accounts[35].is_writable = false,
            2 => {
                let duplicate = invalid.accounts[20].clone();
                invalid.accounts.push(duplicate);
            }
            3 => {
                invalid.accounts.pop();
            }
            _ => invalid.data[9..17].copy_from_slice(&1001u64.to_le_bytes()),
        }
        let blockhash = context.get_new_latest_blockhash().await.unwrap();
        let message = v0::Message::try_compile(
            &payer,
            &[budget.clone(), invalid],
            std::slice::from_ref(&address_table),
            blockhash,
        )
        .unwrap();
        let signed = VersionedTransaction::try_new(
            VersionedMessage::V0(message),
            &[&context.payer, &f.taker],
        )
        .unwrap();
        assert!(bincode::serialize(&signed).unwrap().len() <= 1_232);
        assert!(context
            .banks_client
            .process_transaction(signed)
            .await
            .is_err());
        for (key, expected) in observed.iter().zip(&before) {
            assert_eq!(
                context
                    .banks_client
                    .get_account(*key)
                    .await
                    .unwrap()
                    .as_ref(),
                expected.as_ref(),
            );
        }
    }
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let message =
        v0::Message::try_compile(&payer, &[budget, instruction], &[address_table], blockhash)
            .unwrap();
    assert!(!message.address_table_lookups.is_empty());
    let signed =
        VersionedTransaction::try_new(VersionedMessage::V0(message), &[&context.payer, &f.taker])
            .unwrap();
    let wire_bytes = bincode::serialize(&signed).unwrap().len();
    assert!(wire_bytes <= 1_232);
    let outcome = context
        .banks_client
        .process_transaction_with_metadata(signed)
        .await
        .unwrap();
    if freeze_quote_destination {
        let error = outcome.result.unwrap_err();
        assert_eq!(
            format!("{error:?}"),
            "InstructionError(1, Custom(17))",
            "expected SPL Token AccountFrozen on the settlement instruction"
        );
        let metadata = outcome.metadata.unwrap();
        // The bank may duplicate native CPI log entries and omit SPL's
        // human-readable messages. Require ordered runtime success/failure.
        let success = format!("Program {TOKEN} success");
        let failure = format!("Program {TOKEN} failed: custom program error: 0x11");
        let first_success = metadata
            .log_messages
            .iter()
            .position(|line| line == &success)
            .expect("base transfer must succeed before quote transfer");
        assert!(
            metadata.log_messages[first_success + 1..]
                .iter()
                .any(|line| line == &failure),
            "quote transfer must fail with AccountFrozen after base transfer"
        );
        for (key, expected) in observed.iter().zip(&before) {
            assert_eq!(
                context
                    .banks_client
                    .get_account(*key)
                    .await
                    .unwrap()
                    .as_ref(),
                expected.as_ref(),
            );
        }
        return;
    }
    outcome.result.unwrap();
    let units = outcome.metadata.unwrap().compute_units_consumed;
    eprintln!("opcode 44 sixteen-maker: {wire_bytes} bytes, {units} CU");
    assert!(units > 0 && units <= 1_400_000);

    for (epoch_key, makers, balances, destinations, weights, claims) in [
        (
            f.epoch,
            vec![&f.maker_a, &f.maker_b],
            vec![f.balance_a, f.balance_b],
            vec![f.destination_a, f.destination_b],
            vec![100u64, 200],
            vec![[0, 99, 100, 0, 1, 0], [0, 198, 200, 0, 2, 0]],
        ),
        (
            epoch_one,
            f.extra_makers.iter().collect(),
            f.extra_balances.clone(),
            f.extra_destinations.clone(),
            vec![100u64; 14],
            vec![[0, 99, 50, 0, 1, 0]; 14],
        ),
    ] {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, epoch_key).await).unwrap();
        assert_eq!(epoch.header.maker_count as usize, makers.len());
        let mut levels = vec![makers
            .iter()
            .enumerate()
            .map(|(seat, maker)| {
                leaf(
                    &epoch.header.commitment,
                    seat as u32,
                    maker.pubkey(),
                    weights[seat],
                    claims[seat],
                )
            })
            .collect::<Vec<_>>()];
        while levels.last().unwrap().len() > 1 {
            let next = levels
                .last()
                .unwrap()
                .chunks(2)
                .map(|chunk| {
                    hashv(&[
                        b"HYBRID_MAKER_PUBKEY_NODE_V1\0",
                        &chunk[0],
                        chunk.get(1).unwrap_or(&chunk[0]),
                    ])
                    .to_bytes()
                })
                .collect();
            levels.push(next);
        }
        assert_eq!(epoch.header.claim_root, levels.last().unwrap()[0]);
        for seat in (0..makers.len()).rev() {
            let mut proof = Vec::new();
            let mut index = seat;
            for level in levels.iter().take(levels.len() - 1) {
                proof.push(*level.get(index ^ 1).unwrap_or(&level[index]));
                index /= 2;
            }
            let claim = multipage_bid_claim(
                &f,
                epoch_key,
                makers[seat].pubkey(),
                balances[seat],
                destinations[seat],
                seat as u32,
                weights[seat],
                claims[seat],
                &proof,
            );
            tx(&mut context, claim, &[makers[seat]]).await.unwrap();
            assert_eq!(
                amount(&bytes(&mut context, destinations[seat]).await),
                claims[seat][1]
            );
            let balance =
                MakerBalance::decode_from(&bytes(&mut context, balances[seat]).await).unwrap();
            assert_eq!(balance.locked_quote, 0);
            assert_eq!(balance.free_quote, 0);
        }
        let exhausted = FrozenEpochView::decode(&bytes(&mut context, epoch_key).await).unwrap();
        assert_eq!(exhausted.claimed_count, exhausted.header.maker_count);
        assert_eq!(exhausted.remaining, [0; 6]);
    }
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 2);
    assert_eq!(reserve.maker_base_credit, 0);
    assert_eq!(reserve.maker_quote_debit, 0);
    assert_eq!(reserve.vault_floor(0, 0), Ok((17, 0)));
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 17);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 0);
}

#[tokio::test]
async fn multipage_bid_extended_roster_v0_packet_characterization() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    let context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    for seats in [8usize, 9, 16, 32] {
        let mut ix = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325);
        for _ in 3..seats {
            ix.accounts
                .push(AccountMeta::new(Pubkey::new_unique(), false));
        }
        assert_eq!(ix.accounts.len(), 20 + seats);
        // Synthetic table for serialization only; no execution or CU claim.
        // Keep signers and program IDs static, as required by v0 messages.
        let table = AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: ix
                .accounts
                .iter()
                .filter(|meta| !meta.is_signer)
                .map(|meta| meta.pubkey)
                .collect(),
        };
        let instructions = [budget.clone(), ix];
        let legacy = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer),
            &[&context.payer, &f.taker],
            context.last_blockhash,
        );
        let legacy_bytes = bincode::serialize(&legacy).unwrap().len();
        let message =
            v0::Message::try_compile(&payer, &instructions, &[table], context.last_blockhash)
                .unwrap();
        assert!(!message.address_table_lookups.is_empty());
        let versioned = VersionedTransaction::try_new(
            VersionedMessage::V0(message),
            &[&context.payer, &f.taker],
        )
        .unwrap();
        let v0_bytes = bincode::serialize(&versioned).unwrap().len();
        eprintln!("multipage BID packet: {seats} seats, legacy {legacy_bytes}, v0 {v0_bytes}");
        assert!(v0_bytes < legacy_bytes);
        assert!(v0_bytes <= 1_232);
        if seats >= 16 {
            assert!(legacy_bytes > 1_232);
        }
    }
}

#[tokio::test]
async fn multipage_bid_eight_maker_legacy_packet_fits_wire_limit() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    let context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let mut instruction = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325);
    // Five additional distinct writable keys model the maximum ABI roster.
    // This is a wire-size test, not an execution or compute measurement.
    for _ in 0..5 {
        instruction
            .accounts
            .push(AccountMeta::new(Pubkey::new_unique(), false));
    }
    assert_eq!(instruction.accounts.len(), 28);
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let signed = Transaction::new_signed_with_payer(
        &[budget, instruction],
        Some(&payer),
        &[&context.payer, &f.taker],
        context.last_blockhash,
    );
    let wire_bytes = bincode::serialize(&signed).unwrap().len();
    assert!(wire_bytes <= 1_232, "eight-maker wire size: {wire_bytes}");
}

#[tokio::test]
async fn funded_two_page_bid_seals_two_epochs_and_compacts_order_chain() {
    let (mut f, tail, tail_owner, _balance) = fixture_funded_multipage_bid();
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let before_base = amount(&bytes(&mut context, f.taker_base).await);
    let before_quote = amount(&bytes(&mut context, f.taker_quote).await);
    tx(
        &mut context,
        seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325),
        &[&f.taker],
    )
    .await
    .unwrap();

    let first = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    let second = FrozenEpochView::decode(&bytes(&mut context, epoch_one).await).unwrap();
    assert_eq!(first.header.epoch_id, 0);
    assert_eq!(second.header.epoch_id, 1);
    assert_eq!(first.header.tick_sqrt_price_x64, Q64);
    assert_eq!(
        second.header.tick_sqrt_price_x64,
        13_043_817_825_332_782_213u128
    );
    assert_eq!(first.header.maker_count, 2);
    assert_eq!(second.header.maker_count, 1);
    assert_eq!(first.header.totals, [0, 297, 300, 0, 3, 0]);
    assert_eq!(second.header.totals, [0, 50, 25, 0, 0, 0]);
    assert_ne!(first.header.commitment, second.header.commitment);
    assert_eq!(
        amount(&bytes(&mut context, f.taker_base).await),
        before_base - 350
    );
    assert_eq!(
        amount(&bytes(&mut context, f.taker_quote).await),
        before_quote + 325
    );
    let head = BidPage::decode_from(&bytes(&mut context, f.ask).await).unwrap();
    assert_eq!(head.len(), 1);
    assert_eq!(head.as_slice()[0].base_qty, 50);
    assert_eq!(head.as_slice()[0].sequence, 3);
    assert_eq!(head.links(), PageLinks::new(0, None, None));
    let compacted_owners = AskOwnerPage::decode_from(&bytes(&mut context, f.owners).await).unwrap();
    assert_eq!(
        compacted_owners.as_slice()[0],
        f.extra_makers[0].pubkey().to_bytes()
    );
    let retired = BidPage::decode_from(&bytes(&mut context, tail).await).unwrap();
    assert_eq!(retired.len(), 0);
    assert_eq!(retired.links().page_index, 1);

    // Immutable commitments bind maker claims to each tick's own roster.
    // Claim the singleton lower-price epoch first, then maker 0 of the
    // two-seat high-price epoch, deliberately out of epoch order.
    let lower_amounts = [0, 50, 25, 0, 0, 0];
    assert_eq!(
        second.header.claim_root,
        leaf(
            &second.header.commitment,
            0,
            f.extra_makers[0].pubkey(),
            100,
            lower_amounts
        ),
    );
    let low_claim = multipage_bid_claim(
        &f,
        epoch_one,
        f.extra_makers[0].pubkey(),
        f.extra_balances[0],
        f.extra_destinations[0],
        0,
        100,
        lower_amounts,
        &[],
    );
    let cross_epoch = multipage_bid_claim(
        &f,
        f.epoch,
        f.extra_makers[0].pubkey(),
        f.extra_balances[0],
        f.extra_destinations[0],
        0,
        100,
        lower_amounts,
        &[],
    );
    assert!(tx(&mut context, cross_epoch, &[&f.extra_makers[0]])
        .await
        .is_err());
    // Replay an otherwise valid claim under a different market identity.
    let mut foreign_claim = low_claim.clone();
    foreign_claim.accounts[0] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
    let mut observed = vec![f.market, f.custody, f.reserve, f.epoch, epoch_one];
    observed.extend([
        f.extra_balances[0],
        f.extra_destinations[0],
        f.base_vault,
        f.quote_vault,
    ]);
    let mut before = Vec::new();
    for key in &observed {
        before.push(context.banks_client.get_account(*key).await.unwrap());
    }
    assert!(tx(&mut context, foreign_claim, &[&f.extra_makers[0]])
        .await
        .is_err());
    for (key, expected) in observed.iter().zip(before) {
        assert_eq!(
            context.banks_client.get_account(*key).await.unwrap(),
            expected,
        );
    }
    tx(&mut context, low_claim.clone(), &[&f.extra_makers[0]])
        .await
        .unwrap();
    assert_eq!(
        amount(&bytes(&mut context, f.extra_destinations[0]).await),
        50
    );
    assert!(tx(&mut context, low_claim, &[&f.extra_makers[0]])
        .await
        .is_err());

    let high_zero = [0, 99, 100, 0, 1, 0];
    let high_one = [0, 198, 200, 0, 2, 0];
    let maker_one_leaf = leaf(
        &first.header.commitment,
        1,
        f.maker_b.pubkey(),
        200,
        high_one,
    );
    let maker_zero_leaf = leaf(
        &first.header.commitment,
        0,
        f.maker_a.pubkey(),
        100,
        high_zero,
    );
    assert_eq!(
        first.header.claim_root,
        hashv(&[
            b"HYBRID_MAKER_PUBKEY_NODE_V1\0",
            &maker_zero_leaf,
            &maker_one_leaf,
        ])
        .to_bytes(),
    );
    tx(
        &mut context,
        multipage_bid_claim(
            &f,
            f.epoch,
            f.maker_a.pubkey(),
            f.balance_a,
            f.destination_a,
            0,
            100,
            high_zero,
            &[maker_one_leaf],
        ),
        &[&f.maker_a],
    )
    .await
    .unwrap();
    assert_eq!(amount(&bytes(&mut context, f.destination_a).await), 99);
    tx(
        &mut context,
        multipage_bid_claim(
            &f,
            f.epoch,
            f.maker_b.pubkey(),
            f.balance_b,
            f.destination_b,
            1,
            200,
            high_one,
            &[maker_zero_leaf],
        ),
        &[&f.maker_b],
    )
    .await
    .unwrap();
    assert_eq!(amount(&bytes(&mut context, f.destination_b).await), 198);
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 2);
    assert_eq!(reserve.maker_base_credit, 0);
    assert_eq!(reserve.maker_quote_debit, 0);
    assert_eq!(reserve.vault_floor(0, 25), Ok((3, 25)));
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 3);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 25);
    for key in [f.epoch, epoch_one] {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, key).await).unwrap();
        assert_eq!(epoch.claimed_count, epoch.header.maker_count);
    }
}

#[tokio::test]
async fn multipage_bid_preserves_historical_reserve_floors() {
    // Inject a historical reserve at genesis of the test bank. The old
    // liability is deliberately independent of the current maker roster.
    for solvent in [false, true] {
        let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
        let (_, bump) = Pubkey::find_program_address(&[b"lazy-reserve", f.market.as_ref()], &ID);
        let mut historical = LazyReserve::new(bump, f.market.to_bytes(), GENESIS).unwrap();
        historical
            .reserve_execution(0, [0, 17, 9, 0, 3, 0])
            .unwrap();
        let mut reserve_data = vec![0; LAZY_RESERVE_BYTES];
        historical.encode(&mut reserve_data).unwrap();
        let test = f.test.as_mut().unwrap();
        test.add_account(f.reserve, account(reserve_data, ID));
        test.add_account(
            f.base_vault,
            account(
                token_data(
                    f.base_mint,
                    f.vault_authority,
                    if solvent { 20 } else { 19 },
                    false,
                ),
                TOKEN,
            ),
        );
        let (epoch_two, _) = Pubkey::find_program_address(
            &[b"frozen-epoch", f.market.as_ref(), &2u64.to_le_bytes()],
            &ID,
        );
        let (epoch_one, _) = Pubkey::find_program_address(
            &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
            &ID,
        );
        let mut context = f.test.take().unwrap().start_with_context().await;
        let payer = context.payer.pubkey();
        let mut instruction = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_two, 350, 325);
        instruction.accounts[3] = AccountMeta::new(epoch_one, false);
        let observed: Vec<_> = instruction
            .accounts
            .iter()
            .filter(|meta| meta.is_writable && meta.pubkey != payer)
            .map(|meta| meta.pubkey)
            .collect();
        let mut before = Vec::new();
        for key in &observed {
            before.push(context.banks_client.get_account(*key).await.unwrap());
        }
        let outcome = tx(&mut context, instruction, &[&f.taker]).await;
        if solvent {
            outcome.unwrap();
            let mut expected = historical;
            expected
                .reserve_execution(1, [0, 297, 300, 0, 3, 0])
                .unwrap();
            expected.reserve_execution(2, [0, 50, 25, 0, 0, 0]).unwrap();
            let actual = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.vault_floor(0, 25), Ok((370, 25)));
            assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 370);
            assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 25);
        } else {
            assert!(outcome.is_err());
            for (key, expected) in observed.iter().zip(before) {
                assert_eq!(
                    context.banks_client.get_account(*key).await.unwrap(),
                    expected,
                );
            }
        }
    }
}

#[tokio::test]
async fn multipage_bid_insufficient_taker_base_cpi_rolls_back_both_new_epochs() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    // Keep all resting maker collateral and both vault floors solvent.
    // Only the taker has 349 base atoms rather than the requested 350.
    // This cannot fail planner, epoch-PDA, or pre-CPI vault preflight;
    // the checked SPL base transfer must fail after both epoch creations.
    f.test.as_mut().unwrap().add_account(
        f.taker_base,
        account(token_data(f.base_mint, f.taker.pubkey(), 349, false), TOKEN),
    );
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let observed = [
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        epoch_one,
        f.ask,
        f.owners,
        tail,
        tail_owner,
        f.balance_a,
        f.balance_b,
        f.extra_balances[0],
        f.quote_vault,
        f.base_vault,
        f.taker_base,
        f.taker_quote,
    ];
    let mut before = Vec::new();
    for key in observed {
        before.push(context.banks_client.get_account(key).await.unwrap());
    }
    assert!(before[3].is_none());
    assert!(before[4].is_none());
    let outcome = tx(
        &mut context,
        seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325),
        &[&f.taker],
    )
    .await;
    assert!(outcome.is_err(), "insufficient taker SPL base must fail");
    for (index, key) in observed.iter().enumerate() {
        assert_eq!(
            context.banks_client.get_account(*key).await.unwrap(),
            before[index],
            "transaction modified account at index {index}",
        );
    }
}

#[tokio::test]
async fn multipage_bid_frozen_quote_destination_rolls_back_both_new_epochs() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_bid();
    // Freeze the outgoing quote destination to exercise rejection and rollback.
    f.test.as_mut().unwrap().add_account(
        f.taker_quote,
        account(token_data(f.quote_mint, f.taker.pubkey(), 0, true), TOKEN),
    );
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let observed = [
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        epoch_one,
        f.ask,
        f.owners,
        tail,
        tail_owner,
        f.balance_a,
        f.balance_b,
        f.extra_balances[0],
        f.quote_vault,
        f.base_vault,
        f.taker_base,
        f.taker_quote,
    ];
    let mut before = Vec::new();
    for key in observed {
        before.push(context.banks_client.get_account(key).await.unwrap());
    }
    assert!(before[3].is_none());
    assert!(before[4].is_none());
    let outcome = tx(
        &mut context,
        seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325),
        &[&f.taker],
    )
    .await;
    assert!(outcome.is_err(), "frozen taker quote destination must fail");
    for (index, key) in observed.iter().enumerate() {
        assert_eq!(
            context.banks_client.get_account(*key).await.unwrap(),
            before[index],
            "transaction modified account at index {index}",
        );
    }
}

#[tokio::test]
async fn multipage_bid_rejects_late_tick_failures_without_any_account_writes() {
    let (mut f, tail, tail_owner, _balance) = fixture_funded_multipage_bid();
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let observed = [
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        epoch_one,
        f.ask,
        f.owners,
        tail,
        tail_owner,
        f.balance_a,
        f.balance_b,
        f.extra_balances[0],
        f.quote_vault,
        f.base_vault,
        f.taker_base,
        f.taker_quote,
    ];
    let mut before = Vec::new();
    for key in observed {
        before.push(context.banks_client.get_account(key).await.unwrap());
    }
    let good = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 325);
    let mut forged_owner = good.clone();
    forged_owner.accounts[8] = AccountMeta::new(f.balance_a, false);
    let mut swapped_balances = good.clone();
    swapped_balances.accounts.swap(20, 21);
    let mut duplicate = good.clone();
    duplicate.accounts[22] = duplicate.accounts[21].clone();
    let mut bad_epoch = good.clone();
    bad_epoch.accounts[4] = AccountMeta::new(Pubkey::new_unique(), false);
    let mut foreign_epoch = good.clone();
    let foreign_market = Pubkey::new_unique();
    let (foreign_epoch_key, _) = Pubkey::find_program_address(
        &[
            b"frozen-epoch",
            foreign_market.as_ref(),
            &1u64.to_le_bytes(),
        ],
        &ID,
    );
    foreign_epoch.accounts[4] = AccountMeta::new(foreign_epoch_key, false);
    let mut readonly_balance = good.clone();
    readonly_balance.accounts[22].is_writable = false;
    let mut wrong_system = good.clone();
    wrong_system.accounts[19] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
    let bad_slippage = seal_multipage_bid(&f, payer, tail, tail_owner, epoch_one, 350, 326);
    for instruction in [
        forged_owner,
        swapped_balances,
        duplicate,
        bad_epoch,
        foreign_epoch,
        readonly_balance,
        wrong_system,
        bad_slippage,
    ] {
        assert!(tx(&mut context, instruction, &[&f.taker]).await.is_err());
        for (index, key) in observed.iter().enumerate() {
            assert_eq!(
                context.banks_client.get_account(*key).await.unwrap(),
                before[index],
            );
        }
    }
}

fn fixture_bid_two_linked_prices() -> (Fixture, Pubkey, Pubkey) {
    let mut f = fixture_bid(false);
    let next_index = 1u32.to_le_bytes();
    let (tail_key, _) =
        Pubkey::find_program_address(&[b"bid-page", f.market.as_ref(), &next_index], &ID);
    let (tail_owner_key, _) =
        Pubkey::find_program_address(&[b"bid-owner-page", f.market.as_ref(), &next_index], &ID);

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    market.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    market.set_bid_count(3);
    market.next_sequence = 4;
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&f.owners.to_bytes()[..16]);
    market.set_bid_owner_tag(tag);
    let mut market_bytes = vec![0; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_bytes).unwrap();

    let mut head = BidPage::default();
    let mut head_owners = AskOwnerPage::default();
    head.set_links(PageLinks::new(0, None, Some(1)));
    head_owners.set_links(PageLinks::new(0, None, Some(1)));
    for (index, (maker, qty)) in [(f.maker_a.pubkey(), 100u64), (f.maker_b.pubkey(), 200u64)]
        .into_iter()
        .enumerate()
    {
        head.insert(AskEntry {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: qty,
            sequence: index as u64 + 1,
        })
        .unwrap();
        head_owners.owners[index] = maker.to_bytes();
    }
    head_owners.len = 2;
    let mut head_bytes = vec![0; BID_PAGE_BYTES];
    head.encode_into(&mut head_bytes).unwrap();
    let mut head_owner_bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    head_owners.encode_into(&mut head_owner_bytes).unwrap();

    let mut tail = BidPage::default();
    let mut tail_owners = AskOwnerPage::default();
    tail.set_links(PageLinks::new(1, Some(0), None));
    tail_owners.set_links(PageLinks::new(1, Some(0), None));
    tail.insert(AskEntry {
        price_x64: Q64 / 2,
        sqrt_price_x64: 13_043_817_825_332_782_213u128,
        base_qty: 100,
        sequence: 3,
    })
    .unwrap();
    tail_owners.owners[0] = Pubkey::new_unique().to_bytes();
    tail_owners.len = 1;
    let mut tail_bytes = vec![0; BID_PAGE_BYTES];
    tail.encode_into(&mut tail_bytes).unwrap();
    let mut tail_owner_bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    tail_owners.encode_into(&mut tail_owner_bytes).unwrap();
    let test = f.test.as_mut().unwrap();
    test.add_account(f.market, account(market_bytes, ID));
    test.add_account(f.ask, account(head_bytes, ID));
    test.add_account(f.owners, account(head_owner_bytes, ID));
    test.add_account(tail_key, account(tail_bytes, ID));
    test.add_account(tail_owner_key, account(tail_owner_bytes, ID));
    (f, tail_key, tail_owner_key)
}

#[tokio::test]
async fn authenticated_multilevel_bid_preflight_crosses_two_linked_prices_without_writes() {
    let (mut f, tail, tail_owner) = fixture_bid_two_linked_prices();
    let mut context = f.test.take().unwrap().start_with_context().await;
    let observed = [f.market, f.ask, f.owners, tail, tail_owner];
    let mut before = Vec::new();
    for key in observed {
        before.push(snapshot_bytes(&mut context, key).await);
    }
    // First tick: 300 base at 1 quote/base; second tick: 50 at 1/2.
    tx(
        &mut context,
        multilevel_bid_preflight(&f, Some((tail, tail_owner)), 350, 325),
        &[],
    )
    .await
    .unwrap();
    for (index, key) in observed.into_iter().enumerate() {
        assert_eq!(snapshot_bytes(&mut context, key).await, before[index]);
    }

    // Reject underquoted output, omitted tail page, forged sidecar PDA,
    // duplicate account metas and malformed wire length without mutations.
    let mut forged = multilevel_bid_preflight(&f, Some((tail, tail_owner)), 350, 325);
    // A distinct program-owned maker balance is still not the canonical
    // page-1 owner sidecar PDA; reject it before page decoding.
    forged.accounts[4] = AccountMeta::new_readonly(f.balance_a, false);
    let mut duplicate = multilevel_bid_preflight(&f, Some((tail, tail_owner)), 350, 325);
    duplicate.accounts[3] = AccountMeta::new_readonly(f.ask, false);
    let mut malformed = multilevel_bid_preflight(&f, Some((tail, tail_owner)), 350, 325);
    malformed.data.pop();
    for instruction in [
        multilevel_bid_preflight(&f, Some((tail, tail_owner)), 350, 326),
        multilevel_bid_preflight(&f, None, 350, 325),
        forged,
        duplicate,
        malformed,
    ] {
        assert!(tx(&mut context, instruction, &[]).await.is_err());
        for (index, key) in observed.into_iter().enumerate() {
            assert_eq!(snapshot_bytes(&mut context, key).await, before[index]);
        }
    }
}

#[tokio::test]
async fn extended_bid_roster_rejects_malformed_maker_account_lists_atomically() {
    let mut f = fixture_bid_extended(9);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let mut keys = vec![
        f.market,
        f.custody,
        f.reserve,
        f.epoch,
        f.ask,
        f.owners,
        f.balance_a,
        f.balance_b,
        f.quote_vault,
        f.base_vault,
        f.taker_base,
        f.taker_quote,
    ];
    keys.extend(&f.extra_balances);
    let mut before = Vec::new();
    for key in &keys {
        before.push(snapshot_bytes(&mut context, *key).await);
    }
    let total = 1_000;
    let mut swapped = seal_bid(&f, payer, total, total);
    swapped.accounts.swap(17, 18);
    let mut missing = seal_bid(&f, payer, total, total);
    missing.accounts.pop();
    let mut duplicate = seal_bid(&f, payer, total, total);
    duplicate.accounts[18] = duplicate.accounts[17].clone();
    let mut readonly = seal_bid(&f, payer, total, total);
    readonly.accounts[25].is_writable = false;
    for instruction in [swapped, missing, duplicate, readonly] {
        assert!(tx(&mut context, instruction, &[&f.taker]).await.is_err());
        let mut after = Vec::new();
        for key in &keys {
            after.push(snapshot_bytes(&mut context, *key).await);
        }
        assert_eq!(after, before);
    }
}

#[tokio::test]
async fn extended_bid_rosters_seal_and_pay_every_maker_with_odd_merkle_proofs() {
    for count in [9usize, 16usize] {
        let mut f = fixture_bid_extended(count);
        let mut context = f.test.take().unwrap().start_with_context().await;
        let payer = context.payer.pubkey();
        tx(&mut context, initialize(&f, payer), &[&f.market_signer])
            .await
            .unwrap();

        let weights = [vec![100u64, 200u64], vec![100u64; count - 2]].concat();
        let total: u64 = weights.iter().sum();
        let seal_ix = seal_bid(&f, payer, total, total);
        if count == 16 {
            tx_bid_v0(&mut context, &f, seal_ix).await.unwrap();
        } else {
            tx(&mut context, seal_ix, &[&f.taker]).await.unwrap();
        }
        let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
        let fee = total / 100;
        assert_eq!(epoch.header.side, 1);
        assert_eq!(epoch.header.maker_count, count as u32);
        assert_eq!(epoch.header.totals, [0, total - fee, total, 0, fee, 0]);
        let mut makers = vec![f.maker_a.pubkey(), f.maker_b.pubkey()];
        makers.extend(f.extra_makers.iter().map(|maker| maker.pubkey()));
        let allocations =
            hybrid_state::lazy_pro_rata::allocate_bid_at_price(&weights, total, Q64, fee).unwrap();
        let claims: Vec<[u64; 6]> = allocations
            .iter()
            .map(|allocation| {
                [
                    0,
                    allocation.base_proceeds,
                    allocation.quote_spent,
                    0,
                    allocation.base_fee,
                    0,
                ]
            })
            .collect();
        let mut levels = vec![makers
            .iter()
            .enumerate()
            .map(|(i, maker)| {
                leaf(
                    &epoch.header.commitment,
                    i as u32,
                    *maker,
                    weights[i],
                    claims[i],
                )
            })
            .collect::<Vec<_>>()];
        while levels.last().unwrap().len() > 1 {
            let previous = levels.last().unwrap();
            let next = previous
                .chunks(2)
                .map(|chunk| {
                    hashv(&[
                        b"HYBRID_MAKER_PUBKEY_NODE_V1\0",
                        &chunk[0],
                        chunk.get(1).unwrap_or(&chunk[0]),
                    ])
                    .to_bytes()
                })
                .collect::<Vec<_>>();
            levels.push(next);
        }
        assert_eq!(epoch.header.claim_root, levels.last().unwrap()[0]);

        // All claims, including the odd terminal leaf at nine seats, use
        // the unchanged opcode 40 Merkle verifier and SPL payout path.
        for seat in (0..count).rev() {
            let maker = if seat == 0 {
                &f.maker_a
            } else if seat == 1 {
                &f.maker_b
            } else {
                &f.extra_makers[seat - 2]
            };
            let balance = if seat == 0 {
                f.balance_a
            } else if seat == 1 {
                f.balance_b
            } else {
                f.extra_balances[seat - 2]
            };
            let destination = if seat == 0 {
                f.destination_a
            } else if seat == 1 {
                f.destination_b
            } else {
                f.extra_destinations[seat - 2]
            };
            let mut instruction = claim_bid(&f, seat as u32, [0; 32], destination);
            instruction.accounts[2] = AccountMeta::new(balance, false);
            instruction.accounts[3] = AccountMeta::new_readonly(maker.pubkey(), true);
            instruction.data[37..45].copy_from_slice(&weights[seat].to_le_bytes());
            for (index, amount) in claims[seat].iter().enumerate() {
                instruction.data[45 + index * 8..53 + index * 8]
                    .copy_from_slice(&amount.to_le_bytes());
            }
            instruction.data.truncate(93);
            instruction.data.push((levels.len() - 1) as u8);
            let mut index = seat;
            for level in levels.iter().take(levels.len() - 1) {
                let sibling = level.get(index ^ 1).unwrap_or(&level[index]);
                instruction.data.extend_from_slice(sibling);
                index /= 2;
            }
            tx(&mut context, instruction, &[maker]).await.unwrap();
            assert_eq!(
                amount(&bytes(&mut context, destination).await),
                claims[seat][1]
            );
            let balance = MakerBalance::decode_from(&bytes(&mut context, balance).await).unwrap();
            assert_eq!(balance.locked_quote, 0);
            assert_eq!(balance.free_quote, 0);
        }
        let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
        assert_eq!(epoch.claimed_count, count as u32);
        assert_eq!(epoch.remaining, [0; 6]);
        assert_eq!(amount(&bytes(&mut context, f.base_vault).await), fee);
        assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 0);
        let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
        assert_eq!(reserve.maker_base_credit, 0);
        assert_eq!(reserve.maker_quote_debit, 0);
    }
}

#[tokio::test]
async fn paid_bid_execution_seals_and_claims_base_without_spending_other_reserves() {
    let mut f = fixture_bid(false);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    tx(&mut context, seal_bid(&f, payer, 150, 150), &[&f.taker])
        .await
        .unwrap();

    let epoch_data = bytes(&mut context, f.epoch).await;
    let epoch = FrozenEpochView::decode(&epoch_data).unwrap();
    assert_eq!(epoch.header.side, 1);
    assert_eq!(epoch.header.epoch_id, 0);
    assert_eq!(epoch.header.totals, [0, 149, 150, 0, 1, 0]);
    let a = leaf(
        &epoch.header.commitment,
        0,
        f.maker_a.pubkey(),
        100,
        [0, 50, 50, 0, 0, 0],
    );
    let b = leaf(
        &epoch.header.commitment,
        1,
        f.maker_b.pubkey(),
        200,
        [0, 99, 100, 0, 1, 0],
    );
    assert_eq!(
        epoch.header.claim_root,
        hashv(&[b"HYBRID_MAKER_PUBKEY_NODE_V1\0", &a, &b]).to_bytes()
    );
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 150);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 150);
    assert_eq!(amount(&bytes(&mut context, f.taker_base).await), 350);
    assert_eq!(amount(&bytes(&mut context, f.taker_quote).await), 150);
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.vault_floor(0, 150), Ok((150, 150)));

    tx(
        &mut context,
        claim_bid(&f, 1, a, f.destination_b),
        &[&f.maker_b],
    )
    .await
    .unwrap();
    tx(
        &mut context,
        claim_bid(&f, 0, b, f.destination_a),
        &[&f.maker_a],
    )
    .await
    .unwrap();
    assert_eq!(amount(&bytes(&mut context, f.destination_a).await), 50);
    assert_eq!(amount(&bytes(&mut context, f.destination_b).await), 99);
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.vault_floor(0, 150), Ok((1, 150)));
    assert_eq!(reserve.maker_base_credit, 0);
    assert_eq!(reserve.maker_quote_debit, 0);

    let before = snapshot(&mut context, &f).await;
    assert!(tx(
        &mut context,
        claim_bid(&f, 0, b, f.destination_a),
        &[&f.maker_a]
    )
    .await
    .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
}

#[tokio::test]
async fn failed_bid_claim_to_frozen_destination_rolls_back_all_balances() {
    let mut f = fixture_bid(true);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    tx(&mut context, seal_bid(&f, payer, 150, 150), &[&f.taker])
        .await
        .unwrap();
    let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    let sibling = leaf(
        &epoch.header.commitment,
        1,
        f.maker_b.pubkey(),
        200,
        [0, 99, 100, 0, 1, 0],
    );
    let before = snapshot(&mut context, &f).await;
    assert!(tx(
        &mut context,
        claim_bid(&f, 0, sibling, f.destination_a),
        &[&f.maker_a]
    )
    .await
    .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
}

#[tokio::test]
async fn bid_epoch_rejects_overfill_slippage_and_wrong_mint_without_state_changes() {
    let mut f = fixture_bid(false);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let before = snapshot(&mut context, &f).await;
    assert!(tx(&mut context, seal_bid(&f, payer, 301, 1), &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
    assert!(tx(&mut context, seal_bid(&f, payer, 150, 151), &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);

    let mut wrong_mint = seal_bid(&f, payer, 150, 150);
    wrong_mint.accounts[11] = AccountMeta::new_readonly(f.base_mint, false);
    assert!(tx(&mut context, wrong_mint, &[&f.taker]).await.is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);

    let mut forged_maker_balance = seal_bid(&f, payer, 150, 150);
    forged_maker_balance.accounts[17] = AccountMeta::new(f.balance_b, false);
    assert!(tx(&mut context, forged_maker_balance, &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);

    let mut readonly_maker_balance = seal_bid(&f, payer, 150, 150);
    readonly_maker_balance.accounts[17] = AccountMeta::new_readonly(f.balance_a, false);
    assert!(tx(&mut context, readonly_maker_balance, &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);

    let mut wrong_sidecar = seal_bid(&f, payer, 150, 150);
    wrong_sidecar.accounts[5] = AccountMeta::new_readonly(f.maker_a.pubkey(), false);
    assert!(tx(&mut context, wrong_sidecar, &[&f.taker]).await.is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
}

#[tokio::test]
async fn fractional_bid_full_fill_refunds_surplus_and_claims_do_not_strand_quote() {
    let mut f = fixture_bid_fractional(3);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    tx(&mut context, seal_bid(&f, payer, 6, 3), &[&f.taker])
        .await
        .unwrap();

    let epoch = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    assert_eq!(epoch.header.totals, [0, 6, 3, 0, 0, 0]);
    let balance_a = MakerBalance::decode_from(&bytes(&mut context, f.balance_a).await).unwrap();
    let balance_b = MakerBalance::decode_from(&bytes(&mut context, f.balance_b).await).unwrap();
    assert_eq!((balance_a.locked_quote, balance_a.free_quote), (1, 1));
    assert_eq!((balance_b.locked_quote, balance_b.free_quote), (2, 0));
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 1);

    let claim_a = [0, 3, 1, 0, 0, 0];
    let claim_b = [0, 3, 2, 0, 0, 0];
    let leaf_a = leaf(&epoch.header.commitment, 0, f.maker_a.pubkey(), 3, claim_a);
    let leaf_b = leaf(&epoch.header.commitment, 1, f.maker_b.pubkey(), 3, claim_b);
    assert_eq!(
        epoch.header.claim_root,
        hashv(&[b"HYBRID_MAKER_PUBKEY_NODE_V1\0", &leaf_a, &leaf_b]).to_bytes()
    );

    let mut first = claim_bid(&f, 0, leaf_b, f.destination_a);
    first.data[37..45].copy_from_slice(&3u64.to_le_bytes());
    for (index, amount) in claim_a.iter().enumerate() {
        first.data[45 + index * 8..53 + index * 8].copy_from_slice(&amount.to_le_bytes());
    }
    tx(&mut context, first, &[&f.maker_a]).await.unwrap();

    let mut second = claim_bid(&f, 1, leaf_a, f.destination_b);
    second.data[37..45].copy_from_slice(&3u64.to_le_bytes());
    for (index, amount) in claim_b.iter().enumerate() {
        second.data[45 + index * 8..53 + index * 8].copy_from_slice(&amount.to_le_bytes());
    }
    tx(&mut context, second, &[&f.maker_b]).await.unwrap();
    let balance_a = MakerBalance::decode_from(&bytes(&mut context, f.balance_a).await).unwrap();
    let balance_b = MakerBalance::decode_from(&bytes(&mut context, f.balance_b).await).unwrap();
    assert_eq!((balance_a.locked_quote, balance_a.free_quote), (0, 1));
    assert_eq!((balance_b.locked_quote, balance_b.free_quote), (0, 0));
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.vault_floor(0, 1), Ok((0, 1)));
}

#[tokio::test]
async fn fractional_bid_partial_fill_cannot_spend_still_locked_quote() {
    let mut f = fixture_bid_fractional(2);
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let before = snapshot(&mut context, &f).await;
    // Each 2-base order locks one quote. Filling 1 base from the second
    // order cannot release quote while its remaining 1-base order still
    // needs the full original 1 quote atom.
    assert!(tx(&mut context, seal_bid(&f, payer, 1, 1), &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, before);
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

    // Solana combines account privileges across a transaction. A claim may
    // therefore see market/custody as writable when paired with deposit or
    // order placement, even though claim itself never changes those accounts.
    let mut claim_a = claim(&f, 0, b, f.destination_a);
    claim_a.accounts[0].is_writable = true; // market
    claim_a.accounts[4].is_writable = true; // custody
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

fn fixture_ask_two_linked_prices() -> (Fixture, Pubkey, Pubkey) {
    let mut f = fixture(false);
    let next_index = 1u32.to_le_bytes();
    let (tail_key, _) =
        Pubkey::find_program_address(&[b"ask-page", f.market.as_ref(), &next_index], &ID);
    let (tail_owner_key, _) =
        Pubkey::find_program_address(&[b"ask-owner-page", f.market.as_ref(), &next_index], &ID);

    let mut market = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    market.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    market.ask_count = 3;
    market.next_sequence = 4;
    market.reserved2 = f.owners.to_bytes();
    let mut market_bytes = vec![0; MARKET_HEADER_BYTES];
    market.encode_into(&mut market_bytes).unwrap();

    let mut head = AskPage::default();
    let mut head_owners = AskOwnerPage::default();
    head.set_links(PageLinks::new(0, None, Some(1)));
    head_owners.set_links(PageLinks::new(0, None, Some(1)));
    for (index, (maker, qty)) in [(f.maker_a.pubkey(), 100u64), (f.maker_b.pubkey(), 200u64)]
        .into_iter()
        .enumerate()
    {
        head.insert(AskEntry {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: qty,
            sequence: index as u64 + 1,
        })
        .unwrap();
        head_owners.owners[index] = maker.to_bytes();
    }
    head_owners.len = 2;
    let mut head_bytes = vec![0; ASK_PAGE_BYTES];
    head.encode_into(&mut head_bytes).unwrap();
    let mut head_owner_bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    head_owners.encode_into(&mut head_owner_bytes).unwrap();

    let mut tail = AskPage::default();
    let mut tail_owners = AskOwnerPage::default();
    tail.set_links(PageLinks::new(1, Some(0), None));
    tail_owners.set_links(PageLinks::new(1, Some(0), None));
    tail.insert(AskEntry {
        price_x64: Q64 * 4,
        sqrt_price_x64: Q64 * 2,
        base_qty: 100,
        sequence: 3,
    })
    .unwrap();
    tail_owners.owners[0] = Pubkey::new_unique().to_bytes();
    tail_owners.len = 1;
    let mut tail_bytes = vec![0; ASK_PAGE_BYTES];
    tail.encode_into(&mut tail_bytes).unwrap();
    let mut tail_owner_bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    tail_owners.encode_into(&mut tail_owner_bytes).unwrap();
    let test = f.test.as_mut().unwrap();
    test.add_account(f.market, account(market_bytes, ID));
    test.add_account(f.ask, account(head_bytes, ID));
    test.add_account(f.owners, account(head_owner_bytes, ID));
    test.add_account(tail_key, account(tail_bytes, ID));
    test.add_account(tail_owner_key, account(tail_owner_bytes, ID));
    (f, tail_key, tail_owner_key)
}

fn fixture_funded_multipage_ask() -> (Fixture, Pubkey, Pubkey, Pubkey) {
    let (mut f, tail, tail_owner) = fixture_ask_two_linked_prices();
    let maker = Keypair::new();
    let (balance, bump) = Pubkey::find_program_address(
        &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
        &ID,
    );
    let destination = Pubkey::new_unique();
    let test = f.test.as_mut().unwrap();
    let mut state = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
    state.locked_base = 100;
    let mut balance_data = vec![0; MAKER_BALANCE_BYTES];
    state.encode_into(&mut balance_data).unwrap();
    test.add_account(balance, account(balance_data, ID));
    test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
    test.add_account(
        destination,
        account(token_data(f.quote_mint, maker.pubkey(), 0, false), TOKEN),
    );
    let mut sidecar = AskOwnerPage::default();
    sidecar.set_links(PageLinks::new(1, Some(0), None));
    sidecar.owners[0] = maker.pubkey().to_bytes();
    sidecar.len = 1;
    let mut bytes = vec![0; ASK_OWNER_PAGE_BYTES];
    sidecar.encode_into(&mut bytes).unwrap();
    test.add_account(tail_owner, account(bytes, ID));
    // Add the tail order's 100-base resting collateral to both custody
    // accounting and the actual SPL base vault.
    let (custody_key, custody_bump) =
        Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    assert_eq!(custody_key, f.custody);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump: custody_bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 400,
        total_quote: 0,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut custody_data).unwrap();
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.base_vault,
        account(
            token_data(f.base_mint, f.vault_authority, 400, false),
            TOKEN,
        ),
    );
    test.add_account(
        f.taker_quote,
        account(
            token_data(f.quote_mint, f.taker.pubkey(), 800, false),
            TOKEN,
        ),
    );
    f.extra_makers.push(maker);
    f.extra_balances.push(balance);
    f.extra_destinations.push(destination);
    (f, tail, tail_owner, balance)
}

// Opcode 45 uses opcode 44's account order with readonly maker balances.
fn seal_multipage_ask(
    f: &Fixture,
    payer: Pubkey,
    tail: Pubkey,
    tail_owner: Pubkey,
    epoch_one: Pubkey,
    base: u64,
    max_quote: u64,
) -> Instruction {
    let mut ix = seal_multipage_bid(f, payer, tail, tail_owner, epoch_one, base, max_quote);
    ix.data[0] = 45;
    for meta in &mut ix.accounts[20..] {
        meta.is_writable = false;
    }
    ix
}

#[tokio::test]
async fn funded_multipage_ask_seals_two_ticks_and_pays_all_claims() {
    exercise_multipage_ask(false).await;
}

#[tokio::test]
async fn multipage_ask_late_cpi_failure_is_atomic() {
    exercise_multipage_ask(true).await;
}

async fn exercise_multipage_ask(freeze_destination: bool) {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_ask();
    if freeze_destination {
        f.test.as_mut().unwrap().add_account(
            f.taker_base,
            account(token_data(f.base_mint, f.taker.pubkey(), 0, true), TOKEN),
        );
    }
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let ix = seal_multipage_ask(&f, payer, tail, tail_owner, epoch_one, 350, 505);
    let observed: Vec<_> = ix
        .accounts
        .iter()
        .filter(|meta| meta.pubkey != payer && (meta.is_writable || !meta.is_signer))
        .map(|meta| meta.pubkey)
        .collect();
    let mut before = Vec::new();
    for key in &observed {
        before.push(context.banks_client.get_account(*key).await.unwrap());
    }
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    for case in 0..4 {
        let mut bad = ix.clone();
        match case {
            0 => bad.accounts.swap(20, 22),
            1 => bad.accounts[8] = bad.accounts[6].clone(),
            2 => bad.data[9..17].copy_from_slice(&504u64.to_le_bytes()),
            _ => bad.accounts[4] = AccountMeta::new(Pubkey::new_unique(), false),
        }
        let blockhash = context.get_new_latest_blockhash().await.unwrap();
        let signed = Transaction::new_signed_with_payer(
            &[budget.clone(), bad],
            Some(&payer),
            &[&context.payer, &f.taker],
            blockhash,
        );
        assert!(context
            .banks_client
            .process_transaction(signed)
            .await
            .is_err());
        for (key, expected) in observed.iter().zip(&before) {
            assert_eq!(
                context.banks_client.get_account(*key).await.unwrap(),
                *expected,
            );
        }
    }
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let signed = Transaction::new_signed_with_payer(
        &[budget, ix],
        Some(&payer),
        &[&context.payer, &f.taker],
        blockhash,
    );
    let wire_bytes = bincode::serialize(&signed).unwrap().len();
    assert!(wire_bytes <= 1_232);
    let outcome = context
        .banks_client
        .process_transaction_with_metadata(signed)
        .await
        .unwrap();
    if freeze_destination {
        let error = outcome.result.unwrap_err();
        assert_eq!(format!("{error:?}"), "InstructionError(1, Custom(17))");
        let logs = outcome.metadata.unwrap().log_messages;
        let success = format!("Program {TOKEN} success");
        let failure = format!("Program {TOKEN} failed: custom program error: 0x11");
        let first = logs.iter().position(|line| line == &success).unwrap();
        assert!(logs[first + 1..].iter().any(|line| line == &failure));
        for (key, expected) in observed.iter().zip(&before) {
            assert_eq!(
                context.banks_client.get_account(*key).await.unwrap(),
                *expected,
            );
        }
        return;
    }
    outcome.result.unwrap();
    let units = outcome.metadata.unwrap().compute_units_consumed;
    eprintln!("opcode 45 three-maker: {wire_bytes} bytes, {units} CU");
    assert!(units > 0 && units <= 1_400_000);
    let first = FrozenEpochView::decode(&bytes(&mut context, f.epoch).await).unwrap();
    let second = FrozenEpochView::decode(&bytes(&mut context, epoch_one).await).unwrap();
    assert_eq!(first.header.totals, [300, 0, 0, 300, 0, 3]);
    assert_eq!(second.header.totals, [50, 0, 0, 200, 0, 2]);
    assert_eq!(first.header.side, 0);
    assert_eq!(second.header.side, 0);
    assert_eq!(first.header.epoch_id, 0);
    assert_eq!(second.header.epoch_id, 1);
    assert_ne!(first.header.commitment, second.header.commitment);
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 50);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 505);
    assert_eq!(amount(&bytes(&mut context, f.taker_base).await), 350);
    assert_eq!(amount(&bytes(&mut context, f.taker_quote).await), 295);
    let head = AskPage::decode_from(&bytes(&mut context, f.ask).await).unwrap();
    assert_eq!(head.len(), 1);
    assert_eq!(head.as_slice()[0].base_qty, 50);
    assert_eq!(head.as_slice()[0].sequence, 3);
    let sidecar = AskOwnerPage::decode_from(&bytes(&mut context, f.owners).await).unwrap();
    assert_eq!(sidecar.as_slice()[0], f.extra_makers[0].pubkey().to_bytes());
    let tail = AskPage::decode_from(&bytes(&mut context, tail).await).unwrap();
    assert!(tail.is_empty());

    let a = [100, 0, 0, 100, 0, 1];
    let b = [200, 0, 0, 200, 0, 2];
    let low_a = leaf(&first.header.commitment, 0, f.maker_a.pubkey(), 100, a);
    let low_b = leaf(&first.header.commitment, 1, f.maker_b.pubkey(), 200, b);
    assert_eq!(
        first.header.claim_root,
        hashv(&[b"HYBRID_MAKER_PUBKEY_NODE_V1\0", &low_a, &low_b]).to_bytes()
    );
    let high = [50, 0, 0, 200, 0, 2];
    assert_eq!(
        second.header.claim_root,
        leaf(
            &second.header.commitment,
            0,
            f.extra_makers[0].pubkey(),
            100,
            high,
        ),
    );
    for (epoch, maker, balance, destination, seat, weight, amounts, proof) in [
        (
            epoch_one,
            &f.extra_makers[0],
            f.extra_balances[0],
            f.extra_destinations[0],
            0,
            100,
            high,
            vec![],
        ),
        (
            f.epoch,
            &f.maker_b,
            f.balance_b,
            f.destination_b,
            1,
            200,
            b,
            vec![low_a],
        ),
        (
            f.epoch,
            &f.maker_a,
            f.balance_a,
            f.destination_a,
            0,
            100,
            a,
            vec![low_b],
        ),
    ] {
        let mut claim = multipage_bid_claim(
            &f,
            epoch,
            maker.pubkey(),
            balance,
            destination,
            seat,
            weight,
            amounts,
            &proof,
        );
        claim.accounts[6] = AccountMeta::new(f.quote_vault, false);
        claim.accounts[8] = AccountMeta::new_readonly(f.quote_mint, false);
        tx(&mut context, claim, &[maker]).await.unwrap();
        assert_eq!(amount(&bytes(&mut context, destination).await), amounts[3]);
    }
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 2);
    assert_eq!(reserve.maker_base_debit, 0);
    assert_eq!(reserve.maker_quote_credit, 0);
    assert_eq!(reserve.vault_floor(50, 0), Ok((50, 5)));
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 5);
    for key in [f.epoch, epoch_one] {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, key).await).unwrap();
        assert_eq!(epoch.claimed_count, epoch.header.maker_count);
        assert_eq!(epoch.remaining, [0; 6]);
    }
}

#[tokio::test]
async fn funded_eight_maker_multipage_ask_executes_and_claims() {
    let (mut f, tail, tail_owner, _) = fixture_funded_multipage_ask();
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.ask_count = 8;
    header.next_sequence = 9;
    header.reserved2 = f.owners.to_bytes();
    let mut market_data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut market_data).unwrap();

    let test = f.test.as_mut().unwrap();
    // Preserve the original three maker balances; add five higher-tick makers.
    for _ in 0..5 {
        let maker = Keypair::new();
        let (key, bump) = Pubkey::find_program_address(
            &[b"maker-balance", f.market.as_ref(), maker.pubkey().as_ref()],
            &ID,
        );
        let mut balance = MakerBalance::new(bump, f.market.to_bytes(), maker.pubkey().to_bytes());
        balance.locked_base = 100;
        let mut data = vec![0; MAKER_BALANCE_BYTES];
        balance.encode_into(&mut data).unwrap();
        test.add_account(key, account(data, ID));
        let destination = Pubkey::new_unique();
        test.add_account(maker.pubkey(), account(Vec::new(), Pubkey::default()));
        test.add_account(
            destination,
            account(token_data(f.quote_mint, maker.pubkey(), 0, false), TOKEN),
        );
        f.extra_destinations.push(destination);
        f.extra_makers.push(maker);
        f.extra_balances.push(key);
    }
    let mut page = AskPage::default();
    let mut owners = AskOwnerPage::default();
    page.set_links(PageLinks::new(1, Some(0), None));
    owners.set_links(PageLinks::new(1, Some(0), None));
    for (index, maker) in f.extra_makers.iter().enumerate() {
        page.insert(AskEntry {
            price_x64: Q64 * 4,
            sqrt_price_x64: Q64 * 2,
            base_qty: 100,
            sequence: index as u64 + 3,
        })
        .unwrap();
        owners.owners[index] = maker.pubkey().to_bytes();
    }
    owners.len = 6;
    let mut page_data = vec![0; ASK_PAGE_BYTES];
    page.encode_into(&mut page_data).unwrap();
    let mut owner_data = vec![0; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut owner_data).unwrap();
    let (_, bump) = Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 900,
        total_quote: 0,
    };
    let mut custody_data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut custody_data).unwrap();
    test.add_account(f.market, account(market_data, ID));
    test.add_account(tail, account(page_data, ID));
    test.add_account(tail_owner, account(owner_data, ID));
    test.add_account(f.custody, account(custody_data, ID));
    test.add_account(
        f.base_vault,
        account(
            token_data(f.base_mint, f.vault_authority, 900, false),
            TOKEN,
        ),
    );
    test.add_account(
        f.taker_quote,
        account(
            token_data(f.quote_mint, f.taker.pubkey(), 2727, false),
            TOKEN,
        ),
    );
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();
    let (epoch_one, _) = Pubkey::find_program_address(
        &[b"frozen-epoch", f.market.as_ref(), &1u64.to_le_bytes()],
        &ID,
    );
    let mut instruction = seal_multipage_ask(&f, payer, tail, tail_owner, epoch_one, 900, 2727);
    instruction.accounts.extend(
        f.extra_balances[1..]
            .iter()
            .map(|key| AccountMeta::new_readonly(*key, false)),
    );
    assert_eq!(instruction.accounts.len(), 28);
    let mut data = vec![2u8];
    data.extend_from_slice(&1_400_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: solana_program::pubkey!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let signed = Transaction::new_signed_with_payer(
        &[budget, instruction],
        Some(&payer),
        &[&context.payer, &f.taker],
        blockhash,
    );
    let wire_bytes = bincode::serialize(&signed).unwrap().len();
    assert!(wire_bytes <= 1_232);
    let outcome = context
        .banks_client
        .process_transaction_with_metadata(signed)
        .await
        .unwrap();
    outcome.result.unwrap();
    let units = outcome.metadata.unwrap().compute_units_consumed;
    eprintln!("opcode 45 eight-maker: {wire_bytes} bytes, {units} CU");
    assert!(units > 0 && units <= 1_400_000);
    for (epoch_key, makers, balances, destinations, weights, claims) in [
        (
            f.epoch,
            vec![&f.maker_a, &f.maker_b],
            vec![f.balance_a, f.balance_b],
            vec![f.destination_a, f.destination_b],
            vec![100u64, 200],
            vec![[100, 0, 0, 100, 0, 1], [200, 0, 0, 200, 0, 2]],
        ),
        (
            epoch_one,
            f.extra_makers.iter().collect(),
            f.extra_balances.clone(),
            f.extra_destinations.clone(),
            vec![100u64; 6],
            vec![[100, 0, 0, 400, 0, 4]; 6],
        ),
    ] {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, epoch_key).await).unwrap();
        assert_eq!(epoch.header.side, 0);
        assert_eq!(epoch.header.maker_count as usize, makers.len());
        let mut levels = vec![makers
            .iter()
            .enumerate()
            .map(|(seat, maker)| {
                leaf(
                    &epoch.header.commitment,
                    seat as u32,
                    maker.pubkey(),
                    weights[seat],
                    claims[seat],
                )
            })
            .collect::<Vec<_>>()];
        while levels.last().unwrap().len() > 1 {
            let next = levels
                .last()
                .unwrap()
                .chunks(2)
                .map(|chunk| {
                    hashv(&[
                        b"HYBRID_MAKER_PUBKEY_NODE_V1\0",
                        &chunk[0],
                        chunk.get(1).unwrap_or(&chunk[0]),
                    ])
                    .to_bytes()
                })
                .collect();
            levels.push(next);
        }
        assert_eq!(epoch.header.claim_root, levels.last().unwrap()[0]);
        for seat in (0..makers.len()).rev() {
            let mut proof = Vec::new();
            let mut index = seat;
            for level in levels.iter().take(levels.len() - 1) {
                proof.push(*level.get(index ^ 1).unwrap_or(&level[index]));
                index /= 2;
            }
            let mut claim = multipage_bid_claim(
                &f,
                epoch_key,
                makers[seat].pubkey(),
                balances[seat],
                destinations[seat],
                seat as u32,
                weights[seat],
                claims[seat],
                &proof,
            );
            claim.accounts[6] = AccountMeta::new(f.quote_vault, false);
            claim.accounts[8] = AccountMeta::new_readonly(f.quote_mint, false);
            tx(&mut context, claim, &[makers[seat]]).await.unwrap();
            assert_eq!(
                amount(&bytes(&mut context, destinations[seat]).await),
                claims[seat][3]
            );
            let balance =
                MakerBalance::decode_from(&bytes(&mut context, balances[seat]).await).unwrap();
            assert_eq!(balance.locked_base, 0);
        }
        let exhausted = FrozenEpochView::decode(&bytes(&mut context, epoch_key).await).unwrap();
        assert_eq!(exhausted.claimed_count, exhausted.header.maker_count);
        assert_eq!(exhausted.remaining, [0; 6]);
    }
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 2);
    assert_eq!(reserve.maker_base_debit, 0);
    assert_eq!(reserve.maker_quote_credit, 0);
    assert_eq!(reserve.vault_floor(0, 0), Ok((0, 27)));
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 0);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 27);
    assert_eq!(amount(&bytes(&mut context, f.taker_base).await), 900);
    assert_eq!(amount(&bytes(&mut context, f.taker_quote).await), 0);
}

#[tokio::test]
async fn three_interleaved_ask_bid_ask_epochs_reconcile_shared_vaults() {
    let mut f = fixture(false);
    let (bid_page, _) = Pubkey::find_program_address(
        &[b"bid-page", f.market.as_ref(), &0u32.to_le_bytes()],
        &ID,
    );
    let bid_owners = Pubkey::new_unique();
    let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
    header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
    header.ask_count = 2;
    header.set_bid_count(2);
    header.next_sequence = 5;
    header.reserved2 = f.owners.to_bytes();
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&bid_owners.to_bytes()[..16]);
    header.set_bid_owner_tag(tag);
    let mut data = vec![0; MARKET_HEADER_BYTES];
    header.encode_into(&mut data).unwrap();
    let test = f.test.as_mut().unwrap();
    test.add_account(f.market, account(data, ID));
    let mut bids = BidPage::default();
    let mut owners = AskOwnerPage::default();
    let bid_destinations = [Pubkey::new_unique(), Pubkey::new_unique()];
    for (index, (maker, balance, qty)) in [
        (f.maker_a.pubkey(), f.balance_a, 100u64),
        (f.maker_b.pubkey(), f.balance_b, 200u64),
    ]
    .into_iter()
    .enumerate()
    {
        bids.insert(AskEntry {
            price_x64: Q64,
            sqrt_price_x64: Q64,
            base_qty: qty,
            sequence: index as u64 + 3,
        })
        .unwrap();
        owners.owners[index] = maker.to_bytes();
        let (_, bump) = Pubkey::find_program_address(
            &[b"maker-balance", f.market.as_ref(), maker.as_ref()],
            &ID,
        );
        let mut state = MakerBalance::new(bump, f.market.to_bytes(), maker.to_bytes());
        state.locked_base = qty;
        state.locked_quote = qty;
        let mut data = vec![0; MAKER_BALANCE_BYTES];
        state.encode_into(&mut data).unwrap();
        test.add_account(balance, account(data, ID));
        test.add_account(
            bid_destinations[index],
            account(token_data(f.base_mint, maker, 0, false), TOKEN),
        );
    }
    owners.len = 2;
    let mut data = vec![0; BID_PAGE_BYTES];
    bids.encode_into(&mut data).unwrap();
    test.add_account(bid_page, account(data, ID));
    let mut data = vec![0; ASK_OWNER_PAGE_BYTES];
    owners.encode_into(&mut data).unwrap();
    test.add_account(bid_owners, account(data, ID));
    let (_, bump) = Pubkey::find_program_address(&[b"custody", f.market.as_ref()], &ID);
    let custody = CustodyState {
        magic: hybrid_state::CUSTODY_MAGIC,
        version: hybrid_state::STATE_VERSION,
        bump,
        base_decimals: 6,
        quote_decimals: 6,
        reserved: [0; 4],
        market: f.market.to_bytes(),
        base_mint: f.base_mint.to_bytes(),
        quote_mint: f.quote_mint.to_bytes(),
        base_vault: f.base_vault.to_bytes(),
        quote_vault: f.quote_vault.to_bytes(),
        total_base: 300,
        total_quote: 300,
    };
    let mut data = vec![0; CUSTODY_STATE_BYTES];
    custody.encode_into(&mut data).unwrap();
    test.add_account(f.custody, account(data, ID));
    test.add_account(
        f.quote_vault,
        account(
            token_data(f.quote_mint, f.vault_authority, 300, false),
            TOKEN,
        ),
    );
    let mut epochs = [f.epoch; 3];
    for (index, key) in epochs.iter_mut().enumerate().skip(1) {
        *key = Pubkey::find_program_address(
            &[b"frozen-epoch", f.market.as_ref(), &(index as u64).to_le_bytes()],
            &ID,
        )
        .0;
    }
    let mut context = f.test.take().unwrap().start_with_context().await;
    let payer = context.payer.pubkey();
    tx(&mut context, initialize(&f, payer), &[&f.market_signer])
        .await
        .unwrap();

    // Seal ASK 0, BID 1, ASK 2 against the same reserve, with no claims
    // until all three independent on-chain roots have been created.
    tx(&mut context, seal(&f, payer, 150, 151), &[&f.taker])
        .await
        .unwrap();
    let mut bid = seal_bid(&f, payer, 150, 150);
    bid.accounts[3] = AccountMeta::new(epochs[1], false);
    bid.accounts[4] = AccountMeta::new(bid_page, false);
    bid.accounts[5] = AccountMeta::new(bid_owners, false);
    tx(&mut context, bid, &[&f.taker]).await.unwrap();
    // A stale epoch ID must be rejected while executable liquidity remains.
    let stale_before = snapshot(&mut context, &f).await;
    assert!(tx(&mut context, seal(&f, payer, 150, 151), &[&f.taker])
        .await
        .is_err());
    assert_eq!(snapshot(&mut context, &f).await, stale_before);
    let mut ask = seal(&f, payer, 150, 151);
    ask.accounts[3] = AccountMeta::new(epochs[2], false);
    tx(&mut context, ask, &[&f.taker]).await.unwrap();

    let mut observed = vec![
        f.market,
        f.custody,
        f.reserve,
        f.ask,
        f.owners,
        bid_page,
        bid_owners,
        f.balance_a,
        f.balance_b,
        f.base_vault,
        f.quote_vault,
        f.taker_base,
        f.taker_quote,
        f.destination_a,
        f.destination_b,
    ];
    observed.extend(epochs);
    observed.extend(bid_destinations);
    let mut before = Vec::new();
    for key in &observed {
        before.push(context.banks_client.get_account(*key).await.unwrap());
    }
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 3);
    assert_eq!(reserve.maker_base_debit, 300);
    assert_eq!(reserve.maker_quote_credit, 300);
    assert_eq!(reserve.maker_base_credit, 149);
    assert_eq!(reserve.maker_quote_debit, 150);
    assert_eq!(reserve.vault_floor(0, 150), Ok((150, 452)));
    assert_eq!(amount(&bytes(&mut context, f.base_vault).await), 150);
    assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), 452);

    let mut claims = Vec::new();
    let mut roots = Vec::new();
    for (index, epoch_key) in epochs.iter().enumerate() {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, *epoch_key).await).unwrap();
        assert_eq!(epoch.header.epoch_id, index as u64);
        assert_eq!(epoch.header.side, u8::from(index == 1));
        let (weights, amounts) = match index {
            0 => ([100, 200], [[50, 0, 0, 50, 0, 0], [100, 0, 0, 100, 0, 1]]),
            1 => ([100, 200], [[0, 50, 50, 0, 0, 0], [0, 99, 100, 0, 1, 0]]),
            _ => ([50, 100], [[50, 0, 0, 50, 0, 0], [100, 0, 0, 100, 0, 1]]),
        };
        let makers = [f.maker_a.pubkey(), f.maker_b.pubkey()];
        let leaves = [
            leaf(&epoch.header.commitment, 0, makers[0], weights[0], amounts[0]),
            leaf(&epoch.header.commitment, 1, makers[1], weights[1], amounts[1]),
        ];
        let root =
            hashv(&[b"HYBRID_MAKER_PUBKEY_NODE_V1\0", &leaves[0], &leaves[1]]).to_bytes();
        assert_eq!(epoch.header.claim_root, root);
        assert!(!roots.contains(&root));
        roots.push(root);
        let destinations = if index == 1 {
            bid_destinations
        } else {
            [f.destination_a, f.destination_b]
        };
        for seat in 0..2 {
            let mut ix = multipage_bid_claim(
                &f,
                *epoch_key,
                makers[seat],
                [f.balance_a, f.balance_b][seat],
                destinations[seat],
                seat as u32,
                weights[seat],
                amounts[seat],
                &[leaves[seat ^ 1]],
            );
            if index != 1 {
                ix.accounts[6] = AccountMeta::new(f.quote_vault, false);
                ix.accounts[8] = AccountMeta::new_readonly(f.quote_mint, false);
            }
            claims.push(ix);
        }
    }
    // A valid epoch-0 ASK proof cannot be used against the later ASK epoch.
    let mut wrong = claims[0].clone();
    wrong.accounts[1] = AccountMeta::new(epochs[2], false);
    assert!(tx(&mut context, wrong, &[&f.maker_a]).await.is_err());
    // Nor can the same proof be replayed against an unrelated market.
    let mut wrong = claims[0].clone();
    wrong.accounts[0] = AccountMeta::new_readonly(Pubkey::new_unique(), false);
    assert!(tx(&mut context, wrong, &[&f.maker_a]).await.is_err());
    for (key, expected) in observed.iter().zip(&before) {
        assert_eq!(
            context.banks_client.get_account(*key).await.unwrap(),
            *expected,
        );
    }

    for index in [5usize, 2, 0, 3, 4, 1] {
        let maker = if index % 2 == 0 { &f.maker_a } else { &f.maker_b };
        tx(&mut context, claims[index].clone(), &[maker])
            .await
            .unwrap();
        let custody = CustodyState::decode_from(&bytes(&mut context, f.custody).await).unwrap();
        let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
        let floor = reserve
            .vault_floor(custody.total_base, custody.total_quote)
            .unwrap();
        assert_eq!(amount(&bytes(&mut context, f.base_vault).await), floor.0);
        assert_eq!(amount(&bytes(&mut context, f.quote_vault).await), floor.1);
        let mut snapshot = Vec::new();
        for key in &observed {
            snapshot.push(context.banks_client.get_account(*key).await.unwrap());
        }
        assert!(tx(&mut context, claims[index].clone(), &[maker]).await.is_err());
        for (key, expected) in observed.iter().zip(&snapshot) {
            assert_eq!(
            context.banks_client.get_account(*key).await.unwrap(),
            *expected,
        );
        }
    }
    for key in epochs {
        let epoch = FrozenEpochView::decode(&bytes(&mut context, key).await).unwrap();
        assert_eq!(epoch.claimed_count, 2);
        assert_eq!(epoch.remaining, [0; 6]);
    }
    let reserve = LazyReserve::decode(&bytes(&mut context, f.reserve).await).unwrap();
    assert_eq!(reserve.next_epoch_id, 3);
    assert_eq!(reserve.maker_base_debit, 0);
    assert_eq!(reserve.maker_quote_credit, 0);
    assert_eq!(reserve.maker_base_credit, 0);
    assert_eq!(reserve.maker_quote_debit, 0);
    assert_eq!(reserve.vault_floor(0, 150), Ok((1, 152)));
    assert_eq!(amount(&bytes(&mut context, f.destination_a).await), 100);
    assert_eq!(amount(&bytes(&mut context, f.destination_b).await), 200);
    assert_eq!(amount(&bytes(&mut context, bid_destinations[0]).await), 50);
    assert_eq!(amount(&bytes(&mut context, bid_destinations[1]).await), 99);
    assert_eq!(amount(&bytes(&mut context, f.taker_base).await), 150);
    assert_eq!(amount(&bytes(&mut context, f.taker_quote).await), 348);
}
