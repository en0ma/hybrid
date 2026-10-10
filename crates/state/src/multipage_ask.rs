
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
