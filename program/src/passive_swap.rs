//! Single-position wire adapter. Settlement and collateral checks are shared
//! with the multi-position engine.
use hybrid_state::passive::PositionAccount;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
    pubkey::Pubkey,
};

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 60 || accounts.len() != 14 || !matches!(data[0], 33 | 34) || data[1] > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let position = PositionAccount::decode_from(&accounts[3].try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if position.position.owner != accounts[4].key.to_bytes()
        || position.nonce.to_le_bytes() != data[20..28]
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let mut shared = [0u8; 53];
    shared[..20].copy_from_slice(&data[..20]);
    shared[0] += 2;
    shared[20] = 1;
    shared[21..].copy_from_slice(&data[28..]);
    let mapped: Vec<AccountInfo> = [0, 1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 3]
        .iter()
        .map(|&i| accounts[i].clone())
        .collect();
    crate::passive_multi::process(program_id, &mapped, &shared)
}
