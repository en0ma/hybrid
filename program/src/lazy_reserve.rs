use super::*;

/// Opcode 39: establish market-bound cross-epoch reserve accounting.
/// Market authority (original market keypair) must sign. This instruction
/// creates NO epoch, attests NO fill and moves NO tokens.
///
/// data = 39 | genesis-domain[32]
/// accounts = market, custody PDA, lazy-reserve PDA, payer, system program.
#[inline(never)]
pub(super) fn initialize_market_lazy_reserve(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 33 || data[0] != 39 || accounts.len() != 5 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let market = &accounts[0];
    let custody_account = &accounts[1];
    let reserve_account = &accounts[2];
    let payer = &accounts[3];
    let system = &accounts[4];
    if !market.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market.is_writable || custody_account.is_writable
        || !reserve_account.is_writable
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let market_header = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market_header.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    load_custody(program_id, market, custody_account)?;
    let (expected, bump) = Pubkey::find_program_address(
        &[b"lazy-reserve", market.key.as_ref()], program_id,
    );
    if *reserve_account.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut genesis = [0; 32];
    genesis.copy_from_slice(&data[1..33]);
    let state = hybrid_state::lazy_reserve::LazyReserve::new(
        bump, market.key.to_bytes(), genesis,
    ).map_err(|_| ProgramError::InvalidInstructionData)?;
    create_program_pda(
        program_id, payer, reserve_account, system,
        &[b"lazy-reserve", market.key.as_ref()], bump,
        hybrid_state::lazy_reserve::LAZY_RESERVE_BYTES,
    )?;
    let mut bytes = reserve_account.try_borrow_mut_data()?;
    state.encode(&mut bytes).map_err(|_| ProgramError::InvalidAccountData)
}

/// Strict program ownership, PDA and bump check. This is reused by the
/// future authenticated execution path and the claim payout instruction.
pub(super) fn load_lazy_reserve(
    program_id: &Pubkey,
    market: &AccountInfo,
    account: &AccountInfo,
) -> Result<hybrid_state::lazy_reserve::LazyReserve, ProgramError> {
    if account.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (expected, bump) = Pubkey::find_program_address(
        &[b"lazy-reserve", market.key.as_ref()], program_id,
    );
    if *account.key != expected {
        return Err(ProgramError::InvalidSeeds);
    }
    let state = hybrid_state::lazy_reserve::LazyReserve::decode(
        &account.try_borrow_data()?
    ).map_err(|_| ProgramError::InvalidAccountData)?;
    if state.market != market.key.to_bytes() || state.bump != bump {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(state)
}
