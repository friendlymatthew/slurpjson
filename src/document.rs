use crate::{Tape, TapeEntry, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug)]
pub struct Document<'src, 'tape> {
    source: &'src [u8],
    tape: &'tape Tape,
}

impl<'src, 'tape> Document<'src, 'tape> {
    pub const fn new(source: &'src [u8], tape: &'tape Tape) -> Self {
        Self { source, tape }
    }

    /// returns a handle to the first tape entry, which is expected to be the json root value.
    pub const fn root(&self) -> ValueRef<'_, 'src, 'tape> {
        ValueRef {
            document: self,
            tape_index: 0,
        }
    }

    pub const fn tape(&self) -> &'tape Tape {
        self.tape
    }

    pub const fn source(&self) -> &'src [u8] {
        self.source
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ValueRef<'doc, 'src, 'tape> {
    document: &'doc Document<'src, 'tape>,
    tape_index: usize,
}

impl<'doc, 'src, 'tape> ValueRef<'doc, 'src, 'tape> {
    pub const fn document(&self) -> &'doc Document<'src, 'tape> {
        self.document
    }

    pub const fn tape_index(&self) -> usize {
        self.tape_index
    }

    pub fn kind(&self) -> ValueKind {
        match self.entry().token_kind() {
            TokenKind::LeftBrace => ValueKind::Object,
            TokenKind::LeftBracket => ValueKind::Array,
            TokenKind::String => ValueKind::String,
            TokenKind::Number => ValueKind::Number,
            TokenKind::True | TokenKind::False => ValueKind::Bool,
            TokenKind::Null => ValueKind::Null,
            foreign => panic!("found {foreign:?} token kind for Value"),
        }
    }

    pub fn raw(&self) -> &'src [u8] {
        let start = self.entry().byte_pos();
        let end = self.byte_end();

        self.document
            .source()
            .get(start..end)
            .expect("value byte range must be in source")
    }

    /// returns raw key/value entries when this value is a json object.
    pub fn object_entries(&self) -> Option<ObjectEntryIter<'doc, 'src, 'tape>> {
        (self.entry().token_kind() == TokenKind::LeftBrace).then_some(ObjectEntryIter {
            document: self.document,
            parent_index: self.tape_index,
            next_index: self.tape_index + 1,
        })
    }

    /// returns key entries when this value is a json object
    pub fn object_keys(&self) -> Option<impl Iterator<Item = &'src str>> {
        self.object_entries().map(|i| i.map(|(k, _)| k))
    }

    /// returns raw values when this value is a json array.
    pub fn array_elements(&self) -> Option<ArrayElementIter<'doc, 'src, 'tape>> {
        (self.entry().token_kind() == TokenKind::LeftBracket).then_some(ArrayElementIter {
            document: self.document,
            parent_index: self.tape_index,
            next_index: self.tape_index + 1,
        })
    }

    fn entry(&self) -> &TapeEntry {
        &self.document.tape().as_slice()[self.tape_index]
    }

    fn byte_end(&self) -> usize {
        let entry = self.entry();

        close_kind(entry.token_kind()).map_or_else(
            || entry.byte_end(),
            |close_kind| {
                self.matching_close(close_kind)
                    .expect("compound value must have a closing token")
                    .byte_end()
            },
        )
    }

    fn matching_close(&self, close_kind: TokenKind) -> Option<&TapeEntry> {
        self.document
            .tape()
            .as_slice()
            .iter()
            .skip(self.tape_index + 1)
            .find(|entry| {
                entry.parent_index() == Some(self.tape_index) && entry.token_kind() == close_kind
            })
    }

    // todo: we can speculatively parse out values but this is not the point of the project lmao
    // we'd have things like parse Number { Float, BigInt, Int, whatever }
    // pub fn try_as_str(&self) -> Result<&str> {
    //     ensure!(matches!(self.kind(), ValueKind::String), "expected string");

    //     let b = self.raw();
    //     str::from_utf8(&b[1..b.len() - 1]).map_err(|e| anyhow!("{:?}", e))
    // }
}

trait ComplexEntryIter<'doc, 'src: 'doc, 'tape: 'doc> {
    fn document(&self) -> &'doc Document<'src, 'tape>;
    fn parent_index(&self) -> usize;
    fn next_index(&self) -> usize;
    fn set_next_index(&mut self, next_index: usize);

    fn next_entry(&mut self) -> Option<(usize, &'doc TapeEntry)> {
        while let Some((index, entry)) = self
            .document()
            .tape()
            .as_slice()
            .iter()
            .enumerate()
            .nth(self.next_index())
        {
            self.set_next_index(index + 1);

            if entry.parent_index() == Some(self.parent_index()) {
                return Some((index, entry));
            }
        }

        None
    }

    fn value_ref(&self, tape_index: usize) -> ValueRef<'doc, 'src, 'tape> {
        ValueRef {
            document: self.document(),
            tape_index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectEntryIter<'doc, 'src, 'tape> {
    document: &'doc Document<'src, 'tape>,
    parent_index: usize,
    next_index: usize,
}

impl<'doc, 'src, 'tape> ComplexEntryIter<'doc, 'src, 'tape> for ObjectEntryIter<'doc, 'src, 'tape> {
    fn document(&self) -> &'doc Document<'src, 'tape> {
        self.document
    }

    fn parent_index(&self) -> usize {
        self.parent_index
    }

    fn next_index(&self) -> usize {
        self.next_index
    }

    fn set_next_index(&mut self, next_index: usize) {
        self.next_index = next_index;
    }
}

impl<'doc, 'src, 'tape> Iterator for ObjectEntryIter<'doc, 'src, 'tape> {
    type Item = (&'src str, ValueRef<'doc, 'src, 'tape>);

    fn next(&mut self) -> Option<Self::Item> {
        let key_index = self.find_next_key()?;
        let value_index = self.find_next_value(key_index + 1)?;

        self.next_index = value_index + 1;

        let key = {
            let key = self.value_ref(key_index).raw();
            debug_assert!(key.len() > 2, "keys should not be empty");

            str::from_utf8(&key[1..key.len() - 1]).unwrap()
        };

        Some((key, self.value_ref(value_index)))
    }
}

impl<'doc, 'src, 'tape> ObjectEntryIter<'doc, 'src, 'tape> {
    fn find_next_key(&mut self) -> Option<usize> {
        while let Some((index, entry)) = self.next_entry() {
            if entry.token_kind() == TokenKind::RightBrace {
                return None;
            }

            if entry.token_kind() == TokenKind::String {
                return Some(index);
            }
        }

        None
    }

    fn find_next_value(&self, start_index: usize) -> Option<usize> {
        self.document
            .tape()
            .as_slice()
            .iter()
            .enumerate()
            .skip(start_index)
            .find_map(|(index, entry)| {
                if entry.parent_index() != Some(self.parent_index) {
                    return None;
                }

                if entry.token_kind() == TokenKind::RightBrace {
                    return None;
                }

                if is_value_start(entry.token_kind()) {
                    return Some(index);
                }

                None
            })
    }
}

#[derive(Debug, Clone)]
pub struct ArrayElementIter<'doc, 'src, 'tape> {
    document: &'doc Document<'src, 'tape>,
    parent_index: usize,
    next_index: usize,
}

impl<'doc, 'src, 'tape> ComplexEntryIter<'doc, 'src, 'tape>
    for ArrayElementIter<'doc, 'src, 'tape>
{
    fn document(&self) -> &'doc Document<'src, 'tape> {
        self.document
    }

    fn parent_index(&self) -> usize {
        self.parent_index
    }

    fn next_index(&self) -> usize {
        self.next_index
    }

    fn set_next_index(&mut self, next_index: usize) {
        self.next_index = next_index;
    }
}

impl<'doc, 'src, 'tape> Iterator for ArrayElementIter<'doc, 'src, 'tape> {
    type Item = ValueRef<'doc, 'src, 'tape>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, entry)) = self.next_entry() {
            if entry.token_kind() == TokenKind::RightBracket {
                return None;
            }

            if is_value_start(entry.token_kind()) {
                return Some(self.value_ref(index));
            }
        }

        None
    }
}

const fn close_kind(kind: TokenKind) -> Option<TokenKind> {
    match kind {
        TokenKind::LeftBrace => Some(TokenKind::RightBrace),
        TokenKind::LeftBracket => Some(TokenKind::RightBracket),
        _ => None,
    }
}

const fn is_value_start(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LeftBrace
            | TokenKind::LeftBracket
            | TokenKind::String
            | TokenKind::Number
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
    )
}

#[cfg(test)]
mod tests {
    use crate::Parser;

    use super::*;

    #[test]
    fn test_raw() {
        let source = br#"{"foo":"bar"}"#;

        let p = Parser::try_new().unwrap();
        let t = p.parse_bytes(source).unwrap();

        let document = Document::new(source, &t);

        assert_eq!(document.root().raw(), br#"{"foo":"bar"}"#);
    }

    #[test]
    fn test_object_entries() {
        let source = br#"{"foo":"bar"}"#;

        let p = Parser::try_new().unwrap();
        let t = p.parse_bytes(source).unwrap();

        let document = Document::new(source, &t);

        let entries = document
            .root()
            .object_entries()
            .unwrap()
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 1);

        let (key, value) = entries[0];

        assert!(matches!(value.kind(), ValueKind::String));

        assert_eq!(key, "foo");
        assert_eq!(value.raw(), br#""bar""#);
    }

    #[test]
    fn test_array_elements() {
        let source = br#"[1,"a",null]"#;

        let p = Parser::try_new().unwrap();
        let t = p.parse_bytes(source).unwrap();

        let document = Document::new(source, &t);

        let array_elements = document
            .root()
            .array_elements()
            .unwrap()
            .collect::<Vec<_>>();

        assert_eq!(array_elements.len(), 3);

        assert_eq!(array_elements[0].raw(), b"1");
        assert_eq!(array_elements[1].raw(), br#""a""#);
        assert_eq!(array_elements[2].raw(), b"null");

        let entries = document.root().object_entries();
        assert!(entries.is_none());
    }

    #[test]
    fn test_nested() {
        let source = br#"
            {
                "foo": "bar",
                "baz": [
                    1, 2, 3, 67
                ],
                "wef": {
                    "yearn": 5
                }
            } 
        "#;

        let p = Parser::try_new().unwrap();
        let t = p.parse_bytes(source).unwrap();

        let document = Document::new(source, &t);

        let root = document.root();

        let object1 = root
            .object_entries()
            .expect("is object")
            .collect::<Vec<_>>();

        assert_eq!(object1.len(), 3);

        assert_eq!(object1[0].0, "foo");
        assert_eq!(object1[1].0, "baz");
        assert_eq!(object1[2].0, "wef");

        let object1_value_kinds = object1.iter().map(|(_, v)| v.kind()).collect::<Vec<_>>();
        assert_eq!(object1_value_kinds[0], ValueKind::String);
        assert_eq!(object1_value_kinds[1], ValueKind::Array);
        assert_eq!(object1_value_kinds[2], ValueKind::Object);

        let mut array2 = object1[1].1.array_elements().expect("is array");
        assert_eq!(array2.next().map(|v| v.raw()), Some(b"1".as_slice()));
        assert_eq!(array2.next().map(|v| v.raw()), Some(b"2".as_slice()));
        assert_eq!(array2.next().map(|v| v.raw()), Some(b"3".as_slice()));
        assert_eq!(array2.next().map(|v| v.raw()), Some(b"67".as_slice()));
        assert_eq!(array2.next().map(|v| v.raw()), None);

        let mut object2 = object1[2].1.object_entries().expect("is object");
        let Some((k, v)) = object2.next() else {
            panic!("expected 1 entry")
        };

        assert_eq!(k, "yearn");
        assert_eq!(v.raw(), b"5".as_slice());
        assert!(object2.next().is_none());
    }
}
