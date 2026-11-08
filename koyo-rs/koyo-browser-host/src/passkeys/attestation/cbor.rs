//! A reader for the CBOR an authenticator writes (RFC 8949). Definite lengths only, as
//! `WebAuthn`'s CTAP2 canonical form has them; every length is checked against what's left and
//! nesting is held to a few levels, so malformed input ends in `None` and never loops long.

/// How deep items may nest; an attestation object nests three deep.
const DEPTH: usize = 8;

/// One item's head: a number or a string whole, or what an array, a map or a tag holds still
/// to come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item<'a> {
    /// An integer that fits an `i64`.
    Int(i64),
    /// A byte string.
    Bytes(&'a [u8]),
    /// A text string, as its UTF-8 bytes.
    Text(&'a [u8]),
    /// An array of so many items, which follow.
    Array(usize),
    /// A map of so many pairs, which follow, each key before its value.
    Map(usize),
    /// A tag, whose item follows.
    Tag,
    /// Anything else: a simple value, a float, an integer too big for an `i64`.
    Other,
}

/// Reads items off the front of the input.
pub(super) struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    /// A reader at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { rest: data }
    }

    /// The next item's head; an array's or a map's items, and a tag's, are read after it.
    pub fn item(&mut self) -> Option<Item<'a>> {
        let (major, argument) = self.head()?;
        let item = match major {
            0 => i64::try_from(argument).map_or(Item::Other, Item::Int),
            // -1 - n, which fits an `i64` whenever n does.
            1 => i64::try_from(argument).map_or(Item::Other, |n| Item::Int(-1 - n)),
            2 => Item::Bytes(self.take(usize::try_from(argument).ok()?)?),
            3 => Item::Text(self.take(usize::try_from(argument).ok()?)?),
            // Every item takes a byte at least, so a count over what's left is malformed.
            4 => Item::Array(self.count(argument, 1)?),
            5 => Item::Map(self.count(argument, 2)?),
            6 => Item::Tag,
            _ => Item::Other,
        };
        Some(item)
    }

    /// The next item whole: an array, a map or a tag with all it holds read past.
    pub fn whole(&mut self) -> Option<Item<'a>> {
        let item = self.item()?;
        self.skip_contents(item, DEPTH)?;
        Some(item)
    }

    /// Reads past what `item` holds, `depth` levels down at most.
    fn skip_contents(&mut self, item: Item<'a>, depth: usize) -> Option<()> {
        let items = match item {
            Item::Array(count) => count,
            Item::Map(pairs) => pairs.checked_mul(2)?,
            Item::Tag => 1,
            _ => return Some(()),
        };
        let below = depth.checked_sub(1)?;
        for _ in 0..items {
            let inner = self.item()?;
            self.skip_contents(inner, below)?;
        }
        Some(())
    }

    /// An item's major type and argument: its value, length or count.
    fn head(&mut self) -> Option<(u8, u64)> {
        let first = *self.take(1)?.first()?;
        let argument = match first & 0x1f {
            short @ 0..=23 => u64::from(short),
            24 => u64::from(u8::from_be_bytes(self.array()?)),
            25 => u64::from(u16::from_be_bytes(self.array()?)),
            26 => u64::from(u32::from_be_bytes(self.array()?)),
            27 => u64::from_be_bytes(self.array()?),
            // Reserved, or an indefinite length, which canonical CBOR never has.
            _ => return None,
        };
        Some((first >> 5, argument))
    }

    /// `count` items of `each` items apiece, when what's left could hold them.
    fn count(&self, count: u64, each: usize) -> Option<usize> {
        let count = usize::try_from(count).ok()?;
        (count.checked_mul(each)? <= self.rest.len()).then_some(count)
    }

    /// The next `N` bytes.
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    /// The next `length` bytes.
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.rest.split_at_checked(length)?;
        self.rest = rest;
        Some(taken)
    }
}
