//! Host-only RPC snapshot verification and two-page audit bridge.
//! Transport adapters must use a single getMultipleAccounts response/context
//! (or independently prove identical slots), not unrelated RPC reads.
use crate::client::Side;
use crate::client_audit::{AccountView, AuditError, AuditReport, TwoPageSnapshot};
use solana_program::pubkey::Pubkey;

#[derive(Clone, Debug)]
pub struct RpcAccount {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data: Vec<u8>,
    pub slot: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    MissingAccount,
    DuplicateAccount,
    MixedSlots,
    StaleSnapshot,
    InvalidMarket(AuditError),
}

/// Host-side snapshot; returned accounts must retain their actual keys and owners.
/// Neither this API nor audit() authorizes any on-chain action.
pub struct MarketRpcSnapshot {
    pub slot: u64,
    pub market: RpcAccount,
    pub head: RpcAccount,
    pub head_owner: RpcAccount,
    pub tail: RpcAccount,
    pub tail_owner: RpcAccount,
    pub side: Side,
}

impl MarketRpcSnapshot {
    /// Validate a consistent set of five accounts and reject a caller-defined
    /// stale snapshot. A higher local slot is not a substitute for confirmation.
    pub fn verify(&self, min_slot: u64) -> Result<(), SnapshotError> {
        let accounts = [
            &self.market,
            &self.head,
            &self.head_owner,
            &self.tail,
            &self.tail_owner,
        ];
        if self.slot < min_slot {
            return Err(SnapshotError::StaleSnapshot);
        }
        if accounts.iter().any(|a| a.slot != self.slot) {
            return Err(SnapshotError::MixedSlots);
        }
        for (i, a) in accounts.iter().enumerate() {
            if accounts[i + 1..].iter().any(|b| a.key == b.key) {
                return Err(SnapshotError::DuplicateAccount);
            }
        }
        if accounts.iter().any(|a| a.data.is_empty()) {
            return Err(SnapshotError::MissingAccount);
        }
        Ok(())
    }

    pub fn audit(&self, min_slot: u64) -> Result<AuditReport, SnapshotError> {
        self.verify(min_slot)?;
        self.snapshot()
            .audit()
            .map_err(SnapshotError::InvalidMarket)
    }

    pub fn snapshot(&self) -> TwoPageSnapshot<'_> {
        fn view(a: &RpcAccount) -> AccountView<'_> {
            AccountView {
                key: a.key,
                owner: a.owner,
                data: &a.data,
            }
        }
        TwoPageSnapshot {
            market: view(&self.market),
            head: view(&self.head),
            head_owner: view(&self.head_owner),
            tail: view(&self.tail),
            tail_owner: view(&self.tail_owner),
            side: self.side,
        }
    }
}

/// Validate the RPC response order before constructing a MarketRpcSnapshot.
/// Missing accounts are not silently represented by empty byte buffers.
pub fn ordered_accounts(
    expected: &[Pubkey; 5],
    returned: [Option<RpcAccount>; 5],
    context_slot: u64,
) -> Result<[RpcAccount; 5], SnapshotError> {
    let [market, head, head_owner, tail, tail_owner] = returned;
    let accounts = [
        market.ok_or(SnapshotError::MissingAccount)?,
        head.ok_or(SnapshotError::MissingAccount)?,
        head_owner.ok_or(SnapshotError::MissingAccount)?,
        tail.ok_or(SnapshotError::MissingAccount)?,
        tail_owner.ok_or(SnapshotError::MissingAccount)?,
    ];
    for (i, a) in accounts.iter().enumerate() {
        if a.key != expected[i] {
            return Err(SnapshotError::MissingAccount);
        }
        if a.slot != context_slot {
            return Err(SnapshotError::MixedSlots);
        }
        if accounts[i + 1..].iter().any(|b| a.key == b.key) {
            return Err(SnapshotError::DuplicateAccount);
        }
    }
    Ok(accounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(key: Pubkey, slot: u64) -> RpcAccount {
        RpcAccount {
            key,
            owner: crate::ID,
            data: vec![1],
            slot,
        }
    }

    #[test]
    fn rpc_snapshot_rejects_stale_or_mixed_slots() {
        let keys = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let mut accounts = keys.map(|k| Some(account(k, 40)));
        let ordered = ordered_accounts(&keys, accounts.clone(), 40).unwrap();
        let [market, head, head_owner, tail, tail_owner] = ordered;
        let snapshot = MarketRpcSnapshot {
            slot: 40,
            market,
            head,
            head_owner,
            tail,
            tail_owner,
            side: Side::Ask,
        };
        assert_eq!(snapshot.verify(41), Err(SnapshotError::StaleSnapshot));
        assert_eq!(snapshot.verify(40), Ok(()));
        accounts[4].as_mut().unwrap().slot = 41;
        assert!(matches!(
            ordered_accounts(&keys, accounts, 40),
            Err(SnapshotError::MixedSlots)
        ));
    }

    #[test]
    fn rpc_snapshot_rejects_missing_reordered_and_aliased_accounts() {
        let keys = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let mut accounts = keys.map(|k| Some(account(k, 10)));
        accounts[1] = None;
        assert!(matches!(
            ordered_accounts(&keys, accounts.clone(), 10),
            Err(SnapshotError::MissingAccount)
        ));
        accounts[1] = Some(account(keys[0], 10));
        assert!(matches!(
            ordered_accounts(&keys, accounts.clone(), 10),
            Err(SnapshotError::DuplicateAccount)
        ));
        let duplicate = [keys[0], keys[0], keys[2], keys[3], keys[4]];
        let accounts = duplicate.map(|k| Some(account(k, 10)));
        assert!(matches!(
            ordered_accounts(&duplicate, accounts, 10),
            Err(SnapshotError::DuplicateAccount)
        ));
    }
}
