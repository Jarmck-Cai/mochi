//! Token interning: every LM/dictionary token becomes a `u32` id so the
//! n-gram tables key on packed integers instead of strings (memory + speed).

use rustc_hash::FxHashMap;

/// Sentinel id for tokens never seen at load time (query-time fallback
/// letters outside a-z, digits, ...). The LM treats it as OOV.
pub const UNK: u32 = u32::MAX;

#[derive(Default)]
pub struct Interner {
    map: FxHashMap<Box<str>, u32>,
    strings: Vec<Box<str>>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        let boxed: Box<str> = s.into();
        self.strings.push(boxed.clone());
        self.map.insert(boxed, id);
        id
    }

    #[allow(dead_code)] // query-time lookup; the personal layer (M2-3) uses it
    pub fn get(&self, s: &str) -> Option<u32> {
        self.map.get(s).copied()
    }

    #[allow(dead_code)]
    pub fn resolve(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Rough heap usage in bytes (strings + map/vec overhead).
    pub fn est_bytes(&self) -> usize {
        let chars: usize = self.strings.iter().map(|s| s.len()).sum();
        // each token stored twice (map key clone + vec) + map entry ~32B + vec ptr 16B
        2 * chars + self.strings.len() * 48
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_idempotent_and_resolves() {
        let mut i = Interner::new();
        let a = i.intern("你好");
        let b = i.intern("hello");
        assert_eq!(i.intern("你好"), a);
        assert_ne!(a, b);
        assert_eq!(i.resolve(a), "你好");
        assert_eq!(i.get("hello"), Some(b));
        assert_eq!(i.get("missing"), None);
        assert_eq!(i.len(), 2);
    }
}
