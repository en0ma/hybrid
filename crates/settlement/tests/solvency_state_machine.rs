//! Host-side model tests for the maker collateral conservation boundary.
//! This does not replace transaction-level SPL vault/owner/PDA checks.
use hybrid_settlement::{MakerBalance, SettlementError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Assets {
    base: u128,
    quote: u128,
}
fn liabilities(makers: &[MakerBalance]) -> Assets {
    makers.iter().fold(Assets::default(), |mut sum, maker| {
        sum.base += u128::from(maker.base_free) + u128::from(maker.base_locked);
        sum.quote += u128::from(maker.quote_free) + u128::from(maker.quote_locked);
        sum
    })
}
fn assert_solvent(makers: &[MakerBalance], vault: Assets) {
    assert_eq!(
        liabilities(makers),
        vault,
        "maker collateral must equal simulated vault funds"
    );
}
fn transfer_to_vault(vault: &mut Assets, base: u64, quote: u64) {
    vault.base += u128::from(base);
    vault.quote += u128::from(quote);
}
fn transfer_from_vault(vault: &mut Assets, base: u64, quote: u64) {
    vault.base -= u128::from(base);
    vault.quote -= u128::from(quote);
}

#[test]
fn two_sided_multi_maker_lifecycle_preserves_vault_coverage() {
    let mut makers = [MakerBalance::default(); 3];
    let mut vault = Assets::default();

    for (maker, (base, quote)) in makers
        .iter_mut()
        .zip([(1000, 3000), (2500, 1500), (800, 900)])
    {
        maker.base_free = base;
        maker.quote_free = quote;
        transfer_to_vault(&mut vault, base, quote);
    }
    assert_solvent(&makers, vault);

    // Maker 0 quotes an ask; maker 1 quotes a bid; maker 2 quotes both.
    makers[0].reserve_ask(700).unwrap();
    makers[1].reserve_bid(900).unwrap();
    makers[2].reserve_ask(500).unwrap();
    makers[2].reserve_bid(600).unwrap();
    assert_solvent(&makers, vault);

    // A taker buys from maker 0 and 2: base leaves, quote enters custody.
    makers[0].settle_ask_fill(200, 240).unwrap();
    makers[2].settle_ask_fill(300, 360).unwrap();
    transfer_from_vault(&mut vault, 500, 0);
    transfer_to_vault(&mut vault, 0, 600);
    assert_solvent(&makers, vault);

    // Another taker sells into bid orders of maker 1 and 2.
    makers[1].settle_bid_fill(300, 450).unwrap();
    makers[2].settle_bid_fill(100, 150).unwrap();
    transfer_to_vault(&mut vault, 400, 0);
    transfer_from_vault(&mut vault, 0, 600);
    assert_solvent(&makers, vault);

    // Cancel remaining orders: all locked funds return to free collateral.
    makers[0].release_ask(500).unwrap();
    makers[1].release_bid(450).unwrap();
    makers[2].release_ask(200).unwrap();
    makers[2].release_bid(450).unwrap();
    assert_solvent(&makers, vault);
    assert!(makers
        .iter()
        .all(|m| m.base_locked == 0 && m.quote_locked == 0));

    // Withdraw all free balances; no phantom collateral or dust remains.
    for maker in &mut makers {
        transfer_from_vault(&mut vault, maker.base_free, maker.quote_free);
        maker.base_free = 0;
        maker.quote_free = 0;
    }
    assert_solvent(&makers, vault);
    assert_eq!(vault, Assets::default());
}

type FailureCase = (fn(&mut MakerBalance) -> Result<(), SettlementError>, SettlementError);

#[test]
fn failures_do_not_mutate_collateral_or_simulated_vaults() {
    let mut maker = MakerBalance {
        base_free: 10,
        base_locked: 20,
        quote_free: 30,
        quote_locked: 40,
    };
    let vault = liabilities(&[maker]);
    let cases: &[FailureCase] = &[
        (|m| m.reserve_ask(11), SettlementError::InsufficientBase),
        (|m| m.reserve_bid(31), SettlementError::InsufficientQuote),
        (|m| m.release_ask(21), SettlementError::InsufficientBase),
        (|m| m.release_bid(41), SettlementError::InsufficientQuote),
        (
            |m| m.settle_ask_fill(21, 1),
            SettlementError::InsufficientBase,
        ),
        (
            |m| m.settle_bid_fill(1, 41),
            SettlementError::InsufficientQuote,
        ),
        (|m| m.settle_ask_fill(0, 1), SettlementError::InvalidFill),
        (|m| m.settle_bid_fill(1, 0), SettlementError::InvalidFill),
    ];
    for (transition, error) in cases {
        let before = maker;
        assert_eq!(transition(&mut maker), Err(*error));
        assert_eq!(maker, before);
        assert_solvent(&[maker], vault);
    }
}

#[test]
fn deterministic_order_churn_conserves_collateral() {
    let mut makers = [MakerBalance::default(); 4];
    let mut vault = Assets::default();
    for maker in &mut makers {
        maker.base_free = 10_000;
        maker.quote_free = 10_000;
        transfer_to_vault(&mut vault, 10_000, 10_000);
    }
    // LCG produces repeatable multi-maker, mixed-side state transitions.
    let mut seed = 0x8fb4_2123_u64;
    for step in 0..2048 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let i = (seed as usize >> 16) % makers.len();
        let amount = seed % 23 + 1;
        let maker = &mut makers[i];
        match (seed >> 24) & 7 {
            0 => {
                let _ = maker.reserve_ask(amount);
            }
            1 => {
                let _ = maker.reserve_bid(amount);
            }
            2 => {
                let _ = maker.release_ask(amount);
            }
            3 => {
                let _ = maker.release_bid(amount);
            }
            4 | 5 if maker.base_locked >= amount => {
                maker.settle_ask_fill(amount, amount).unwrap();
                transfer_from_vault(&mut vault, amount, 0);
                transfer_to_vault(&mut vault, 0, amount);
            }
            6 | 7 if maker.quote_locked >= amount => {
                maker.settle_bid_fill(amount, amount).unwrap();
                transfer_to_vault(&mut vault, amount, 0);
                transfer_from_vault(&mut vault, 0, amount);
            }
            _ => {}
        }
        assert_solvent(&makers, vault);
        if step % 128 == 0 {
            assert!(makers
                .iter()
                .all(|m| m.base_free.checked_add(m.base_locked).is_some()));
        }
    }
}

#[test]
fn overflow_does_not_commit_partial_balance_updates() {
    let mut maker = MakerBalance {
        base_free: 0,
        base_locked: 1,
        quote_free: u64::MAX,
        quote_locked: 0,
    };
    let original = maker;
    assert_eq!(maker.settle_ask_fill(1, 1), Err(SettlementError::Overflow));
    assert_eq!(maker, original);
    maker = MakerBalance {
        base_free: u64::MAX,
        base_locked: 0,
        quote_free: 0,
        quote_locked: 1,
    };
    let original = maker;
    assert_eq!(maker.settle_bid_fill(1, 1), Err(SettlementError::Overflow));
    assert_eq!(maker, original);
}
