#![forbid(unsafe_code)]

use hybrid_engine::{validate_limit_ask, LimitAsk, PassiveBoundary, QuoteError};

pub const MARKET_HEADER_BYTES: usize = 128;
pub const ASK_ENTRY_BYTES: usize = 48;
pub const BID_ENTRY_BYTES: usize = ASK_ENTRY_BYTES;
pub const BOUNDARY_ENTRY_BYTES: usize = 32;
pub const PAGE_HEADER_BYTES: usize = 16;
pub const ASKS_PER_PAGE: usize = 32;
pub const BIDS_PER_PAGE: usize = ASKS_PER_PAGE;
pub const BOUNDARIES_PER_PAGE: usize = 32;
pub const ASK_PAGE_BYTES: usize = PAGE_HEADER_BYTES + ASKS_PER_PAGE * ASK_ENTRY_BYTES;
pub const BID_PAGE_BYTES: usize = PAGE_HEADER_BYTES + BIDS_PER_PAGE * BID_ENTRY_BYTES;
pub const ASK_OWNER_BYTES: usize = 32;
pub const BID_OWNER_BYTES: usize = ASK_OWNER_BYTES;
pub const ASK_OWNER_PAGE_BYTES: usize = PAGE_HEADER_BYTES + ASKS_PER_PAGE * ASK_OWNER_BYTES;
pub const BID_OWNER_PAGE_BYTES: usize = PAGE_HEADER_BYTES + BIDS_PER_PAGE * BID_OWNER_BYTES;
pub const BOUNDARY_PAGE_BYTES: usize =
    PAGE_HEADER_BYTES + BOUNDARIES_PER_PAGE * BOUNDARY_ENTRY_BYTES;
pub const CUSTODY_STATE_BYTES: usize = 192;
pub const MAKER_BALANCE_BYTES: usize = 112;

pub const MARKET_MAGIC: [u8; 8] = *b"HYBRID01";
pub const STATE_VERSION: u8 = 1;
pub const MARKET_FLAG_COLLATERALIZED_ACTIVE: u16 = 1;
pub const CUSTODY_MAGIC: [u8; 8] = *b"HYBCUST1";
pub const MAKER_BALANCE_MAGIC: [u8; 8] = *b"HYBBAL01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    Full,
    DuplicateSequence,
    DuplicateBoundary,
    NotFound,
    InvalidAsk,
    InvalidBoundary,
    InvalidOwner,
    Unauthorized,
    BufferSize,
    Corrupt,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustodyState {
    pub magic: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub reserved: [u8; 4],
    pub market: [u8; 32],
    pub base_mint: [u8; 32],
    pub quote_mint: [u8; 32],
    pub base_vault: [u8; 32],
    pub quote_vault: [u8; 32],
    pub total_base: u64,
    pub total_quote: u64,
}

impl CustodyState {
    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != CUSTODY_STATE_BYTES {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        out[0..8].copy_from_slice(&self.magic);
        out[8] = self.version;
        out[9] = self.bump;
        out[10] = self.base_decimals;
        out[11] = self.quote_decimals;
        out[12..16].copy_from_slice(&self.reserved);
        out[16..48].copy_from_slice(&self.market);
        out[48..80].copy_from_slice(&self.base_mint);
        out[80..112].copy_from_slice(&self.quote_mint);
        out[112..144].copy_from_slice(&self.base_vault);
        out[144..176].copy_from_slice(&self.quote_vault);
        put_u64(out, 176, self.total_base);
        put_u64(out, 184, self.total_quote);
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        if input.len() != CUSTODY_STATE_BYTES {
            return Err(StateError::BufferSize);
        }
        let state = Self {
            magic: input[0..8].try_into().map_err(|_| StateError::Corrupt)?,
            version: input[8],
            bump: input[9],
            base_decimals: input[10],
            quote_decimals: input[11],
            reserved: input[12..16].try_into().map_err(|_| StateError::Corrupt)?,
            market: input[16..48].try_into().map_err(|_| StateError::Corrupt)?,
            base_mint: input[48..80].try_into().map_err(|_| StateError::Corrupt)?,
            quote_mint: input[80..112].try_into().map_err(|_| StateError::Corrupt)?,
            base_vault: input[112..144]
                .try_into()
                .map_err(|_| StateError::Corrupt)?,
            quote_vault: input[144..176]
                .try_into()
                .map_err(|_| StateError::Corrupt)?,
            total_base: get_u64(input, 176),
            total_quote: get_u64(input, 184),
        };
        if state.magic != CUSTODY_MAGIC
            || state.version != STATE_VERSION
            || state.reserved != [0; 4]
            || state.market == [0; 32]
            || state.base_mint == [0; 32]
            || state.quote_mint == [0; 32]
            || state.base_vault == [0; 32]
            || state.quote_vault == [0; 32]
        {
            return Err(StateError::Corrupt);
        }
        if state.base_mint == state.quote_mint || state.base_vault == state.quote_vault {
            return Err(StateError::Corrupt);
        }
        Ok(state)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MakerBalance {
    pub magic: [u8; 8],
    pub version: u8,
    pub bump: u8,
    pub reserved: [u8; 6],
    pub market: [u8; 32],
    pub owner: [u8; 32],
    pub free_base: u64,
    pub locked_base: u64,
    pub free_quote: u64,
    pub locked_quote: u64,
}

impl MakerBalance {
    pub const fn new(bump: u8, market: [u8; 32], owner: [u8; 32]) -> Self {
        Self {
            magic: MAKER_BALANCE_MAGIC,
            version: STATE_VERSION,
            bump,
            reserved: [0; 6],
            market,
            owner,
            free_base: 0,
            locked_base: 0,
            free_quote: 0,
            locked_quote: 0,
        }
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != MAKER_BALANCE_BYTES {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        out[0..8].copy_from_slice(&self.magic);
        out[8] = self.version;
        out[9] = self.bump;
        out[10..16].copy_from_slice(&self.reserved);
        out[16..48].copy_from_slice(&self.market);
        out[48..80].copy_from_slice(&self.owner);
        put_u64(out, 80, self.free_base);
        put_u64(out, 88, self.locked_base);
        put_u64(out, 96, self.free_quote);
        put_u64(out, 104, self.locked_quote);
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        if input.len() != MAKER_BALANCE_BYTES {
            return Err(StateError::BufferSize);
        }
        let balance = Self {
            magic: input[0..8].try_into().map_err(|_| StateError::Corrupt)?,
            version: input[8],
            bump: input[9],
            reserved: input[10..16].try_into().map_err(|_| StateError::Corrupt)?,
            market: input[16..48].try_into().map_err(|_| StateError::Corrupt)?,
            owner: input[48..80].try_into().map_err(|_| StateError::Corrupt)?,
            free_base: get_u64(input, 80),
            locked_base: get_u64(input, 88),
            free_quote: get_u64(input, 96),
            locked_quote: get_u64(input, 104),
        };
        if balance.magic != MAKER_BALANCE_MAGIC
            || balance.version != STATE_VERSION
            || balance.reserved != [0; 6]
            || balance.market == [0; 32]
            || balance.owner == [0; 32]
        {
            return Err(StateError::Corrupt);
        }
        Ok(balance)
    }

    pub fn lock_base(&mut self, amount: u64) -> Result<(), StateError> {
        self.free_base = self
            .free_base
            .checked_sub(amount)
            .ok_or(StateError::Unauthorized)?;
        self.locked_base = self
            .locked_base
            .checked_add(amount)
            .ok_or(StateError::Full)?;
        Ok(())
    }

    pub fn unlock_base(&mut self, amount: u64) -> Result<(), StateError> {
        self.locked_base = self
            .locked_base
            .checked_sub(amount)
            .ok_or(StateError::Corrupt)?;
        self.free_base = self.free_base.checked_add(amount).ok_or(StateError::Full)?;
        Ok(())
    }

    pub fn lock_quote(&mut self, amount: u64) -> Result<(), StateError> {
        self.free_quote = self
            .free_quote
            .checked_sub(amount)
            .ok_or(StateError::Unauthorized)?;
        self.locked_quote = self
            .locked_quote
            .checked_add(amount)
            .ok_or(StateError::Full)?;
        Ok(())
    }

    pub fn unlock_quote(&mut self, amount: u64) -> Result<(), StateError> {
        self.locked_quote = self
            .locked_quote
            .checked_sub(amount)
            .ok_or(StateError::Corrupt)?;
        self.free_quote = self
            .free_quote
            .checked_add(amount)
            .ok_or(StateError::Full)?;
        Ok(())
    }

    pub fn settle_ask(&mut self, base: u64, quote: u64) -> Result<(), StateError> {
        self.locked_base = self
            .locked_base
            .checked_sub(base)
            .ok_or(StateError::Corrupt)?;
        self.free_quote = self.free_quote.checked_add(quote).ok_or(StateError::Full)?;
        Ok(())
    }

    pub fn settle_bid(&mut self, base: u64, quote: u64) -> Result<(), StateError> {
        self.locked_quote = self
            .locked_quote
            .checked_sub(quote)
            .ok_or(StateError::Corrupt)?;
        self.free_base = self.free_base.checked_add(base).ok_or(StateError::Full)?;
        Ok(())
    }
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

    pub fn bid_count(&self) -> u32 {
        u32::from_le_bytes(self.reserved0[0..4].try_into().expect("bid count"))
    }

    pub fn set_bid_count(&mut self, bid_count: u32) {
        self.reserved0[0..4].copy_from_slice(&bid_count.to_le_bytes());
    }

    pub fn bid_owner_tag(&self) -> [u8; 16] {
        let mut tag = [0u8; 16];
        tag[0..8].copy_from_slice(&self.reserved0[4..12]);
        tag[8..16].copy_from_slice(&self.reserved1);
        tag
    }

    pub fn set_bid_owner_tag(&mut self, tag: [u8; 16]) {
        self.reserved0[4..12].copy_from_slice(&tag[0..8]);
        self.reserved1.copy_from_slice(&tag[8..16]);
    }

    pub fn collateralized_active(&self) -> bool {
        self.flags & MARKET_FLAG_COLLATERALIZED_ACTIVE != 0
    }

    pub fn enable_collateralized_active(&mut self) {
        self.flags |= MARKET_FLAG_COLLATERALIZED_ACTIVE;
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

    pub fn decode_into(input: &[u8], page: &mut Self) -> Result<(), StateError> {
        if input.len() != ASK_PAGE_BYTES {
            return Err(StateError::BufferSize);
        }
        let len = usize::from(get_u16(input, 0));
        if len > ASKS_PER_PAGE {
            return Err(StateError::Corrupt);
        }
        page.len = len as u16;
        page.reserved.copy_from_slice(&input[2..16]);
        for index in 0..ASKS_PER_PAGE {
            let start = PAGE_HEADER_BYTES + index * ASK_ENTRY_BYTES;
            page.entries[index] = AskEntry::decode_from(&input[start..start + ASK_ENTRY_BYTES]);
        }
        page.validate_order()
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        let mut page = Self::default();
        Self::decode_into(input, &mut page)?;
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
pub struct AskOwnerPage {
    pub len: u16,
    pub reserved: [u8; 14],
    pub owners: [[u8; ASK_OWNER_BYTES]; ASKS_PER_PAGE],
}

impl Default for AskOwnerPage {
    fn default() -> Self {
        Self {
            len: 0,
            reserved: [0; 14],
            owners: [[0; ASK_OWNER_BYTES]; ASKS_PER_PAGE],
        }
    }
}

impl AskOwnerPage {
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn links(&self) -> PageLinks {
        PageLinks::decode_from(&self.reserved)
    }

    pub fn set_links(&mut self, links: PageLinks) {
        links.encode_into(&mut self.reserved);
    }

    pub fn as_slice(&self) -> &[[u8; ASK_OWNER_BYTES]] {
        &self.owners[..self.len()]
    }

    pub fn validate_parallel(&self, asks: &AskPage) -> Result<(), StateError> {
        if self.len != asks.len || self.links() != asks.links() {
            return Err(StateError::Corrupt);
        }
        if self.as_slice().contains(&[0; ASK_OWNER_BYTES]) {
            return Err(StateError::InvalidOwner);
        }
        if self.owners[self.len()..]
            .iter()
            .any(|owner| *owner != [0; ASK_OWNER_BYTES])
        {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }

    fn insert_at(&mut self, index: usize, owner: [u8; ASK_OWNER_BYTES]) -> Result<(), StateError> {
        if owner == [0; ASK_OWNER_BYTES] {
            return Err(StateError::InvalidOwner);
        }
        let len = self.len();
        if len >= ASKS_PER_PAGE || index > len {
            return Err(StateError::Full);
        }
        let mut cursor = len;
        while cursor > index {
            self.owners[cursor] = self.owners[cursor - 1];
            cursor -= 1;
        }
        self.owners[index] = owner;
        self.len = self.len.checked_add(1).ok_or(StateError::Full)?;
        Ok(())
    }

    fn remove_at(&mut self, index: usize) -> Result<[u8; ASK_OWNER_BYTES], StateError> {
        let len = self.len();
        if index >= len {
            return Err(StateError::NotFound);
        }
        let removed = self.owners[index];
        let mut cursor = index;
        while cursor + 1 < len {
            self.owners[cursor] = self.owners[cursor + 1];
            cursor += 1;
        }
        self.owners[len - 1] = [0; ASK_OWNER_BYTES];
        self.len -= 1;
        Ok(removed)
    }

    pub fn encode_into(&self, out: &mut [u8]) -> Result<(), StateError> {
        if out.len() != ASK_OWNER_PAGE_BYTES || self.len() > ASKS_PER_PAGE {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        put_u16(out, 0, self.len);
        out[2..16].copy_from_slice(&self.reserved);
        for (index, owner) in self.owners.iter().enumerate() {
            let start = PAGE_HEADER_BYTES + index * ASK_OWNER_BYTES;
            out[start..start + ASK_OWNER_BYTES].copy_from_slice(owner);
        }
        Ok(())
    }

    pub fn decode_into(input: &[u8], page: &mut Self) -> Result<(), StateError> {
        if input.len() != ASK_OWNER_PAGE_BYTES {
            return Err(StateError::BufferSize);
        }
        let len = usize::from(get_u16(input, 0));
        if len > ASKS_PER_PAGE {
            return Err(StateError::Corrupt);
        }
        page.len = len as u16;
        page.reserved.copy_from_slice(&input[2..16]);
        for index in 0..ASKS_PER_PAGE {
            let start = PAGE_HEADER_BYTES + index * ASK_OWNER_BYTES;
            page.owners[index].copy_from_slice(&input[start..start + ASK_OWNER_BYTES]);
        }
        if page.owners[page.len()..]
            .iter()
            .any(|owner| *owner != [0; ASK_OWNER_BYTES])
        {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        let mut page = Self::default();
        Self::decode_into(input, &mut page)?;
        Ok(page)
    }
}

pub fn insert_owned_ask(
    asks: &mut AskPage,
    owners: &mut AskOwnerPage,
    entry: AskEntry,
    owner: [u8; ASK_OWNER_BYTES],
) -> Result<usize, StateError> {
    owners.validate_parallel(asks)?;
    if owner == [0; ASK_OWNER_BYTES] {
        return Err(StateError::InvalidOwner);
    }
    if asks.len() >= ASKS_PER_PAGE {
        return Err(StateError::Full);
    }

    entry.validate()?;
    if asks
        .as_slice()
        .iter()
        .any(|existing| existing.sequence == entry.sequence)
    {
        return Err(StateError::DuplicateSequence);
    }

    let mut index = 0usize;
    while index < asks.len()
        && (asks.entries[index].price_x64, asks.entries[index].sequence)
            < (entry.price_x64, entry.sequence)
    {
        index += 1;
    }

    owners.insert_at(index, owner)?;
    let inserted = asks.insert(entry)?;
    debug_assert_eq!(inserted, index);
    owners.validate_parallel(asks)?;
    Ok(index)
}

pub fn cancel_owned_ask(
    asks: &mut AskPage,
    owners: &mut AskOwnerPage,
    sequence: u64,
    owner: [u8; ASK_OWNER_BYTES],
) -> Result<AskEntry, StateError> {
    owners.validate_parallel(asks)?;
    let index = asks
        .as_slice()
        .iter()
        .position(|entry| entry.sequence == sequence)
        .ok_or(StateError::NotFound)?;
    if owners.owners[index] != owner {
        return Err(StateError::Unauthorized);
    }

    let removed = asks.remove_by_sequence(sequence)?;
    owners.remove_at(index)?;
    owners.validate_parallel(asks)?;
    Ok(removed)
}

pub type BidEntry = AskEntry;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BidPage {
    pub len: u16,
    pub reserved: [u8; 14],
    pub entries: [BidEntry; BIDS_PER_PAGE],
}

impl Default for BidPage {
    fn default() -> Self {
        Self {
            len: 0,
            reserved: [0; 14],
            entries: [BidEntry::EMPTY; BIDS_PER_PAGE],
        }
    }
}

impl BidPage {
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[BidEntry] {
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
            if left.price_x64 < right.price_x64
                || (left.price_x64 == right.price_x64 && left.sequence >= right.sequence)
            {
                return Err(StateError::Corrupt);
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, entry: BidEntry) -> Result<usize, StateError> {
        entry.validate()?;
        let len = self.len();
        if len >= BIDS_PER_PAGE {
            return Err(StateError::Full);
        }
        if self.as_slice().iter().any(|e| e.sequence == entry.sequence) {
            return Err(StateError::DuplicateSequence);
        }

        let mut index = 0usize;
        while index < len {
            let current = self.entries[index];
            if entry.price_x64 > current.price_x64
                || (entry.price_x64 == current.price_x64 && entry.sequence < current.sequence)
            {
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

    pub fn remove_by_sequence(&mut self, sequence: u64) -> Result<BidEntry, StateError> {
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
        self.entries[len - 1] = BidEntry::EMPTY;
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
        if out.len() != BID_PAGE_BYTES || self.len() > BIDS_PER_PAGE {
            return Err(StateError::BufferSize);
        }
        out.fill(0);
        put_u16(out, 0, self.len);
        out[2..16].copy_from_slice(&self.reserved);
        for (index, entry) in self.entries.iter().enumerate() {
            let start = PAGE_HEADER_BYTES + index * BID_ENTRY_BYTES;
            entry.encode_into(&mut out[start..start + BID_ENTRY_BYTES]);
        }
        Ok(())
    }

    pub fn decode_into(input: &[u8], page: &mut Self) -> Result<(), StateError> {
        if input.len() != BID_PAGE_BYTES {
            return Err(StateError::BufferSize);
        }
        let len = usize::from(get_u16(input, 0));
        if len > BIDS_PER_PAGE {
            return Err(StateError::Corrupt);
        }
        page.len = len as u16;
        page.reserved.copy_from_slice(&input[2..16]);
        for index in 0..BIDS_PER_PAGE {
            let start = PAGE_HEADER_BYTES + index * BID_ENTRY_BYTES;
            page.entries[index] = BidEntry::decode_from(&input[start..start + BID_ENTRY_BYTES]);
        }
        page.validate_order()
    }

    pub fn decode_from(input: &[u8]) -> Result<Self, StateError> {
        let mut page = Self::default();
        Self::decode_into(input, &mut page)?;
        Ok(page)
    }

    pub fn validate_order(&self) -> Result<(), StateError> {
        let slice = self.as_slice();
        for entry in slice {
            entry.validate()?;
        }
        for pair in slice.windows(2) {
            if pair[0].price_x64 < pair[1].price_x64
                || (pair[0].price_x64 == pair[1].price_x64 && pair[0].sequence >= pair[1].sequence)
            {
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
            .any(|e| *e != BidEntry::EMPTY)
        {
            return Err(StateError::Corrupt);
        }
        Ok(())
    }
}

pub type BidOwnerPage = AskOwnerPage;

pub fn insert_owned_bid(
    bids: &mut BidPage,
    owners: &mut BidOwnerPage,
    entry: BidEntry,
    owner: [u8; BID_OWNER_BYTES],
) -> Result<usize, StateError> {
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    if owner == [0; BID_OWNER_BYTES] {
        return Err(StateError::InvalidOwner);
    }
    if bids.len() >= BIDS_PER_PAGE {
        return Err(StateError::Full);
    }

    let index = bids.insert(entry)?;
    owners.insert_at(index, owner)?;
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    Ok(index)
}

pub fn cancel_owned_bid(
    bids: &mut BidPage,
    owners: &mut BidOwnerPage,
    sequence: u64,
    owner: [u8; BID_OWNER_BYTES],
) -> Result<BidEntry, StateError> {
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    let index = bids
        .as_slice()
        .iter()
        .position(|entry| entry.sequence == sequence)
        .ok_or(StateError::NotFound)?;
    if owners.owners[index] != owner {
        return Err(StateError::Unauthorized);
    }

    let removed = bids.remove_by_sequence(sequence)?;
    owners.remove_at(index)?;
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    Ok(removed)
}

pub fn apply_active_fills_to_ask_page(
    asks: &mut AskPage,
    owners: &mut AskOwnerPage,
    fills: &[hybrid_engine::ActiveFill],
) -> Result<usize, StateError> {
    owners.validate_parallel(asks)?;
    let original_len = asks.len();
    let mut previous_index = None;
    for fill in fills {
        let index = usize::from(fill.ask_index);
        if fill.base_qty == 0 || fill.quote_qty == 0 || index >= original_len {
            return Err(StateError::Corrupt);
        }
        if previous_index.is_some_and(|previous| index <= previous) {
            return Err(StateError::Corrupt);
        }
        if asks.entries[index].base_qty < fill.base_qty {
            return Err(StateError::Corrupt);
        }
        previous_index = Some(index);
    }

    let mut removed = 0usize;
    for fill in fills.iter().rev() {
        let index = usize::from(fill.ask_index);
        let remaining = asks.entries[index].base_qty - fill.base_qty;
        if remaining == 0 {
            let sequence = asks.entries[index].sequence;
            asks.remove_by_sequence(sequence)?;
            owners.remove_at(index)?;
            removed += 1;
        } else {
            asks.entries[index].base_qty = remaining;
        }
    }
    asks.validate_order()?;
    owners.validate_parallel(asks)?;
    Ok(removed)
}

/// Apply a canonical active sell plan without breaking the parallel owner index.
/// Validate all records before mutation, then remove full bids in reverse order.
pub fn apply_active_fills_to_bid_page(
    bids: &mut BidPage,
    owners: &mut BidOwnerPage,
    fills: &[hybrid_engine::ActiveBidFill],
) -> Result<usize, StateError> {
    bids.validate_order()?;
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    let mut previous = None;
    for fill in fills {
        let index = usize::from(fill.bid_index);
        if fill.base_qty == 0
            || fill.quote_qty == 0
            || index >= bids.len()
            || previous.is_some_and(|prior| index <= prior)
            || bids.entries[index].base_qty < fill.base_qty
        {
            return Err(StateError::Corrupt);
        }
        previous = Some(index);
    }
    let mut removed = 0usize;
    for fill in fills.iter().rev() {
        let index = usize::from(fill.bid_index);
        let remaining = bids.entries[index].base_qty - fill.base_qty;
        if remaining == 0 {
            let sequence = bids.entries[index].sequence;
            bids.remove_by_sequence(sequence)?;
            owners.remove_at(index)?;
            removed += 1;
        } else {
            bids.entries[index].base_qty = remaining;
        }
    }
    bids.validate_order()?;
    if owners.len() != bids.len() || owners.links() != bids.links() {
        return Err(StateError::Corrupt);
    }
    Ok(removed)
}

pub fn validate_bid_chain(pages: &[BidPage]) -> Result<(), StateError> {
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

const _: [(); CUSTODY_STATE_BYTES] = [(); core::mem::size_of::<CustodyState>()];
const _: [(); MAKER_BALANCE_BYTES] = [(); core::mem::size_of::<MakerBalance>()];
const _: [(); MARKET_HEADER_BYTES] = [(); core::mem::size_of::<MarketHeader>()];
const _: [(); ASK_ENTRY_BYTES] = [(); core::mem::size_of::<AskEntry>()];
const _: [(); BID_ENTRY_BYTES] = [(); core::mem::size_of::<BidEntry>()];
const _: [(); BOUNDARY_ENTRY_BYTES] = [(); core::mem::size_of::<BoundaryEntry>()];
const _: [(); ASK_PAGE_BYTES] = [(); core::mem::size_of::<AskPage>()];
const _: [(); BID_PAGE_BYTES] = [(); core::mem::size_of::<BidPage>()];
const _: [(); ASK_OWNER_PAGE_BYTES] = [(); core::mem::size_of::<AskOwnerPage>()];
const _: [(); BID_OWNER_PAGE_BYTES] = [(); core::mem::size_of::<BidOwnerPage>()];
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
    fn owned_ask_insert_and_cancel_preserve_parallel_order() {
        let mut asks = AskPage::default();
        asks.set_links(PageLinks::new(0, None, None));
        let mut owners = AskOwnerPage::default();
        owners.set_links(PageLinks::new(0, None, None));

        let owner_a = [1u8; ASK_OWNER_BYTES];
        let owner_b = [2u8; ASK_OWNER_BYTES];
        let higher_sqrt = hybrid_engine::Q64 + hybrid_engine::Q64 / 100;

        insert_owned_ask(
            &mut asks,
            &mut owners,
            AskEntry {
                price_x64: hybrid_engine::spot_price_x64(higher_sqrt).unwrap(),
                sqrt_price_x64: higher_sqrt,
                base_qty: 20,
                sequence: 2,
            },
            owner_b,
        )
        .unwrap();
        insert_owned_ask(
            &mut asks,
            &mut owners,
            AskEntry {
                price_x64: hybrid_engine::Q64,
                sqrt_price_x64: hybrid_engine::Q64,
                base_qty: 10,
                sequence: 1,
            },
            owner_a,
        )
        .unwrap();

        assert_eq!(asks.as_slice()[0].sequence, 1);
        assert_eq!(owners.as_slice()[0], owner_a);
        assert_eq!(asks.as_slice()[1].sequence, 2);
        assert_eq!(owners.as_slice()[1], owner_b);

        assert_eq!(
            cancel_owned_ask(&mut asks, &mut owners, 1, owner_b),
            Err(StateError::Unauthorized)
        );
        let removed = cancel_owned_ask(&mut asks, &mut owners, 1, owner_a).unwrap();
        assert_eq!(removed.sequence, 1);
        assert_eq!(asks.as_slice()[0].sequence, 2);
        assert_eq!(owners.as_slice()[0], owner_b);
    }

    #[test]
    fn active_fill_application_reduces_and_compacts_asks() {
        let mut asks = AskPage::default();
        asks.set_links(PageLinks::new(0, None, None));
        let mut owners = AskOwnerPage::default();
        owners.set_links(PageLinks::new(0, None, None));
        insert_owned_ask(
            &mut asks,
            &mut owners,
            ask(Q64, 100, 1),
            [1; ASK_OWNER_BYTES],
        )
        .unwrap();
        insert_owned_ask(
            &mut asks,
            &mut owners,
            ask(Q64, 200, 2),
            [2; ASK_OWNER_BYTES],
        )
        .unwrap();
        insert_owned_ask(
            &mut asks,
            &mut owners,
            ask(Q64 + 1, 300, 3),
            [3; ASK_OWNER_BYTES],
        )
        .unwrap();

        let fills = [
            hybrid_engine::ActiveFill {
                ask_index: 0,
                base_qty: 100,
                quote_qty: 100,
            },
            hybrid_engine::ActiveFill {
                ask_index: 1,
                base_qty: 50,
                quote_qty: 50,
            },
        ];

        let removed = apply_active_fills_to_ask_page(&mut asks, &mut owners, &fills).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(asks.len(), 2);
        assert_eq!(asks.as_slice()[0].sequence, 2);
        assert_eq!(asks.as_slice()[0].base_qty, 150);
        assert_eq!(asks.as_slice()[1].sequence, 3);
        assert_eq!(owners.len(), 2);
        assert_eq!(owners.as_slice()[0], [2; ASK_OWNER_BYTES]);
        assert_eq!(owners.as_slice()[1], [3; ASK_OWNER_BYTES]);
        owners.validate_parallel(&asks).unwrap();
    }

    #[test]
    fn active_fill_application_rejects_overfill() {
        let mut asks = AskPage::default();
        asks.set_links(PageLinks::new(0, None, None));
        let mut owners = AskOwnerPage::default();
        owners.set_links(PageLinks::new(0, None, None));
        insert_owned_ask(
            &mut asks,
            &mut owners,
            ask(Q64, 100, 1),
            [1; ASK_OWNER_BYTES],
        )
        .unwrap();
        let fills = [hybrid_engine::ActiveFill {
            ask_index: 0,
            base_qty: 101,
            quote_qty: 101,
        }];
        assert_eq!(
            apply_active_fills_to_ask_page(&mut asks, &mut owners, &fills),
            Err(StateError::Corrupt)
        );
        assert_eq!(asks.as_slice()[0].base_qty, 100);
        assert_eq!(owners.as_slice()[0], [1; ASK_OWNER_BYTES]);
        owners.validate_parallel(&asks).unwrap();
    }

    #[test]
    fn bid_page_preserves_price_time_priority() {
        let mut page = BidPage::default();
        let low = Q64;
        let high = Q64 + Q64 / 100;
        page.insert(ask(low, 10, 2)).unwrap();
        page.insert(ask(high, 20, 3)).unwrap();
        page.insert(ask(high, 30, 1)).unwrap();

        let slice = page.as_slice();
        assert_eq!(slice[0].sequence, 1);
        assert_eq!(slice[1].sequence, 3);
        assert_eq!(slice[2].sequence, 2);
        page.validate_order().unwrap();
    }

    #[test]
    fn owned_bid_insert_and_cancel_preserve_parallel_order() {
        let mut bids = BidPage::default();
        bids.set_links(PageLinks::new(0, None, None));
        let mut owners = BidOwnerPage::default();
        owners.set_links(PageLinks::new(0, None, None));

        insert_owned_bid(
            &mut bids,
            &mut owners,
            ask(Q64, 10, 1),
            [1u8; BID_OWNER_BYTES],
        )
        .unwrap();
        insert_owned_bid(
            &mut bids,
            &mut owners,
            ask(Q64 + Q64 / 100, 20, 2),
            [2u8; BID_OWNER_BYTES],
        )
        .unwrap();

        assert_eq!(bids.as_slice()[0].sequence, 2);
        assert_eq!(owners.as_slice()[0], [2u8; BID_OWNER_BYTES]);
        assert_eq!(
            cancel_owned_bid(&mut bids, &mut owners, 2, [1u8; BID_OWNER_BYTES]),
            Err(StateError::Unauthorized)
        );
        cancel_owned_bid(&mut bids, &mut owners, 2, [2u8; BID_OWNER_BYTES]).unwrap();
        assert_eq!(bids.as_slice()[0].sequence, 1);
    }

    #[test]
    fn market_header_tracks_bid_metadata_without_size_growth() {
        let mut header = MarketHeader::new(1, Q64, 1, 1, 1);
        let tag = [7u8; 16];
        header.set_bid_count(3);
        header.set_bid_owner_tag(tag);

        let mut bytes = [0u8; MARKET_HEADER_BYTES];
        header.encode_into(&mut bytes).unwrap();
        let decoded = MarketHeader::decode_from(&bytes).unwrap();
        assert_eq!(decoded.bid_count(), 3);
        assert_eq!(decoded.bid_owner_tag(), tag);
    }

    #[test]
    fn owner_page_round_trips_and_rejects_desync() {
        let mut asks = AskPage::default();
        asks.set_links(PageLinks::new(0, None, None));
        let mut owners = AskOwnerPage::default();
        owners.set_links(PageLinks::new(0, None, None));

        insert_owned_ask(
            &mut asks,
            &mut owners,
            AskEntry {
                price_x64: hybrid_engine::Q64,
                sqrt_price_x64: hybrid_engine::Q64,
                base_qty: 10,
                sequence: 1,
            },
            [9u8; ASK_OWNER_BYTES],
        )
        .unwrap();

        let mut bytes = [0u8; ASK_OWNER_PAGE_BYTES];
        owners.encode_into(&mut bytes).unwrap();
        assert_eq!(AskOwnerPage::decode_from(&bytes).unwrap(), owners);

        owners.len = 0;
        assert_eq!(owners.validate_parallel(&asks), Err(StateError::Corrupt));
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
            ("bid_entry", core::mem::size_of::<BidEntry>(), 48usize),
            (
                "boundary_entry",
                core::mem::size_of::<BoundaryEntry>(),
                32usize,
            ),
            ("ask_page", core::mem::size_of::<AskPage>(), 1_552usize),
            ("bid_page", core::mem::size_of::<BidPage>(), 1_552usize),
            (
                "ask_owner_page",
                core::mem::size_of::<AskOwnerPage>(),
                1_040usize,
            ),
            (
                "bid_owner_page",
                core::mem::size_of::<BidOwnerPage>(),
                1_040usize,
            ),
            (
                "boundary_page",
                core::mem::size_of::<BoundaryPage>(),
                1_040usize,
            ),
            (
                "custody_state",
                core::mem::size_of::<CustodyState>(),
                192usize,
            ),
            (
                "maker_balance",
                core::mem::size_of::<MakerBalance>(),
                112usize,
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


#[cfg(test)]
mod bid_fill_regression_tests {
    use super::*;

    #[test]
    fn full_and_partial_fills_preserve_parallel_bid_owners() {
        let mut bids = BidPage::default();
        let mut owners = BidOwnerPage::default();
        bids.set_links(PageLinks::new(0, None, None));
        owners.set_links(PageLinks::new(0, None, None));
        for sequence in 1..=3 {
            insert_owned_bid(
                &mut bids,
                &mut owners,
                BidEntry {
                    price_x64: hybrid_engine::Q64,
                    sqrt_price_x64: hybrid_engine::Q64,
                    base_qty: 10,
                    sequence,
                },
                [sequence as u8; BID_OWNER_BYTES],
            )
            .unwrap();
        }
        let fills = [
            hybrid_engine::ActiveBidFill {
                bid_index: 0,
                base_qty: 10,
                quote_qty: 10,
            },
            hybrid_engine::ActiveBidFill {
                bid_index: 2,
                base_qty: 4,
                quote_qty: 4,
            },
        ];
        assert_eq!(apply_active_fills_to_bid_page(&mut bids, &mut owners, &fills), Ok(1));
        assert_eq!(bids.len(), 2);
        assert_eq!(bids.as_slice()[0].sequence, 2);
        assert_eq!(bids.as_slice()[1].base_qty, 6);
        assert_eq!(owners.owners[0], [2; BID_OWNER_BYTES]);
        assert_eq!(owners.owners[1], [3; BID_OWNER_BYTES]);
    }

    #[test]
    fn invalid_bid_fill_keeps_pages_unchanged() {
        let mut bids = BidPage::default();
        let mut owners = BidOwnerPage::default();
        bids.set_links(PageLinks::new(0, None, None));
        owners.set_links(PageLinks::new(0, None, None));
        insert_owned_bid(
            &mut bids,
            &mut owners,
            BidEntry {
                price_x64: hybrid_engine::Q64,
                sqrt_price_x64: hybrid_engine::Q64,
                base_qty: 5,
                sequence: 1,
            },
            [7; BID_OWNER_BYTES],
        )
        .unwrap();
        let before_bids = bids.clone();
        let before_owners = owners.clone();
        let bad = [hybrid_engine::ActiveBidFill {
            bid_index: 0,
            base_qty: 6,
            quote_qty: 6,
        }];
        assert!(apply_active_fills_to_bid_page(&mut bids, &mut owners, &bad).is_err());
        assert_eq!(bids, before_bids);
        assert_eq!(owners, before_owners);
    }
}
