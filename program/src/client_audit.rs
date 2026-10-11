//! Offline audit of two-page market state and safe next-step planning.
//! All decisions require a fresh snapshot; transaction execution must revalidate.
use crate::client::{canonical_page, PageAccounts, Side, TwoPageBook};
use hybrid_state::{AskOwnerPage, AskPage, BidPage, MarketHeader, PageLinks};
use solana_program::pubkey::Pubkey;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditError {
    InvalidMarket,
    BadPage,
    BadSidecar,
    WrongAddress,
    WrongOwner,
    InconsistentLinks,
    InconsistentCount,
    DuplicateSequence,
    Unsorted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NextAction {
    /// Safe to attach an empty detached page, if a market signer exists.
    AttachEmpty,
    /// A currently linked tail is empty; can detach with market signature.
    DetachEmpty,
    /// Tail is detached and empty; can close and reclaim rent into the market.
    CloseDetached,
    /// Tail is linked and populated; orders can be cancelled or filled.
    CancelOrFill,
    /// Head has orders; may place orders on head, but cannot safely attach tail.
    HeadOrders,
    /// No operational transition is needed.
    None,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditReport {
    pub side: Side,
    pub head_orders: usize,
    pub tail_orders: usize,
    pub next_action: NextAction,
    pub is_linked: bool,
    /// True only for a detached, fully empty head and tail.
    pub can_attach: bool,
    pub total_orders: usize,
}
/// Copies of a decoded account view. Ownership and address are retained for
/// independent authentication; no RPC is performed by the auditing module.
pub struct AccountView<'a> {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data: &'a [u8],
}
pub struct TwoPageSnapshot<'a> {
    pub market: AccountView<'a>,
    pub head: AccountView<'a>,
    pub head_owner: AccountView<'a>,
    pub tail: AccountView<'a>,
    pub tail_owner: AccountView<'a>,
    pub side: Side,
}

fn sidecar_tag(key: Pubkey) -> [u8; 16] {
    let mut tag = [0; 16];
    tag.copy_from_slice(&key.to_bytes()[..16]);
    tag
}
impl<'a> TwoPageSnapshot<'a> {
    pub fn audit(&self) -> Result<AuditReport, AuditError> {
        let market_key = self.market.key;
        let expected_head = canonical_page(market_key, self.side, 0).page;
        let expected_tail = canonical_page(market_key, self.side, 1);
        if self.head.key != expected_head
            || self.tail.key != expected_tail.page
            || self.tail_owner.key != expected_tail.owner
        {
            return Err(AuditError::WrongAddress);
        }
        let accounts = [
            &self.market,
            &self.head,
            &self.head_owner,
            &self.tail,
            &self.tail_owner,
        ];
        if accounts.iter().any(|a| a.owner != crate::ID) {
            return Err(AuditError::WrongOwner);
        }
        for (i, a) in accounts.iter().enumerate() {
            if accounts[i + 1..].iter().any(|b| a.key == b.key) {
                return Err(AuditError::WrongAddress);
            }
        }
        let header =
            MarketHeader::decode_from(self.market.data).map_err(|_| AuditError::InvalidMarket)?;
        if !header.collateralized_active() {
            return Err(AuditError::InvalidMarket);
        }
        match self.side {
            Side::Ask if header.reserved2 != self.head_owner.key.to_bytes() => {
                return Err(AuditError::WrongAddress);
            }
            Side::Bid if header.bid_owner_tag() != sidecar_tag(self.head_owner.key) => {
                return Err(AuditError::WrongAddress);
            }
            _ => {}
        }
        let howner =
            AskOwnerPage::decode_from(self.head_owner.data).map_err(|_| AuditError::BadSidecar)?;
        let towner =
            AskOwnerPage::decode_from(self.tail_owner.data).map_err(|_| AuditError::BadSidecar)?;
        let (head_len, tail_len, head_links, tail_links, sequences, ordered) = match self.side {
            Side::Ask => {
                let h = AskPage::decode_from(self.head.data).map_err(|_| AuditError::BadPage)?;
                let t = AskPage::decode_from(self.tail.data).map_err(|_| AuditError::BadPage)?;
                howner
                    .validate_parallel(&h)
                    .map_err(|_| AuditError::BadSidecar)?;
                towner
                    .validate_parallel(&t)
                    .map_err(|_| AuditError::BadSidecar)?;
                let ordered = h.validate_order().is_ok() && t.validate_order().is_ok();
                let sequences = h
                    .as_slice()
                    .iter()
                    .chain(t.as_slice())
                    .map(|x| x.sequence)
                    .collect::<Vec<_>>();
                (
                    h.len(),
                    t.len(),
                    h.links(),
                    t.links(),
                    sequences,
                    ordered && (h.links().next_page != Some(1) || h.validate_next(&t).is_ok()),
                )
            }
            Side::Bid => {
                let h = BidPage::decode_from(self.head.data).map_err(|_| AuditError::BadPage)?;
                let t = BidPage::decode_from(self.tail.data).map_err(|_| AuditError::BadPage)?;
                if howner.len() != h.len()
                    || towner.len() != t.len()
                    || howner.links() != h.links()
                    || towner.links() != t.links()
                    || howner
                        .as_slice()
                        .iter()
                        .chain(towner.as_slice())
                        .any(|x| *x == [0u8; 32])
                {
                    return Err(AuditError::BadSidecar);
                }
                let ordered = h.validate_order().is_ok() && t.validate_order().is_ok();
                let sequences = h
                    .as_slice()
                    .iter()
                    .chain(t.as_slice())
                    .map(|x| x.sequence)
                    .collect::<Vec<_>>();
                (
                    h.len(),
                    t.len(),
                    h.links(),
                    t.links(),
                    sequences,
                    ordered && (h.links().next_page != Some(1) || h.validate_next(&t).is_ok()),
                )
            }
        };
        if !ordered {
            return Err(AuditError::Unsorted);
        }
        for (i, seq) in sequences.iter().enumerate() {
            if *seq == 0 || sequences[i + 1..].contains(seq) {
                return Err(AuditError::DuplicateSequence);
            }
        }
        // Maker placement uses this sequence directly; it must be strictly
        // newer than every existing order on either page.
        if header.next_sequence == 0 || sequences.iter().any(|seq| *seq >= header.next_sequence) {
            return Err(AuditError::DuplicateSequence);
        }
        let linked = head_links == PageLinks::new(0, None, Some(1))
            && tail_links == PageLinks::new(1, Some(0), None);
        let detached = head_links == PageLinks::new(0, None, None)
            && tail_links == PageLinks::new(1, None, None);
        if !linked && !detached {
            return Err(AuditError::InconsistentLinks);
        }
        if (match self.side {
            Side::Ask => header.ask_count,
            Side::Bid => header.bid_count(),
        }) as usize
            != head_len + tail_len
        {
            return Err(AuditError::InconsistentCount);
        }
        if detached && tail_len != 0 {
            return Err(AuditError::InconsistentLinks);
        }
        let next_action = if linked {
            if tail_len == 0 {
                NextAction::DetachEmpty
            } else {
                NextAction::CancelOrFill
            }
        } else {
            // A detached empty tail can always be closed; attachment is a
            // separate eligibility predicate because both are valid actions.
            NextAction::CloseDetached
        };
        Ok(AuditReport {
            side: self.side,
            head_orders: head_len,
            tail_orders: tail_len,
            next_action,
            is_linked: linked,
            can_attach: detached && head_len == 0,
            total_orders: head_len + tail_len,
        })
    }
    pub fn builder(&self) -> Result<TwoPageBook, AuditError> {
        self.audit()?;
        Ok(TwoPageBook::new(
            self.market.key,
            self.head_owner.key,
            self.side,
        ))
    }
    pub fn expected_addresses(&self) -> (PageAccounts, PageAccounts) {
        let mut head = canonical_page(self.market.key, self.side, 0);
        head.owner = self.head_owner.key;
        (head, canonical_page(self.market.key, self.side, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hybrid_engine::Q64;
    use hybrid_state::{
        AskEntry, ASK_OWNER_PAGE_BYTES, ASK_PAGE_BYTES, BID_PAGE_BYTES,
        MARKET_FLAG_COLLATERALIZED_ACTIVE, MARKET_HEADER_BYTES,
    };
    type FixtureAccounts = (Pubkey, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Pubkey);

    fn valid(side: Side, linked: bool, count: usize) -> FixtureAccounts {
        let market = Pubkey::new_unique();
        let head_owner = Pubkey::new_unique();
        let mut header = MarketHeader::new(1, Q64, 1_000_000, 1, 1);
        header.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
        header.next_sequence = count as u64 + 1;
        if side == Side::Ask {
            header.ask_count = count as u32;
            header.reserved2 = head_owner.to_bytes();
        } else {
            header.set_bid_count(count as u32);
            header.set_bid_owner_tag(sidecar_tag(head_owner));
        }
        let mut mh = vec![0; MARKET_HEADER_BYTES];
        header.encode_into(&mut mh).unwrap();
        let mut a = vec![
            0;
            if side == Side::Ask {
                ASK_PAGE_BYTES
            } else {
                BID_PAGE_BYTES
            }
        ];
        let mut b = a.clone();
        let mut ho = AskOwnerPage::default();
        let mut to = AskOwnerPage::default();
        let hlinks = PageLinks::new(0, None, linked.then_some(1));
        let tlinks = PageLinks::new(1, linked.then_some(0), None);
        ho.set_links(hlinks);
        to.set_links(tlinks);
        let mut oh = vec![0; ASK_OWNER_PAGE_BYTES];
        let mut ot = oh.clone();
        if side == Side::Ask {
            let mut h = AskPage::default();
            let mut t = AskPage::default();
            h.set_links(hlinks);
            t.set_links(tlinks);
            if count > 0 {
                h.insert(AskEntry {
                    price_x64: Q64,
                    sqrt_price_x64: Q64,
                    base_qty: 10,
                    sequence: 1,
                })
                .unwrap();
                ho.len = 1;
                ho.owners[0] = Pubkey::new_unique().to_bytes();
            }
            h.encode_into(&mut a).unwrap();
            t.encode_into(&mut b).unwrap();
        } else {
            let mut h = BidPage::default();
            let mut t = BidPage::default();
            h.set_links(hlinks);
            t.set_links(tlinks);
            if count > 0 {
                h.insert(AskEntry {
                    price_x64: Q64,
                    sqrt_price_x64: Q64,
                    base_qty: 10,
                    sequence: 1,
                })
                .unwrap();
                ho.len = 1;
                ho.owners[0] = Pubkey::new_unique().to_bytes();
            }
            h.encode_into(&mut a).unwrap();
            t.encode_into(&mut b).unwrap();
        }
        ho.encode_into(&mut oh).unwrap();
        to.encode_into(&mut ot).unwrap();
        (market, mh, a, b, oh, ot, head_owner)
    }
    // A test adapter preserves explicit keys and backing byte slices across lifetimes.
    #[allow(clippy::too_many_arguments)]
    fn test_snapshot<'a>(
        market: Pubkey,
        side: Side,
        head_owner: Pubkey,
        head_page: Pubkey,
        tail: PageAccounts,
        market_data: &'a [u8],
        head: &'a [u8],
        tail_data: &'a [u8],
        head_owner_data: &'a [u8],
        tail_owner_data: &'a [u8],
    ) -> TwoPageSnapshot<'a> {
        TwoPageSnapshot {
            market: AccountView {
                key: market,
                owner: crate::ID,
                data: market_data,
            },
            head: AccountView {
                key: head_page,
                owner: crate::ID,
                data: head,
            },
            head_owner: AccountView {
                key: head_owner,
                owner: crate::ID,
                data: head_owner_data,
            },
            tail: AccountView {
                key: tail.page,
                owner: crate::ID,
                data: tail_data,
            },
            tail_owner: AccountView {
                key: tail.owner,
                owner: crate::ID,
                data: tail_owner_data,
            },
            side,
        }
    }
    #[test]
    fn audit_detects_safe_link_transitions_and_tampering() {
        for side in [Side::Ask, Side::Bid] {
            for (linked, count, expected) in [
                (false, 0, NextAction::CloseDetached),
                (true, 0, NextAction::DetachEmpty),
                (true, 1, NextAction::DetachEmpty),
                (false, 1, NextAction::CloseDetached),
            ] {
                let (market, mut mh, h, t, ho, to, owner) = valid(side, linked, count);
                let hp = canonical_page(market, side, 0);
                let tp = canonical_page(market, side, 1);
                let result = test_snapshot(market, side, owner, hp.page, tp, &mh, &h, &t, &ho, &to)
                    .audit()
                    .unwrap();
                assert_eq!(result.next_action, expected);
                assert_eq!(result.total_orders, count);
                assert_eq!(result.can_attach, !linked && count == 0);
                assert_eq!(
                    test_snapshot(market, side, owner, hp.page, tp, &mh, &h, &t, &ho, &to)
                        .builder()
                        .unwrap()
                        .tail
                        .page,
                    tp.page
                );
                // Deliberately corrupt the next placement sequence without
                // changing any page: zero and reused IDs must fail audits.
                let mut bad_header = MarketHeader::decode_from(&mh).unwrap();
                bad_header.next_sequence = 0;
                let mut invalid = mh.clone();
                bad_header.encode_into(&mut invalid).unwrap();
                assert_eq!(
                    test_snapshot(market, side, owner, hp.page, tp, &invalid, &h, &t, &ho, &to)
                        .audit(),
                    Err(AuditError::DuplicateSequence)
                );
                if count > 0 {
                    bad_header.next_sequence = 1;
                    bad_header.encode_into(&mut invalid).unwrap();
                    assert_eq!(
                        test_snapshot(market, side, owner, hp.page, tp, &invalid, &h, &t, &ho, &to)
                            .audit(),
                        Err(AuditError::DuplicateSequence)
                    );
                }
                mh[12..16].copy_from_slice(&999u32.to_le_bytes());
                if side == Side::Ask {
                    assert_eq!(
                        test_snapshot(market, side, owner, hp.page, tp, &mh, &h, &t, &ho, &to)
                            .audit(),
                        Err(AuditError::InconsistentCount)
                    );
                }
            }
        }
    }
}
