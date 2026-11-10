//! Opcode 38: read-only maker claim preflight for the first Hybrid launch.
//!
//! This does NOT establish a trusted epoch root (no sealer instruction exists)
//! and does not authorize payouts, SPL CPI or claim-bit updates. A successful
//! preflight is never a withdraw capability. In particular, the supplied
//! genesis hash MUST be authenticated by the final deployment configuration;
//! a transaction-supplied genesis alone does not prove cluster identity.
//!
//! Accounts: [market, frozen epoch PDA, maker balance PDA, maker signer].
//! Instruction: 38 | genesis[32] | seat u32LE | weight u64LE |
//!              six claim amounts u64LE | proof depth u8 | siblings[32*N].
//! N must equal the deterministic Merkle height for maker_count.
use super::*;

const CLAIM_PREFIX_BYTES: usize = 1 + 32 + 4 + 8 + 6 * 8 + 1;
const MAX_MERKLE_DEPTH: usize = 12;

#[inline(never)]
pub(super) fn process_frozen_claim_preflight(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 4 || data.len() < CLAIM_PREFIX_BYTES || data[0] != 38 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [market, epoch_account, balance_account, maker] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if market.owner != program_id
        || epoch_account.owner != program_id
        || balance_account.owner != program_id
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    // The preflight cannot mutate any account, even when callers mark them writable.
    if epoch_account.is_writable || balance_account.is_writable || market.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if market.key == epoch_account.key
        || market.key == balance_account.key
        || epoch_account.key == balance_account.key
        || maker.key == epoch_account.key
        || maker.key == balance_account.key
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let market_state = hybrid_state::MarketHeader::decode_from(&market.try_borrow_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if !market_state.collateralized_active() {
        return Err(ProgramError::InvalidAccountData);
    }
    let epoch_data = epoch_account.try_borrow_data()?;
    let epoch = hybrid_state::frozen_epoch::FrozenEpochView::decode(&epoch_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if epoch.header.market != market.key.to_bytes() || epoch.header.program != program_id.to_bytes()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let epoch_id = epoch.header.epoch_id.to_le_bytes();
    let (expected, bump) = Pubkey::find_program_address(
        &[b"frozen-epoch", market.key.as_ref(), &epoch_id],
        program_id,
    );
    if *epoch_account.key != expected || epoch.header.bump != bump {
        return Err(ProgramError::InvalidSeeds);
    }
    let balance = load_maker_balance(program_id, market, balance_account, maker)?;
    let mut genesis = [0u8; 32];
    genesis.copy_from_slice(&data[1..33]);
    let seat = u32::from_le_bytes(
        data[33..37]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let weight = u64::from_le_bytes(
        data[37..45]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let mut amounts = [0u64; 6];
    for (i, amount) in amounts.iter_mut().enumerate() {
        *amount = u64::from_le_bytes(
            data[45 + 8 * i..53 + 8 * i]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        );
    }
    let depth = usize::from(data[93]);
    if depth > MAX_MERKLE_DEPTH || data.len() != CLAIM_PREFIX_BYTES + depth * 32 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut siblings = Vec::with_capacity(depth);
    for i in 0..depth {
        let offset = CLAIM_PREFIX_BYTES + i * 32;
        let mut sibling = [0u8; 32];
        sibling.copy_from_slice(&data[offset..offset + 32]);
        siblings.push(sibling);
    }
    // Use Solana's SHA-256 syscall rather than linking the host-side
    // sha2 verification crate into the executable; preserve byte-for-byte
    // parity with hybrid-settlement::frozen_claim_gate and the SDK.
    if epoch.header.genesis != genesis
        || balance.owner != maker.key.to_bytes()
        || balance.market != market.key.to_bytes()
        || seat >= epoch.header.maker_count
        || weight == 0
        || epoch.is_claimed(&epoch_data, seat).map_err(|_| ProgramError::InvalidAccountData)?
    {
        return Err(ProgramError::InvalidArgument);
    }
    if amounts.iter().zip(epoch.header.totals).any(|(amount, total)| *amount > total) {
        return Err(ProgramError::InvalidArgument);
    }
    let base = if epoch.header.side == 0 { amounts[0] } else { amounts[1] };
    if base == 0
        || (epoch.header.side == 0 && (amounts[1] != 0 || amounts[2] != 0 || amounts[4] != 0))
        || (epoch.header.side == 1 && (amounts[0] != 0 || amounts[3] != 0 || amounts[5] != 0))
    {
        return Err(ProgramError::InvalidArgument);
    }
    let maker_bytes = maker.key.to_bytes();
    let seat_bytes = seat.to_le_bytes();
    let weight_bytes = weight.to_le_bytes();
    let atom_bytes = amounts.map(u64::to_le_bytes);
    let mut hash = solana_program::hash::hashv(&[
        b"HYBRID_MAKER_PUBKEY_CLAIM_V1\\0",
        &epoch.header.commitment,
        &seat_bytes,
        &maker_bytes,
        &weight_bytes,
        &atom_bytes[0],
        &atom_bytes[1],
        &atom_bytes[2],
        &atom_bytes[3],
        &atom_bytes[4],
        &atom_bytes[5],
    ]).to_bytes();
    let mut node_count = epoch.header.maker_count;
    let mut index = seat;
    let mut used = 0usize;
    while node_count > 1 {
        let sibling = siblings.get(used).ok_or(ProgramError::InvalidInstructionData)?;
        if index.is_multiple_of(2) && index + 1 >= node_count && sibling != &hash {
            return Err(ProgramError::InvalidArgument);
        }
        hash = if index.is_multiple_of(2) {
            solana_program::hash::hashv(&[
                b"HYBRID_MAKER_PUBKEY_NODE_V1\\0",
                &hash,
                sibling,
            ]).to_bytes()
        } else {
            solana_program::hash::hashv(&[
                b"HYBRID_MAKER_PUBKEY_NODE_V1\\0",
                sibling,
                &hash,
            ]).to_bytes()
        };
        index /= 2;
        node_count = node_count.div_ceil(2);
        used += 1;
    }
    if used != siblings.len() || hash != epoch.header.claim_root {
        return Err(ProgramError::InvalidArgument);
    }
    Ok(())
}
