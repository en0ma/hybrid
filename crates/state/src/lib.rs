#![forbid(unsafe_code)]

use hybrid_engine::{validate_limit_ask, LimitAsk, PassiveBoundary, QuoteError};

pub const MARKET_HEADER_BYTES: usize = 128;
pub const ASK_ENTRY_BYTES: usize = 48;
pub const BOUNDARY_ENTRY_BYTES: usize = 32;
pub const PAGE_HEADER_BYTES: usize = 16;
pub const ASKS_PER_PAGE: usize = 32;
pub const BOUNDARIES_PER_PAGE: usize = 32;
pub const ASK_PAGE_BYTES: usize = PAGE_HEADER_BYTES + ASKS_PER_PAGE * ASK_ENTRY_BYTES;
pub const BOUNDARY_PAGE_BYTES: usize =
    PAGE_HEADER_BYTES + BOUNDARIES_PER_PAGE * BOUNDARY_ENTRY_BYTES;

pub const MARKET_MAGIC: [u8; 8] = *b"HYBRID01";
pub const STATE_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    Full,
    DuplicateSequence,
    DuplicateBoundary,
    NotFound,
    InvalidAsk,
    InvalidBoundary,
    BufferSize,
    Corrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLinks {
    pub page_index: u32,
    pub prev_page: Option<u32>,
    pub next_page: Option<u32>,
}

impl PageLinks {
    pub const NONE: u32 = u32::MAX;

    pub const fn new(page_index: u32, prev_page: Option<u32>, next_page: Option<u32>) -> Self {
        Self {
            page_index,
            prev_page,
            next_page,
        }
    }

    fn encode_into(self, reserved: &mut [u8; 14]) {
        reserved.fill(0);
        reserved[0..4].copy_from_slice(&self.page_index.to_le_bytes());
        reserved[4..8].copy_from_slice(&self.prev_page.unwrap_or(Self::NONE).to_le_bytes());
        reserved[8..12].copy_from_slice(&self.next_page.unwrap_or(Self::NONE).to_le_bytes());
    }

    fn decode_from(reserved: &[u8; 14]) -> Self {
        if reserved[..12].iter().all(|byte| *byte == 0) {
            return Self::new(0, None, None);
        }

        let page_index = u32::from_le_bytes(reserved[0..4].try_into().expect("page index"));
        let prev_raw = u32::from_le_bytes(reserved[4..8].try_into().expect("prev page"));
        let next_raw = u32::from_le_bytes(reserved[8..12].try_into().expect("next page"));
        Self {
            page_index,
            prev_page: (prev_raw != Self::NONE).then_some(prev_raw),
            next_page: (next_raw != Self::NONE).then_some(next_raw),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketHeader {
    pub magic: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub flags: u16,
    pub ask_count: u32,
    pub boundary_count: u32,
    pub reserved0: [u8; 12],
    pub next_sequence: u64,
    pub reserved1: [u8; 8],
    pub sqrt_price_x64: u128,
    pub liquidity: u128,
    pub base_lot_size: u64,
    pub quote_lot_size: u64,
    pub reserved2: [u8; 32],
}

impl MarketHeader {
    pub const fn new(
        bump: u8,
        sqrt_price_x64: u128,
        liquidity: u128,
        base_lot_size: u64,
        quote_lot_size: u64,
    ) -> Self {
        Self {
            magic: MARKET_MAGIC,
            version: STATE_VERSION,
            bump,
            flags: 0,
            ask_count: 0,
            boundary_count: 0,
            reserved0: [0; 12],
            next_sequence: 1,
            reserved1: [0; 8],
            sqrt_price_x64,
            liquidity,
            base_lot_size,
            quote_lot_size,
            reserved2: [0; 32],
        }
    }

    pub fn allocate_sequence(&mut self) -> Result<u64, StateError> {
        let sequence = self.next_sequence;
        if sequence == 0 {
            return Err(StateError::Corrupt);
        }
        self.next_sequence = sequence.checked_add(1).ok_or(StateError::Full)?;
        Ok(sequence)
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != MARKET_HEADER_BYTES {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        out[0..8].copy_from_slice(&self.magic);
        out[8] = self.version;
        out[9] = self.bump;
        put_u16(out, 10, self.flags);
        put_u32(out, 12, self.ask_count);
        put_u32(out, 16, self.boundary_count);
        out[20..32].copy_from_slice(&self.reserved0);
        put_u64(out, 32, self.next_sequence);
        out[40..48].copy_from_slice(&self.reserved1);
        put_u128(out, 48, self.sqrt_price_x64);
        put_u128(out, 64, self.liquidity);
        put_u64(out, 80, self.base_lot_size);
        put_u64(out, 88, self.quote_lot_size);
        out[96..128].copy_from_slice(&self.reserved2);
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        if input.len() != MARKET_HEADER_BYTES {
            return Err(StateError::BufferSize);
        }
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&input[0..8]);
        if magic != MARKET_MAGIC || input[8] != STATE_VERSION {
            return Err(StateError::Corrupt);
        }
        let mut reserved0 = [0u8; 12];
        reserved0.copy_from_slice(&input[20..32]);
        let mut reserved1 = [0u8; 8];
        reserved1.copy_from_slice(&input[40..48]);
        let mut reserved2 = [0u8; 32];
        reserved2.copy_from_slice(&input[96..128]);
        Ok(Self {
            magic,
            version: input[8],
            bump: input[9],
            flags: get_u16(input, 10),
            ask_count: get_u32(input, 12),
            boundary_count: get_u32(input, 16),
            reserved0,
            next_sequence: get_u64(input, 32),
            reserved1,
            sqrt_price_x64: get_u128(input, 48),
            liquidity: get_u128(input, 64),
            base_lot_size: get_u64(input, 80),
            quote_lot_size: get_u64(input, 88),
            reserved2,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AskEntry {
    pub price_x64: u128,
    pub sqrt_price_x64: u128,
    pub base_qty: u64,
    pub sequence: u64,
}

impl AskEntry {
    pub const EMPTY: Self = Self {
        price_x64: 0,
        sqrt_price_x64: 0,
        base_qty: 0,
        sequence: 0,
    };

    pub fn as_limit_ask(self) -> LimitAsk {
        LimitAsk {
            price_x64: self.price_x64,
            sqrt_price_x64: self.sqrt_price_x64,
            base_qty: self.base_qty,
        }
    }

    fn validate(self) -> Result<(), StateError> {
        if self.sequence == 0 || self.base_qty == 0 {
            return Err(StateError::InvalidAsk);
        }
        validate_limit_ask(self.as_limit_ask()).map_err(map_quote_error)
    }

    fn encode_into(self, out: &mut [u8]) {
        put_u128(out, 0, self.price_x64);
        put_u128(out, 16, self.sqrt_price_x64);
        put_u64(out, 32, self.base_qty);
        put_u64(out, 40, self.sequence);
    }

    fn decode_from(input: &[u8]) -> Self {
        Self {
            price_x64: get_u128(input, 0),
            sqrt_price_x64: get_u128(input, 16),
            base_qty: get_u64(input, 32),
            sequence: get_u64(input, 40),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryEntry {
    pub sqrt_price_x64: u128,
    pub liquidity_after: u128,
}

impl BoundaryEntry {
    pub const EMPTY: Self = Self {
        sqrt_price_x64: 0,
        liquidity_after: 0,
    };

    pub fn as_passive_boundary(self) -> PassiveBoundary {
        PassiveBoundary {
            sqrt_price_x64: self.sqrt_price_x64,
            liquidity_after: self.liquidity_after,
        }
    }

    fn validate(self) -> Result<(), StateError> {
        if self.sqrt_price_x64 == 0 || self.liquidity_after == 0 {
            return Err(StateError::InvalidBoundary);
        }
        Ok(())
    }

    fn encode_into(self, out: &mut [u8]) {
        put_u128(out, 0, self.sqrt_price_x64);
        put_u128(out, 16, self.liquidity_after);
    }

    fn decode_from(input: &[u8]) -> Self {
        Self {
            sqrt_price_x64: get_u128(input, 0),
            liquidity_after: get_u128(input, 16),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AskPage {
    pub len: u16,
    pub reserved: [u8; 14],
    pub entries: [AskEntry; ASKS_PER_PAGE],
}

impl Default for AskPage {
    fn default() -> Self {
        Self {
            len: 0,
            reserved: [0; 14],
            entries: [AskEntry::EMPTY; ASKS_PER_PAGE],
        }
    }
}

impl AskPage {
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[AskEntry] {
        &self.entries[..self.len()]
    }

    pub fn links(&self) -> PageLinks {
        PageLinks::decode_from(&self.reserved)
    }

    pub fn set_links(&mut self, links: PageLinks) {
        links.encode_into(&mut self.reserved);
    }

    pub fn validate_next(&self, next: &Self) -> Result<(), StateError> {
        let links = self.links();
        let next_links = next.links();
        if links.next_page != Some(next_links.page_index)
            || next_links.prev_page != Some(links.page_index)
        {
            return Err(StateError::Corrupt);
        }
        if let (Some(left), Some(right)) = (self.as_slice().last(), next.as_slice().first()) {
            if (left.price_x64, left.sequence) >= (right.price_x64, right.sequence) {
                return Err(StateError::Corrupt);
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, entry: AskEntry) -> Result<usize, StateError> {
        entry.validate()?;
        let len = self.len();
        if len >= ASKS_PER_PAGE {
            return Err(StateError::Full);
        }
        if self.as_slice().iter().any(|e| e.sequence == entry.sequence) {
            return Err(StateError::DuplicateSequence);
        }

        let mut index = 0usize;
        while index < len {
            let current = self.entries[index];
            if (entry.price_x64, entry.sequence) < (current.price_x64, current.sequence) {
                break;
            }
            index += 1;
        }

        let mut cursor = len;
        while cursor > index {
            self.entries[cursor] = self.entries[cursor - 1];
            cursor -= 1;
        }
        self.entries[index] = entry;
        self.len = self.len.checked_add(1).ok_or(StateError::Full)?;
        Ok(index)
    }

    pub fn remove_by_sequence(&mut self, sequence: u64) -> Result<AskEntry, StateError> {
        let len = self.len();
        let index = self
            .as_slice()
            .iter()
            .position(|e| e.sequence == sequence)
            .ok_or(StateError::NotFound)?;
        let removed = self.entries[index];
        let mut cursor = index;
        while cursor + 1 < len {
            self.entries[cursor] = self.entries[cursor + 1];
            cursor += 1;
        }
        self.entries[len - 1] = AskEntry::EMPTY;
        self.len -= 1;
        Ok(removed)
    }

    pub fn set_quantity(&mut self, sequence: u64, base_qty: u64) -> Result<(), StateError> {
        if base_qty == 0 {
            self.remove_by_sequence(sequence)?;
            return Ok(());
        }
        let len = usize::from(self.len);
        let entry = self
            .entries
            .iter_mut()
            .take(len)
            .find(|e| e.sequence == sequence)
            .ok_or(StateError::NotFound)?;
        entry.base_qty = base_qty;
        Ok(())
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != ASK_PAGE_BYTES || self.len() > ASKS_PER_PAGE {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        put_u16(out, 0, self.len);
        out[2..16].copy_from_slice(&self.reserved);
        for (index, entry) in self.entries.iter().enumerate() {
            let start = PAGE_HEADER_BYTES + index * ASK_ENTRY_BYTES;
            entry.encode_into(&mut out[start..start + ASK_ENTRY_BYTES]);
        }
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        if input.len() != ASK_PAGE_BYTES {
            return Err(StateError::BufferSize);
        }
        let len = usize::from(get_u16(input, 0));
        if len > ASKS_PER_PAGE {
            return Err(StateError::Corrupt);
        }
        let mut page = Self {
            len: len as u16,
            ..Self::default()
        };
        page.reserved.copy_from_slice(&input[2..16]);
        for index in 0..ASKS_PER_PAGE {
            let start = PAGE_HEADER_BYTES + index * ASK_ENTRY_BYTES;
            page.entries[index] = AskEntry::decode_from(&input[start..start + ASK_ENTRY_BYTES]);
        }
        page.validate_order()?;
        Ok(page)
    }

    pub fn validate_order(&self) -> Result<(), StateError> {
        let slice = self.as_slice();
        for entry in slice {
            entry.validate()?;
        }
        for pair in slice.windows(2) {
            if (pair[0].price_x64, pair[0].sequence) >= (pair[1].price_x64, pair[1].sequence) {
                return Err(StateError::Corrupt);
            }
        }
        for (index, entry) in slice.iter().enumerate() {
            if slice[index + 1..]
                .iter()
                .any(|other| other.sequence == entry.sequence)
            {
                return Err(StateError::Corrupt);
            }
        }
        if self.entries[self.len()..]
            .iter()
            .any(|e| *e != AskEntry::EMPTY)
        {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryPage {
    pub len: u16,
    pub reserved: [u8; 14],
    pub entries: [BoundaryEntry; BOUNDARIES_PER_PAGE],
}

impl Default for BoundaryPage {
    fn default() -> Self {
        Self {
            len: 0,
            reserved: [0; 14],
            entries: [BoundaryEntry::EMPTY; BOUNDARIES_PER_PAGE],
        }
    }
}

impl BoundaryPage {
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[BoundaryEntry] {
        &self.entries[..self.len()]
    }

    pub fn links(&self) -> PageLinks {
        PageLinks::decode_from(&self.reserved)
    }

    pub fn set_links(&mut self, links: PageLinks) {
        links.encode_into(&mut self.reserved);
    }

    pub fn validate_next(&self, next: &Self) -> Result<(), StateError> {
        let links = self.links();
        let next_links = next.links();
        if links.next_page != Some(next_links.page_index)
            || next_links.prev_page != Some(links.page_index)
        {
            return Err(StateError::Corrupt);
        }
        if let (Some(left), Some(right)) = (self.as_slice().last(), next.as_slice().first()) {
            if left.sqrt_price_x64 >= right.sqrt_price_x64 {
                return Err(StateError::Corrupt);
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, entry: BoundaryEntry) -> Result<usize, StateError> {
        entry.validate()?;
        let len = self.len();
        if len >= BOUNDARIES_PER_PAGE {
            return Err(StateError::Full);
        }
        if self
            .as_slice()
            .iter()
            .any(|e| e.sqrt_price_x64 == entry.sqrt_price_x64)
        {
            return Err(StateError::DuplicateBoundary);
        }

        let mut index = 0usize;
        while index < len && self.entries[index].sqrt_price_x64 < entry.sqrt_price_x64 {
            index += 1;
        }
        let mut cursor = len;
        while cursor > index {
            self.entries[cursor] = self.entries[cursor - 1];
            cursor -= 1;
        }
        self.entries[index] = entry;
        self.len = self.len.checked_add(1).ok_or(StateError::Full)?;
        Ok(index)
    }

    pub fn remove(&mut self, sqrt_price_x64: u128) -> Result<BoundaryEntry, StateError> {
        let len = self.len();
        let index = self
            .as_slice()
            .iter()
            .position(|e| e.sqrt_price_x64 == sqrt_price_x64)
            .ok_or(StateError::NotFound)?;
        let removed = self.entries[index];
        let mut cursor = index;
        while cursor + 1 < len {
            self.entries[cursor] = self.entries[cursor + 1];
            cursor += 1;
        }
        self.entries[len - 1] = BoundaryEntry::EMPTY;
        self.len -= 1;
        Ok(removed)
    }

    pub fn set_liquidity(
        &mut self,
        sqrt_price_x64: u128,
        liquidity_after: u128,
    ) -> Result<(), StateError> {
        if liquidity_after == 0 {
            return Err(StateError::InvalidBoundary);
        }
        let entry = self
            .entries
            .iter_mut()
            .take(usize::from(self.len))
            .find(|e| e.sqrt_price_x64 == sqrt_price_x64)
            .ok_or(StateError::NotFound)?;
        entry.liquidity_after = liquidity_after;
        Ok(())
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != BOUNDARY_PAGE_BYTES || self.len() > BOUNDARIES_PER_PAGE {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        put_u16(out, 0, self.len);
        out[2..16].copy_from_slice(&self.reserved);
        for (index, entry) in self.entries.iter().enumerate() {
            let start = PAGE_HEADER_BYTES + index * BOUNDARY_ENTRY_BYTES;
            entry.encode_into(&mut out[start..start + BOUNDARY_ENTRY_BYTES]);
        }
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        if input.len() != BOUNDARY_PAGE_BYTES {
            return Err(StateError::BufferSize);
        }
        let len = usize::from(get_u16(input, 0));
        if len > BOUNDARIES_PER_PAGE {
            return Err(StateError::Corrupt);
        }
        let mut page = Self {
            len: len as u16,
            ..Self::default()
        };
        page.reserved.copy_from_slice(&input[2..16]);
        for index in 0..BOUNDARIES_PER_PAGE {
            let start = PAGE_HEADER_BYTES + index * BOUNDARY_ENTRY_BYTES;
            page.entries[index] =
                BoundaryEntry::decode_from(&input[start..start + BOUNDARY_ENTRY_BYTES]);
        }
        page.validate_order()?;
        Ok(page)
    }

    pub fn validate_order(&self) -> Result<(), StateError> {
        let slice = self.as_slice();
        for entry in slice {
            entry.validate()?;
        }
        for pair in slice.windows(2) {
            if pair[0].sqrt_price_x64 >= pair[1].sqrt_price_x64 {
                return Err(StateError::Corrupt);
            }
        }
        if self.entries[self.len()..]
            .iter()
            .any(|e| *e != BoundaryEntry::EMPTY)
        {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }
}

pub fn validate_ask_chain(pages: &[AskPage]) -> Result<(), StateError> {
    for (index, page) in pages.iter().enumerate() {
        page.validate_order()?;
        let links = page.links();
        if links.page_index != index as u32 {
            return Err(StateError::Corrupt);
        }
        if index > 0 && index + 1 < pages.len() && page.is_empty() {
            return Err(StateError::Corrupt);
        }
        match index {
            0 if links.prev_page.is_some() => return Err(StateError::Corrupt),
            i if i + 1 == pages.len() && links.next_page.is_some() => {
                return Err(StateError::Corrupt)
            }
            _ => {}
        }

        for entry in page.as_slice() {
            if pages[index + 1..]
                .iter()
                .flat_map(|later| later.as_slice())
                .any(|other| other.sequence == entry.sequence)
            {
                return Err(StateError::Corrupt);
            }
        }
    }
    for pair in pages.windows(2) {
        pair[0].validate_next(&pair[1])?;
    }
    Ok(())
}

pub fn validate_boundary_chain(pages: &[BoundaryPage]) -> Result<(), StateError> {
    for (index, page) in pages.iter().enumerate() {
        page.validate_order()?;
        let links = page.links();
        if links.page_index != index as u32 {
            return Err(StateError::Corrupt);
        }
        if index > 0 && index + 1 < pages.len() && page.is_empty() {
            return Err(StateError::Corrupt);
        }
        match index {
            0 if links.prev_page.is_some() => return Err(StateError::Corrupt),
            i if i + 1 == pages.len() && links.next_page.is_some() => {
                return Err(StateError::Corrupt)
            }
            _ => {}
        }
    }
    for pair in pages.windows(2) {
        pair[0].validate_next(&pair[1])?;
    }
    Ok(())
}

fn map_quote_error(_: QuoteError) -> StateError {
    StateError::InvalidAsk
}

fn put_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn put_u128(out: &mut [u8], offset: usize, value: u128) {
    out[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("fixed u16 slice"),
    )
}

fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("fixed u32 slice"),
    )
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        input[offset..offset + 8]
            .try_into()
            .expect("fixed u64 slice"),
    )
}

fn get_u128(input: &[u8], offset: usize) -> u128 {
    u128::from_le_bytes(
        input[offset..offset + 16]
            .try_into()
            .expect("fixed u128 slice"),
    )
}

const _: [(); MARKET_HEADER_BYTES] = [(); core::mem::size_of::<MarketHeader>()];
const _: [(); ASK_ENTRY_BYTES] = [(); core::mem::size_of::<AskEntry>()];
const _: [(); BOUNDARY_ENTRY_BYTES] = [(); core::mem::size_of::<BoundaryEntry>()];
const _: [(); ASK_PAGE_BYTES] = [(); core::mem::size_of::<AskPage>()];
const _: [(); BOUNDARY_PAGE_BYTES] = [(); core::mem::size_of::<BoundaryPage>()];

#[cfg(test)]
mod tests {
    use super::*;
    use hybrid_engine::{spot_price_x64, Q64};

    fn ask(sqrt_price_x64: u128, base_qty: u64, sequence: u64) -> AskEntry {
        AskEntry {
            price_x64: spot_price_x64(sqrt_price_x64).unwrap(),
            sqrt_price_x64,
            base_qty,
            sequence,
        }
    }

    #[test]
    fn layouts_are_exact_and_small() {
        let layouts = [
            (
                "market_header",
                core::mem::size_of::<MarketHeader>(),
                128usize,
            ),
            ("ask_entry", core::mem::size_of::<AskEntry>(), 48usize),
            (
                "boundary_entry",
                core::mem::size_of::<BoundaryEntry>(),
                32usize,
            ),
            ("ask_page", core::mem::size_of::<AskPage>(), 1_552usize),
            (
                "boundary_page",
                core::mem::size_of::<BoundaryPage>(),
                1_040usize,
            ),
        ];
        for (name, actual, expected) in layouts {
            println!("HYBRID_STATE_BYTES {name} {actual}");
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn market_header_round_trips_exactly() {
        let header = MarketHeader::new(7, Q64, 1_000_000, 10, 20);
        let mut bytes = [0u8; MARKET_HEADER_BYTES];
        header.encode_into(&mut bytes).unwrap();
        assert_eq!(MarketHeader::decode_from(&bytes).unwrap(), header);
    }

    #[test]
    fn ask_page_preserves_price_time_priority() {
        let mut page = AskPage::default();
        let p1 = Q64;
        let p2 = Q64 + Q64 / 100;
        page.insert(ask(p2, 30, 3)).unwrap();
        page.insert(ask(p1, 20, 2)).unwrap();
        page.insert(ask(p1, 10, 1)).unwrap();

        let slice = page.as_slice();
        assert_eq!(slice[0].sequence, 1);
        assert_eq!(slice[1].sequence, 2);
        assert_eq!(slice[2].sequence, 3);
        page.validate_order().unwrap();
    }

    #[test]
    fn ask_cancel_and_quantity_update_compact_page() {
        let mut page = AskPage::default();
        page.insert(ask(Q64, 10, 1)).unwrap();
        page.insert(ask(Q64, 20, 2)).unwrap();
        page.insert(ask(Q64, 30, 3)).unwrap();

        page.set_quantity(2, 25).unwrap();
        assert_eq!(page.as_slice()[1].base_qty, 25);
        page.set_quantity(2, 0).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page.as_slice()[1].sequence, 3);
        assert_eq!(page.entries[2], AskEntry::EMPTY);
    }

    #[test]
    fn ask_page_round_trip_revalidates_order() {
        let mut page = AskPage::default();
        page.insert(ask(Q64, 10, 1)).unwrap();
        page.insert(ask(Q64 + 1, 20, 2)).unwrap();
        let mut bytes = [0u8; ASK_PAGE_BYTES];
        page.encode_into(&mut bytes).unwrap();
        assert_eq!(AskPage::decode_from(&bytes).unwrap(), page);
    }

    #[test]
    fn ask_page_decode_rejects_duplicate_sequences_across_prices() {
        let mut page = AskPage::default();
        page.insert(ask(Q64, 10, 1)).unwrap();
        page.insert(ask(Q64 + Q64 / 100, 20, 2)).unwrap();

        let mut bytes = [0u8; ASK_PAGE_BYTES];
        page.encode_into(&mut bytes).unwrap();

        let second_sequence_offset = PAGE_HEADER_BYTES + ASK_ENTRY_BYTES + 40;
        put_u64(&mut bytes, second_sequence_offset, 1);

        assert_eq!(AskPage::decode_from(&bytes), Err(StateError::Corrupt));
    }

    #[test]
    fn boundary_page_sorts_updates_removes_and_round_trips() {
        let mut page = BoundaryPage::default();
        page.insert(BoundaryEntry {
            sqrt_price_x64: Q64 + 20,
            liquidity_after: 2_000,
        })
        .unwrap();
        page.insert(BoundaryEntry {
            sqrt_price_x64: Q64 + 10,
            liquidity_after: 1_000,
        })
        .unwrap();
        page.set_liquidity(Q64 + 10, 1_500).unwrap();
        assert_eq!(page.as_slice()[0].liquidity_after, 1_500);

        let mut bytes = [0u8; BOUNDARY_PAGE_BYTES];
        page.encode_into(&mut bytes).unwrap();
        assert_eq!(BoundaryPage::decode_from(&bytes).unwrap(), page);

        page.remove(Q64 + 10).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page.entries[1], BoundaryEntry::EMPTY);
    }

    #[test]
    fn page_capacity_is_enforced() {
        let mut page = AskPage::default();
        for i in 0..ASKS_PER_PAGE {
            page.insert(ask(Q64, 1, (i + 1) as u64)).unwrap();
        }
        assert_eq!(page.insert(ask(Q64, 1, 100)), Err(StateError::Full));
    }

    #[test]
    fn sequence_allocator_never_returns_zero() {
        let mut header = MarketHeader::new(1, Q64, 1, 1, 1);
        assert_eq!(header.allocate_sequence().unwrap(), 1);
        assert_eq!(header.allocate_sequence().unwrap(), 2);
        header.next_sequence = u64::MAX;
        assert_eq!(header.allocate_sequence(), Err(StateError::Full));
    }
    #[test]
    fn ask_pages_validate_bidirectional_links_and_cross_page_order() {
        let mut first = AskPage::default();
        first.set_links(PageLinks::new(0, None, Some(1)));
        first.insert(ask(Q64, 10, 1)).unwrap();

        let mut second = AskPage::default();
        second.set_links(PageLinks::new(1, Some(0), None));
        second.insert(ask(Q64 + Q64 / 100, 10, 2)).unwrap();

        assert_eq!(validate_ask_chain(&[first, second]), Ok(()));

        second.set_links(PageLinks::new(1, None, None));
        assert_eq!(
            validate_ask_chain(&[first, second]),
            Err(StateError::Corrupt)
        );
    }

    #[test]
    fn boundary_pages_reject_cross_page_price_overlap() {
        let mut first = BoundaryPage::default();
        first.set_links(PageLinks::new(0, None, Some(1)));
        first
            .insert(BoundaryEntry {
                sqrt_price_x64: Q64 + 20,
                liquidity_after: 1_000,
            })
            .unwrap();

        let mut second = BoundaryPage::default();
        second.set_links(PageLinks::new(1, Some(0), None));
        second
            .insert(BoundaryEntry {
                sqrt_price_x64: Q64 + 10,
                liquidity_after: 2_000,
            })
            .unwrap();

        assert_eq!(
            validate_boundary_chain(&[first, second]),
            Err(StateError::Corrupt)
        );
    }

    #[test]
    fn page_links_round_trip_without_growing_layout() {
        let mut page = AskPage::default();
        let links = PageLinks::new(7, Some(3), Some(11));
        page.set_links(links);
        assert_eq!(page.links(), links);

        let mut bytes = [0u8; ASK_PAGE_BYTES];
        page.encode_into(&mut bytes).unwrap();
        let decoded = AskPage::decode_from(&bytes).unwrap();
        assert_eq!(decoded.links(), links);
        assert_eq!(core::mem::size_of::<AskPage>(), ASK_PAGE_BYTES);
    }

    #[test]
    fn legacy_zero_page_header_decodes_as_unlinked_page_zero() {
        let page = AskPage::default();
        assert_eq!(page.links(), PageLinks::new(0, None, None));
        assert_eq!(validate_ask_chain(core::slice::from_ref(&page)), Ok(()));
    }

    #[test]
    fn ask_chain_rejects_duplicate_sequences_across_pages() {
        let mut first = AskPage::default();
        first.set_links(PageLinks::new(0, None, Some(1)));
        first.insert(ask(Q64, 10, 7)).unwrap();

        let mut second = AskPage::default();
        second.set_links(PageLinks::new(1, Some(0), None));
        second.insert(ask(Q64 + Q64 / 100, 10, 7)).unwrap();

        assert_eq!(
            validate_ask_chain(&[first, second]),
            Err(StateError::Corrupt)
        );
    }

    #[test]
    fn chains_reject_empty_interior_pages() {
        let mut first = AskPage::default();
        first.set_links(PageLinks::new(0, None, Some(1)));
        first.insert(ask(Q64, 10, 1)).unwrap();

        let mut middle = AskPage::default();
        middle.set_links(PageLinks::new(1, Some(0), Some(2)));

        let mut last_page = AskPage::default();
        last_page.set_links(PageLinks::new(2, Some(1), None));
        last_page.insert(ask(Q64 + Q64 / 100, 10, 2)).unwrap();

        assert_eq!(
            validate_ask_chain(&[first, middle, last_page]),
            Err(StateError::Corrupt)
        );
    }
}
