/// PersistentMap: a persistent (structural-sharing) hash map.
///
/// Will be backed by im::HashMap for O(1) clone via structural sharing,
/// with O(log n) get/insert/remove.
use std::hash::Hash;

/// A persistent hash map with structural sharing on clone.
pub struct PersistentMap<K: Clone + Hash + Eq, V: Clone> {
    _phantom: std::marker::PhantomData<(K, V)>,
}

impl<K: Clone + Hash + Eq, V: Clone> PersistentMap<K, V> {
    pub fn new() -> Self {
        todo!()
    }

    pub fn get(&self, _key: &K) -> Option<&V> {
        todo!()
    }

    pub fn insert(&mut self, _key: K, _value: V) {
        todo!()
    }

    pub fn insert_functional(&self, _key: K, _value: V) -> Self {
        todo!()
    }

    pub fn remove(&mut self, _key: &K) {
        todo!()
    }

    pub fn contains_key(&self, _key: &K) -> bool {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        todo!()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        std::iter::empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        std::iter::empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        std::iter::empty()
    }
}

impl<K: Clone + Hash + Eq, V: Clone> Default for PersistentMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq + std::fmt::Debug, V: Clone + std::fmt::Debug> std::fmt::Debug
    for PersistentMap<K, V>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentMap").finish()
    }
}

impl<K: Clone + Hash + Eq, V: Clone + PartialEq> PartialEq for PersistentMap<K, V> {
    fn eq(&self, _other: &Self) -> bool {
        todo!()
    }
}

impl<K: Clone + Hash + Eq, V: Clone> Clone for PersistentMap<K, V> {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<K: Clone + Hash + Eq, V: Clone> FromIterator<(K, V)> for PersistentMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(_iter: I) -> Self {
        todo!()
    }
}

impl<K: Clone + Hash + Eq, V: Clone> IntoIterator for PersistentMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        todo!()
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
        assert_eq!(
            pairs,
            vec![("a".to_string(), 1), ("b".to_string(), 2)]
        );
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
        assert_eq!(
            pairs,
            vec![("a".to_string(), 1), ("b".to_string(), 2)]
        );
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
}
