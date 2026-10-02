//! String interning and a small, fast non-cryptographic hasher.
//!
//! Every token text (identifiers, punctuators, numbers, whitespace runs and
//! comments) is interned once per [`crate::Preprocessor`], so tokens are small
//! `Copy` values and macro lookups are plain vector indexing.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

/// FxHash-style hasher (the multiply-rotate hash used by rustc). Not DoS
/// resistant, which is fine for interning pack sources.
#[derive(Default, Clone, Copy)]
pub(crate) struct FxHasher {
    hash: u64,
}

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
        // Mix in the length so zero padding cannot collide ("a" vs "a\0").
        self.add(bytes.len() as u64);
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub(crate) type FxBuild = BuildHasherDefault<FxHasher>;
pub(crate) type FxHashMap<K, V> = HashMap<K, V, FxBuild>;

/// An interned string id. Ids are dense, starting at 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub(crate) struct Sym(pub u32);

impl Sym {
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

/// Well-known symbols, interned first so their ids are constants.
pub(crate) mod known {
    use super::Sym;
    pub(crate) const EMPTY: Sym = Sym(0);
    pub(crate) const SPACE: Sym = Sym(1);
    pub(crate) const DEFINED: Sym = Sym(2);
    pub(crate) const LINE: Sym = Sym(3);
    pub(crate) const FILE: Sym = Sym(4);
    pub(crate) const VERSION: Sym = Sym(5);
    pub(crate) const VA_ARGS: Sym = Sym(6);
    pub(crate) const LAYOUT: Sym = Sym(7);
    pub(crate) const LPAREN: Sym = Sym(8);
    pub(crate) const RPAREN: Sym = Sym(9);
    pub(crate) const COMMA: Sym = Sym(10);
    pub(crate) const HASH: Sym = Sym(11);
    pub(crate) const HASHHASH: Sym = Sym(12);
    pub(crate) const ELLIPSIS: Sym = Sym(13);
    pub(crate) const ZERO: Sym = Sym(14);
    pub(crate) const ONE: Sym = Sym(15);
    pub(crate) const COLON: Sym = Sym(16);

    /// Texts of the well-known symbols, in id order.
    pub(crate) const TEXTS: [&str; 17] = [
        "",
        " ",
        "defined",
        "__LINE__",
        "__FILE__",
        "__VERSION__",
        "__VA_ARGS__",
        "layout",
        "(",
        ")",
        ",",
        "#",
        "##",
        "...",
        "0",
        "1",
        ":",
    ];
}

/// String interner. Strings are stored once (shared between the lookup map and
/// the id table through `Arc<str>`).
pub(crate) struct Interner {
    map: FxHashMap<Arc<str>, Sym>,
    strs: Vec<Arc<str>>,
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl Interner {
    pub(crate) fn new() -> Self {
        let mut i = Interner {
            map: FxHashMap::default(),
            strs: Vec::new(),
        };
        for (n, t) in known::TEXTS.iter().enumerate() {
            let s = i.intern(t);
            debug_assert_eq!(s.idx(), n);
        }
        i
    }

    #[inline]
    pub(crate) fn intern(&mut self, s: &str) -> Sym {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }
        let sym = Sym(u32::try_from(self.strs.len()).unwrap_or(u32::MAX));
        let a: Arc<str> = Arc::from(s);
        self.strs.push(a.clone());
        self.map.insert(a, sym);
        sym
    }

    #[inline]
    pub(crate) fn get(&self, sym: Sym) -> &str {
        self.strs.get(sym.idx()).map_or("", |s| s)
    }

    pub(crate) fn len(&self) -> usize {
        self.strs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_symbols_are_stable() {
        let mut i = Interner::new();
        assert_eq!(i.intern("defined"), known::DEFINED);
        assert_eq!(i.intern("__VA_ARGS__"), known::VA_ARGS);
        assert_eq!(i.intern("##"), known::HASHHASH);
        assert_eq!(i.get(known::LAYOUT), "layout");
        assert_eq!(i.len(), known::TEXTS.len());
    }

    #[test]
    fn interning_is_idempotent() {
        let mut i = Interner::new();
        let a = i.intern("hello");
        let b = i.intern("hello");
        assert_eq!(a, b);
        assert_eq!(i.get(a), "hello");
        assert_eq!(i.get(Sym(999_999)), "");
    }

    #[test]
    fn hasher_distinguishes_lengths() {
        use std::hash::BuildHasher;
        let b = FxBuild::default();
        assert_ne!(b.hash_one("a"), b.hash_one("a\0"));
        assert_ne!(b.hash_one("abcdefgh1"), b.hash_one("abcdefgh2"));
    }
}
