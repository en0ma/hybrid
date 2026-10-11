//! Host-only adapter boundary for an atomic getMultipleAccounts-style read.
//! Implementations must attach the response context slot to every account;
//! independently fetched account payloads must not be combined here.
use crate::client::{canonical_page, Side};
use crate::client_rpc::{ordered_accounts, MarketRpcSnapshot, RpcAccount, SnapshotError};
use solana_program::pubkey::Pubkey;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    RpcUnavailable,
    InvalidResponse(SnapshotError),
}

/// A transport must fetch all keys with one RPC response/context slot.
/// The legacy page-0 owner sidecar must be discovered from an authenticated
/// market header before this method is called.
pub trait AccountBatchRpc {
    fn read_batch(
        &mut self,
        keys: [Pubkey; 5],
    ) -> Result<(u64, [Option<RpcAccount>; 5]), TransportError>;
}

pub fn read_book<T: AccountBatchRpc>(
    transport: &mut T,
    market: Pubkey,
    legacy_head_owner: Pubkey,
    side: Side,
    min_slot: u64,
) -> Result<MarketRpcSnapshot, TransportError> {
    let head = canonical_page(market, side, 0);
    let tail = canonical_page(market, side, 1);
    let keys = [
        market,
        head.page,
        legacy_head_owner,
        tail.page,
        tail.owner,
    ];
    let (slot, accounts) = transport.read_batch(keys)?;
    let [market, head, head_owner, tail, tail_owner] =
        ordered_accounts(&keys, accounts, slot).map_err(TransportError::InvalidResponse)?;
    let result = MarketRpcSnapshot {
        slot,
        market,
        head,
        head_owner,
        tail,
        tail_owner,
        side,
    };
    result.verify(min_slot).map_err(TransportError::InvalidResponse)?;
    result
        .audit(min_slot)
        .map_err(TransportError::InvalidResponse)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRpc {
        slot: u64,
        missing: bool,
    }

    impl AccountBatchRpc for FakeRpc {
        fn read_batch(
            &mut self,
            keys: [Pubkey; 5],
        ) -> Result<(u64, [Option<RpcAccount>; 5]), TransportError> {
            let mut accounts = keys.map(|key| {
                Some(RpcAccount {
                    key,
                    owner: crate::ID,
                    data: vec![1],
                    slot: self.slot,
                })
            });
            if self.missing {
                accounts[3] = None;
            }
            Ok((self.slot, accounts))
        }
    }

    #[test]
    fn batch_read_rejects_missing_account_before_decode() {
        let mut rpc = FakeRpc {
            slot: 20,
            missing: true,
        };
        let result = read_book(
            &mut rpc,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Side::Ask,
            10,
        );
        assert!(matches!(
            result,
            Err(TransportError::InvalidResponse(
                SnapshotError::MissingAccount
            ))
        ));
    }

    #[test]
    fn batch_read_rejects_stale_context_before_decode() {
        let mut rpc = FakeRpc {
            slot: 20,
            missing: false,
        };
        let result = read_book(
            &mut rpc,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Side::Bid,
            21,
        );
        assert!(matches!(
            result,
            Err(TransportError::InvalidResponse(
                SnapshotError::StaleSnapshot
            ))
        ));
    }
}
