//! Host-only solvency checks across active maker obligations and passive LP reserves.
//! These tests exercise pure accounting. Real SPL vault transaction tests remain required.
use hybrid_state::passive::{PassiveAccountingError, PassivePool, PassivePosition};

fn position(owner: u8, base: u64, quote: u64) -> PassivePosition {
    PassivePosition {
        owner: [owner; 32],
        lower_sqrt_price_x64: 1,
        upper_sqrt_price_x64: 3,
        liquidity: 10,
        base_principal: base,
        quote_principal: quote,
    }
}

#[test]
fn active_and_passive_obligations_cannot_spend_each_others_reserves() {
    let mut pool = PassivePool::default();
    pool.deposit(&position(1, 300, 500)).unwrap();
    pool.deposit(&position(2, 200, 400)).unwrap();
    let active_base = 100u64;
    let active_quote = 250u64;
    let vault_base = 600u64;
    let vault_quote = 1150u64;
    assert_eq!(
        pool.verify_vault_coverage(active_base, active_quote, vault_base, vault_quote),
        Ok(())
    );
    assert_eq!(
        pool.verify_vault_coverage(active_base, active_quote, vault_base - 1, vault_quote),
        Err(PassiveAccountingError::InvariantViolation)
    );
    assert_eq!(
        pool.verify_vault_coverage(active_base, active_quote, vault_base, vault_quote - 1),
        Err(PassiveAccountingError::InvariantViolation)
    );

    // Pure reserve transitions: the simulated vault receives and releases the
    // exact input/output quantities, including fees retained in custody.
    let mut base = vault_base;
    let mut quote = vault_quote;
    pool.buy(120, 60, 3).unwrap();
    base -= 60;
    quote += 120;
    pool.verify_vault_coverage(active_base, active_quote, base, quote)
        .unwrap();
    pool.sell(90, 70, 2).unwrap();
    base += 90;
    quote -= 70;
    pool.verify_vault_coverage(active_base, active_quote, base, quote)
        .unwrap();

    // Fees are obligations, not surplus that can be withdrawn by active makers.
    assert_eq!(
        pool.verify_vault_coverage(active_base, active_quote, base - 1, quote),
        Err(PassiveAccountingError::InvariantViolation)
    );
    assert_eq!(
        pool.verify_vault_coverage(active_base, active_quote, base, quote - 1),
        Err(PassiveAccountingError::InvariantViolation)
    );
}

#[test]
fn insolvent_and_overflowed_coverage_rejects_without_state_change() {
    let mut pool = PassivePool::default();
    pool.deposit(&position(1, 100, 200)).unwrap();
    let prior = pool;
    assert_eq!(
        pool.verify_vault_coverage(1, 0, 100, 200),
        Err(PassiveAccountingError::InvariantViolation)
    );
    assert_eq!(
        pool.verify_vault_coverage(u64::MAX, 0, u64::MAX, 200),
        Err(PassiveAccountingError::Overflow)
    );
    assert_eq!(
        pool.verify_vault_coverage(0, u64::MAX, 100, u64::MAX),
        Err(PassiveAccountingError::Overflow)
    );
    assert_eq!(pool, prior);
}
