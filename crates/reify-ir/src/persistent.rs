/// PersistentMap: a persistent (structural-sharing) hash map backed by im::HashMap.
///
/// Provides O(1) clone via structural sharing, with O(log n) get/insert/remove.
/// Used internally by ValueMap for efficient snapshot cloning.
use im::HashMap as ImHashMap;
use std::fmt;
use std::hash::Hash;

/// A persistent hash map with structural sharing on clone.
///
/// Wraps `im::HashMap<K, V>` in a newtype to encapsulate the backing
/// implementation and expose a controlled API surface.
pub struct PersistentMap<K: Clone + Hash + Eq, V: Clone> {
    inner: ImHashMap<K, V>,
}

impl<K: Clone + Hash + Eq, V: Clone> PersistentMap<K, V> {
    /// Create an empty PersistentMap.
    pub fn new() -> Self {
        Self {
            inner: ImHashMap::new(),
        }
    }

    /// Look up a value by key.
    ///
    /// Accepts any borrowed form of the key: `get("name")` works on a
    /// `PersistentMap<String, V>` without allocating a temporary `String`,
    /// exactly as `std::collections::HashMap::get` does.  The bound
    /// `K: Borrow<Q>` is satisfied for `Q = K` by the standard blanket
    /// `impl<T: ?Sized> Borrow<T> for T`, so all existing `&K` / `&String`
    /// callers continue to compile unchanged.
    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.inner.get(k)
    }

    /// Look up a mutable reference to a value by key.
    ///
    /// Uses `im::HashMap`'s copy-on-write semantics: if the underlying trie
    /// node is shared with another (cloned) map, it is cloned before the
    /// mutable borrow is returned, preserving structural-sharing invariants
    /// for siblings.
    ///
    /// Accepts any borrowed form of the key (e.g. `&str` on a
    /// `PersistentMap<String, V>`) via the same `K: Borrow<Q>` bound as
    /// `get`.  All existing `&K` / `&String` callers continue to compile
    /// unchanged (Q=K via the blanket `impl<T:?Sized> Borrow<T> for T`).
    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.inner.get_mut(k)
    }

    /// Insert a key-value pair, mutating in place (but sharing structure on clone).
    pub fn insert(&mut self, key: K, value: V) {
        self.inner.insert(key, value);
    }

    /// Functional insert: returns a new map with the key-value pair added.
    /// The original map is not modified.
    pub fn insert_functional(&self, key: K, value: V) -> Self {
        Self {
            inner: self.inner.update(key, value),
        }
    }

    /// Remove a key, mutating in place.
    pub fn remove(&mut self, key: &K) {
        self.inner.remove(key);
    }

    /// Check if the map contains a key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Is the map empty?
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate over key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.inner.iter()
    }

    /// Iterate over keys.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }

    /// Iterate over values.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }
}

impl<K: Clone + Hash + Eq, V: Clone> Default for PersistentMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq + fmt::Debug, V: Clone + fmt::Debug> fmt::Debug for PersistentMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.inner.iter()).finish()
    }
}

impl<K: Clone + Hash + Eq, V: Clone + PartialEq> PartialEq for PersistentMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<K: Clone + Hash + Eq, V: Clone> Clone for PersistentMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K: Clone + Hash + Eq, V: Clone> FromIterator<(K, V)> for PersistentMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl<K: Clone + Hash + Eq, V: Clone> IntoIterator for PersistentMap<K, V> {
    type Item = (K, V);
    type IntoIter = im::hashmap::ConsumingIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_map() {
        let map: PersistentMap<String, i32> = PersistentMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn default_creates_empty_map() {
        let map: PersistentMap<String, i32> = PersistentMap::default();
        assert!(map.is_empty());
    }

    #[test]
    fn insert_and_get_round_trip() {
        let mut map = PersistentMap::new();
        map.insert("key".to_string(), 42);
        assert_eq!(map.get(&"key".to_string()), Some(&42));
        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
    }

    #[test]
    fn insert_functional_does_not_mutate_original() {
        let mut map = PersistentMap::new();
        map.insert("a".to_string(), 1);

        let map2 = map.insert_functional("b".to_string(), 2);

        // Original unchanged
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&"a".to_string()), Some(&1));
        assert_eq!(map.get(&"b".to_string()), None);

        // New map has both
        assert_eq!(map2.len(), 2);
        assert_eq!(map2.get(&"a".to_string()), Some(&1));
        assert_eq!(map2.get(&"b".to_string()), Some(&2));
    }

    #[test]
    fn contains_key() {
        let mut map = PersistentMap::new();
        map.insert("x".to_string(), 10);
        assert!(map.contains_key(&"x".to_string()));
        assert!(!map.contains_key(&"y".to_string()));
    }

    #[test]
    fn remove() {
        let mut map = PersistentMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        assert_eq!(map.len(), 2);

        map.remove(&"a".to_string());
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&"a".to_string()), None);
        assert_eq!(map.get(&"b".to_string()), Some(&2));
    }

    #[test]
    fn keys_values_iter() {
        let mut map = PersistentMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);

        let mut keys: Vec<_> = map.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);

        let mut values: Vec<_> = map.values().cloned().collect();
        values.sort();
        assert_eq!(values, vec![1, 2]);

        let mut pairs: Vec<_> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
        pairs.sort();
        assert_eq!(pairs, vec![("a".to_string(), 1), ("b".to_string(), 2)]);
    }

    #[test]
    fn from_iterator() {
        let map: PersistentMap<String, i32> = vec![("a".to_string(), 1), ("b".to_string(), 2)]
            .into_iter()
            .collect();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"a".to_string()), Some(&1));
    }

    #[test]
    fn into_iterator() {
        let mut map = PersistentMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);

        let mut pairs: Vec<_> = map.into_iter().collect();
        pairs.sort();
        assert_eq!(pairs, vec![("a".to_string(), 1), ("b".to_string(), 2)]);
    }

    #[test]
    fn clone_structural_sharing() {
        let mut original = PersistentMap::new();
        original.insert("a".to_string(), 1);
        original.insert("b".to_string(), 2);

        let mut cloned = original.clone();
        cloned.insert("c".to_string(), 3);

        // Original is unmodified
        assert_eq!(original.len(), 2);
        assert_eq!(original.get(&"c".to_string()), None);

        // Clone has the new entry
        assert_eq!(cloned.len(), 3);
        assert_eq!(cloned.get(&"c".to_string()), Some(&3));
    }

    #[test]
    fn debug_impl() {
        let mut map = PersistentMap::new();
        map.insert("key".to_string(), 42);
        let debug_str = format!("{:?}", map);
        assert!(debug_str.contains("key"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn get_mut_returns_mutable_reference() {
        let mut map = PersistentMap::new();
        map.insert("key".to_string(), 42);
        {
            let val = map.get_mut(&"key".to_string()).unwrap();
            *val = 99;
        }
        assert_eq!(map.get(&"key".to_string()), Some(&99));
    }

    #[test]
    fn get_mut_missing_key_returns_none() {
        let mut map: PersistentMap<String, i32> = PersistentMap::new();
        assert!(map.get_mut(&"missing".to_string()).is_none());
    }

    #[test]
    fn get_mut_cow_semantics_sibling_clone_unaffected() {
        // Verify the copy-on-write property documented on `get_mut`:
        // mutating the original through `get_mut` must not affect a sibling
        // clone that was taken before the mutation.
        let mut original = PersistentMap::new();
        original.insert("key".to_string(), 10i32);

        // Clone shares structure with original at this point.
        let sibling = original.clone();

        // Mutate original in-place — im::HashMap clones the shared trie node
        // before returning the mutable borrow, so sibling is unaffected.
        *original.get_mut(&"key".to_string()).unwrap() = 99;

        assert_eq!(original.get(&"key".to_string()), Some(&99));
        assert_eq!(sibling.get(&"key".to_string()), Some(&10));
    }

    #[test]
    fn partial_eq_impl() {
        let mut map1 = PersistentMap::new();
        map1.insert("a".to_string(), 1);

        let mut map2 = PersistentMap::new();
        map2.insert("a".to_string(), 1);

        let mut map3 = PersistentMap::new();
        map3.insert("a".to_string(), 2);

        assert_eq!(map1, map2);
        assert_ne!(map1, map3);
    }

    #[test]
    fn get_mut_accepts_borrowed_str_key() {
        // Verifies the Borrow-generic `get_mut<Q>` overload: a bare `&str` must
        // be accepted as a key without any `to_string()` allocation.
        let mut map: PersistentMap<String, i32> = PersistentMap::new();
        map.insert("key".to_string(), 42);
        *map.get_mut("key").unwrap() = 99;
        assert_eq!(map.get("key"), Some(&99));
        assert!(map.get_mut("missing").is_none());
    }

    #[test]
    fn get_accepts_borrowed_str_key() {
        // Verifies the Borrow-generic `get<Q>` overload: a bare `&str` must be
        // accepted as a key into a `PersistentMap<String, i32>` without any
        // intermediate `to_string()` allocation.
        let mut map: PersistentMap<String, i32> = PersistentMap::new();
        map.insert("key".to_string(), 42);
        assert_eq!(map.get("key"), Some(&42));
        assert_eq!(map.get("missing"), None);
    }

    #[test]
    fn works_with_value_cell_id_and_value() {
        use reify_core::identity::ValueCellId;
        use crate::value::Value;

        let mut map: PersistentMap<ValueCellId, Value> = PersistentMap::new();
        let id_width = ValueCellId::new("Bracket", "width");
        let id_height = ValueCellId::new("Bracket", "height");

        map.insert(id_width.clone(), Value::length(0.08));
        map.insert(id_height.clone(), Value::length(0.10));

        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&id_width));
        assert!(map.contains_key(&id_height));

        // Verify get returns the correct values
        match map.get(&id_width) {
            Some(Value::Scalar { si_value, .. }) => assert!((si_value - 0.08).abs() < 1e-10),
            other => panic!("Expected Scalar, got {:?}", other),
        }

        // Verify clone + insert doesn't affect original
        let mut cloned = map.clone();
        let id_depth = ValueCellId::new("Bracket", "depth");
        cloned.insert(id_depth.clone(), Value::length(0.05));

        assert_eq!(map.len(), 2);
        assert_eq!(cloned.len(), 3);
        assert!(!map.contains_key(&id_depth));
        assert!(cloned.contains_key(&id_depth));
    }
}
