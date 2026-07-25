#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TokenKind {
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    String,
    Number,
    True,
    False,
    Null,
    Invalid,
    Unknown(u32),
}

impl TokenKind {
    pub const fn is_open(self) -> bool {
        matches!(self, Self::LeftBrace | Self::LeftBracket)
    }

    pub const fn is_close(self) -> bool {
        matches!(self, Self::RightBrace | Self::RightBracket)
    }

    pub const fn is_separator(self) -> bool {
        matches!(self, Self::Colon | Self::Comma)
    }

    pub const fn is_scalar(self) -> bool {
        matches!(
            self,
            Self::String | Self::Number | Self::True | Self::False | Self::Null | Self::Invalid
        )
    }

    pub const fn as_byte(self) -> Option<u8> {
        match self {
            Self::LeftBrace => Some(b'{'),
            Self::RightBrace => Some(b'}'),
            Self::LeftBracket => Some(b'['),
            Self::RightBracket => Some(b']'),
            Self::Colon => Some(b':'),
            Self::Comma => Some(b','),
            Self::String | Self::Number | Self::True | Self::False | Self::Null | Self::Invalid => {
                None
            }
            Self::Unknown(_) => None,
        }
    }
}

impl From<u32> for TokenKind {
    fn from(raw: u32) -> Self {
        match raw {
            1 => Self::LeftBrace,
            2 => Self::RightBrace,
            3 => Self::LeftBracket,
            4 => Self::RightBracket,
            5 => Self::Colon,
            6 => Self::Comma,
            7 => Self::String,
            8 => Self::Number,
            9 => Self::True,
            10 => Self::False,
            11 => Self::Null,
            12 => Self::Invalid,
            _ => Self::Unknown(raw),
        }
    }
}

impl From<TokenKind> for u32 {
    fn from(kind: TokenKind) -> Self {
        match kind {
            TokenKind::LeftBrace => 1,
            TokenKind::RightBrace => 2,
            TokenKind::LeftBracket => 3,
            TokenKind::RightBracket => 4,
            TokenKind::Colon => 5,
            TokenKind::Comma => 6,
            TokenKind::String => 7,
            TokenKind::Number => 8,
            TokenKind::True => 9,
            TokenKind::False => 10,
            TokenKind::Null => 11,
            TokenKind::Invalid => 12,
            TokenKind::Unknown(raw) => raw,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TapeEntry {
    byte_pos: u32,
    byte_end: u32,
    depth: i32,
    parent: i32,
    token_kind: u32,
}

impl TapeEntry {
    pub fn new(
        byte_pos: u32,
        byte_end: u32,
        depth: i32,
        parent: i32,
        token_kind: TokenKind,
    ) -> Self {
        Self {
            byte_pos,
            byte_end,
            depth,
            parent,
            token_kind: token_kind.into(),
        }
    }

    pub fn token_kind(&self) -> TokenKind {
        TokenKind::from(self.token_kind)
    }

    pub fn byte_pos(&self) -> usize {
        usize::try_from(self.byte_pos).expect("byte position must fit in usize")
    }

    pub fn byte_end(&self) -> usize {
        usize::try_from(self.byte_end).expect("byte end must fit in usize")
    }

    pub fn byte_range(&self) -> std::ops::Range<usize> {
        self.byte_pos()..self.byte_end()
    }

    pub fn source<'a>(&self, json: &'a [u8]) -> Option<&'a [u8]> {
        json.get(self.byte_range())
    }

    pub const fn depth(&self) -> i32 {
        self.depth
    }

    pub const fn parent(&self) -> i32 {
        self.parent
    }

    pub fn parent_index(&self) -> Option<usize> {
        usize::try_from(self.parent).ok()
    }
}

impl std::fmt::Debug for TapeEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapeEntry")
            .field("byte_pos", &self.byte_pos)
            .field("byte_end", &self.byte_end)
            .field("depth", &self.depth)
            .field("parent", &self.parent)
            .field("token_kind", &self.token_kind())
            .finish()
    }
}

// note: should this be a clone?
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tape {
    entries: Vec<TapeEntry>,
}

impl Tape {
    pub const fn new(entries: Vec<TapeEntry>) -> Self {
        Self { entries }
    }

    pub fn iter(&self) -> TapeIter<'_> {
        TapeIter {
            inner: self.entries.iter(),
            current_depth: 0,
        }
    }

    pub fn as_slice(&self) -> &[TapeEntry] {
        &self.entries
    }

    pub fn entries(&self) -> &[TapeEntry] {
        &self.entries
    }

    pub fn into_entries(self) -> Vec<TapeEntry> {
        self.entries
    }

    pub fn get(&self, index: usize) -> Option<&TapeEntry> {
        self.entries.get(index)
    }

    pub fn root(&self) -> Option<&TapeEntry> {
        self.entries.first()
    }

    pub fn parent_of(&self, index: usize) -> Option<&TapeEntry> {
        let parent_index = self.entries.get(index)?.parent_index()?;

        self.entries.get(parent_index)
    }

    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<Vec<TapeEntry>> for Tape {
    fn from(tape: Vec<TapeEntry>) -> Self {
        Self::new(tape)
    }
}

impl FromIterator<TapeEntry> for Tape {
    fn from_iter<T: IntoIterator<Item = TapeEntry>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<'a> IntoIterator for &'a Tape {
    type Item = &'a TapeEntry;
    type IntoIter = TapeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for Tape {
    type Item = TapeEntry;
    type IntoIter = std::vec::IntoIter<TapeEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[derive(Debug, Clone)]
pub struct TapeIter<'a> {
    inner: std::slice::Iter<'a, TapeEntry>,
    current_depth: usize,
}

impl TapeIter<'_> {
    pub const fn current_depth(&self) -> usize {
        self.current_depth
    }
}

impl<'a> Iterator for TapeIter<'a> {
    type Item = &'a TapeEntry;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.inner.next()?;

        self.current_depth = depth_to_usize(entry.depth);

        Some(entry)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for TapeIter<'_> {}

fn depth_to_usize(depth: i32) -> usize {
    usize::try_from(depth).unwrap_or_default()
}
