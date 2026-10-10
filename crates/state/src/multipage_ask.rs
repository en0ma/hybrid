use crate::{AskEntry, AskOwnerPage, AskPage, StateError, ASKS_PER_PAGE};

/// Atomically apply global-index active fills and repack canonical linked pages.
/// The caller writes the returned pages only after settlement checks succeed.
pub fn apply_linked_ask_fills(
    pages: &[AskPage],
    sidecars: &[AskOwnerPage],
    fills: &[hybrid_engine::ActiveFill],
) -> Result<(Vec<AskPage>, Vec<AskOwnerPage>, usize), StateError> {
    if pages.is_empty() || pages.len() > 8 || pages.len() != sidecars.len() {
        return Err(StateError::Corrupt);
    }
    crate::validate_ask_chain(pages)?;
    for (page, sidecar) in pages.iter().zip(sidecars) {
        sidecar.validate_parallel(page)?;
    }

    let mut entries: Vec<(AskEntry, [u8; 32])> = Vec::new();
    for (page, sidecar) in pages.iter().zip(sidecars) {
        entries.extend(
            page.as_slice()
                .iter()
                .copied()
                .zip(sidecar.as_slice().iter().copied()),
        );
    }
    let initial_len = entries.len();
    let mut previous = None;
    for fill in fills {
        let index = usize::from(fill.ask_index);
        if fill.base_qty == 0
            || fill.quote_qty == 0
            || index >= initial_len
            || previous.is_some_and(|prior| index <= prior)
            || entries[index].0.base_qty < fill.base_qty
        {
            return Err(StateError::Corrupt);
        }
        previous = Some(index);
    }

    for fill in fills.iter().rev() {
        let index = usize::from(fill.ask_index);
        let remaining = entries[index].0.base_qty - fill.base_qty;
        if remaining == 0 {
            entries.remove(index);
        } else {
            entries[index].0.base_qty = remaining;
        }
    }

    let mut result_pages = Vec::with_capacity(pages.len());
    let mut result_sidecars = Vec::with_capacity(pages.len());
    for index in 0..pages.len() {
        let mut page = AskPage::default();
        let mut owners = AskOwnerPage::default();
        let links = pages[index].links();
        page.set_links(links);
        owners.set_links(links);
        let start = index * ASKS_PER_PAGE;
        let end = entries.len().min(start + ASKS_PER_PAGE);
        if start < end {
            for &(entry, owner) in &entries[start..end] {
                crate::insert_owned_ask(&mut page, &mut owners, entry, owner)?;
            }
        }
        result_pages.push(page);
        result_sidecars.push(owners);
    }
    // More than one trailing empty linked page would create an empty middle.
    // Reject instead of writing a chain that later readers cannot validate.
    crate::validate_ask_chain(&result_pages)?;
    for (page, owners) in result_pages.iter().zip(&result_sidecars) {
        owners.validate_parallel(page)?;
    }
    Ok((result_pages, result_sidecars, initial_len - entries.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hybrid_engine::Q64;

    fn linked_pair(
        index: u32,
        prev: Option<u32>,
        next: Option<u32>,
        qty: u64,
        sequence: u64,
    ) -> (AskPage, AskOwnerPage) {
        let links = crate::PageLinks::new(index, prev, next);
        let mut page = AskPage::default();
        let mut owners = AskOwnerPage::default();
        page.set_links(links);
        owners.set_links(links);
        let sqrt = if index == 0 { Q64 } else { Q64 * 2 };
        let price = if index == 0 { Q64 } else { Q64 * 4 };
        crate::insert_owned_ask(
            &mut page,
            &mut owners,
            AskEntry {
                price_x64: price,
                sqrt_price_x64: sqrt,
                base_qty: qty,
                sequence,
            },
            [index as u8 + 1; 32],
        )
        .unwrap();
        (page, owners)
    }

    #[test]
    fn fills_across_boundary_and_compacts_owner_sidecars() {
        let (p0, o0) = linked_pair(0, None, Some(1), 100, 1);
        let (p1, o1) = linked_pair(1, Some(0), None, 200, 2);
        let fills = [
            hybrid_engine::ActiveFill {
                ask_index: 0,
                base_qty: 100,
                quote_qty: 100,
            },
            hybrid_engine::ActiveFill {
                ask_index: 1,
                base_qty: 50,
                quote_qty: 200,
            },
        ];
        let (pages, owners, removed) =
            apply_linked_ask_fills(&[p0, p1], &[o0, o1], &fills).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[0].entries[0].base_qty, 150);
        assert_eq!(pages[0].entries[0].sequence, 2);
        assert_eq!(owners[0].owners[0], [2; 32]);
        assert_eq!(pages[1].len(), 0);
        assert_eq!(owners[1].len(), 0);
    }

    #[test]
    fn rejects_overfill_without_mutating_input() {
        let (p0, o0) = linked_pair(0, None, Some(1), 100, 1);
        let (p1, o1) = linked_pair(1, Some(0), None, 200, 2);
        let fills = [hybrid_engine::ActiveFill {
            ask_index: 1,
            base_qty: 201,
            quote_qty: 804,
        }];
        assert!(apply_linked_ask_fills(&[p0, p1], &[o0, o1], &fills).is_err());
        assert_eq!(p0.entries[0].base_qty, 100);
        assert_eq!(p1.entries[0].base_qty, 200);
    }
}
