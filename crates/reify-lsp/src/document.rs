use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::Url;

/// State of a single open document.
pub struct DocumentState {
    pub text: String,
    pub version: i32,
}

/// Stores open document contents, keyed by URI.
#[derive(Default)]
pub struct DocumentStore {
    documents: HashMap<Url, DocumentState>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Url, text: String, version: i32) {
        self.documents.insert(uri, DocumentState { text, version });
    }

    pub fn update(&mut self, uri: &Url, text: String, version: i32) -> bool {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.text = text;
            doc.version = version;
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Url, &DocumentState)> {
        self.documents.iter()
    }

    /// Return a snapshot of all open documents keyed by filesystem path.
    ///
    /// Non-file URIs (e.g. `untitled:`) are silently skipped because they
    /// have no meaningful [`PathBuf`] representation.
    pub fn snapshot_as_path_map(&self) -> HashMap<PathBuf, String> {
        self.documents
            .iter()
            .filter_map(|(uri, doc)| uri.to_file_path().ok().map(|p| (p, doc.text.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri(name: &str) -> Url {
        Url::parse(&format!("file:///{name}.ri")).unwrap()
    }

    #[test]
    fn open_document_stores_text_and_uri() {
        let mut store = DocumentStore::new();
        let uri = test_uri("test");
        store.open(uri.clone(), "hello".to_string(), 1);
        let doc = store.get(&uri).expect("document should exist");
        assert_eq!(doc.text, "hello");
        assert_eq!(doc.version, 1);
    }

    #[test]
    fn get_document_returns_text_after_open() {
        let mut store = DocumentStore::new();
        let uri = test_uri("foo");
        store.open(uri.clone(), "content".to_string(), 0);
        assert!(store.get(&uri).is_some());
    }

    #[test]
    fn update_document_replaces_text() {
        let mut store = DocumentStore::new();
        let uri = test_uri("bar");
        store.open(uri.clone(), "original".to_string(), 1);
        store.update(&uri, "updated".to_string(), 2);
        let doc = store.get(&uri).unwrap();
        assert_eq!(doc.text, "updated");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn close_document_removes_entry() {
        let mut store = DocumentStore::new();
        let uri = test_uri("baz");
        store.open(uri.clone(), "text".to_string(), 1);
        store.close(&uri);
        assert!(store.get(&uri).is_none());
    }

    #[test]
    fn get_returns_none_after_close() {
        let mut store = DocumentStore::new();
        let uri = test_uri("qux");
        store.open(uri.clone(), "text".to_string(), 1);
        store.close(&uri);
        assert!(store.get(&uri).is_none());
    }

    #[test]
    fn update_unopened_document_returns_false() {
        let mut store = DocumentStore::new();
        let uri = test_uri("unknown");
        let found = store.update(&uri, "text".to_string(), 1);
        assert!(!found, "update on unopened URI should return false");
    }

    #[test]
    fn update_opened_document_returns_true() {
        let mut store = DocumentStore::new();
        let uri = test_uri("known");
        store.open(uri.clone(), "original".to_string(), 1);
        let found = store.update(&uri, "updated".to_string(), 2);
        assert!(found, "update on opened URI should return true");
    }

    #[test]
    fn open_two_documents_independently() {
        let mut store = DocumentStore::new();
        let uri_a = test_uri("a");
        let uri_b = test_uri("b");
        store.open(uri_a.clone(), "aaa".to_string(), 1);
        store.open(uri_b.clone(), "bbb".to_string(), 2);
        assert_eq!(store.get(&uri_a).unwrap().text, "aaa");
        assert_eq!(store.get(&uri_b).unwrap().text, "bbb");
    }

    #[test]
    fn snapshot_as_path_map_returns_path_keyed_entries() {
        let mut store = DocumentStore::new();
        let uri_a = test_uri("alpha");
        let uri_b = test_uri("beta");
        store.open(uri_a.clone(), "aaa".to_string(), 1);
        store.open(uri_b.clone(), "bbb".to_string(), 2);

        let map = store.snapshot_as_path_map();

        assert_eq!(map.len(), 2);
        let path_a = uri_a.to_file_path().unwrap();
        let path_b = uri_b.to_file_path().unwrap();
        assert_eq!(map.get(&path_a).unwrap(), "aaa");
        assert_eq!(map.get(&path_b).unwrap(), "bbb");
    }

    #[test]
    fn snapshot_as_path_map_skips_non_file_uris() {
        let mut store = DocumentStore::new();
        let file_uri = test_uri("real");
        let non_file_uri = Url::parse("untitled:Untitled-1").unwrap();
        store.open(file_uri.clone(), "file_content".to_string(), 1);
        store.open(non_file_uri, "untitled_content".to_string(), 2);

        let map = store.snapshot_as_path_map();

        assert_eq!(map.len(), 1);
        let path = file_uri.to_file_path().unwrap();
        assert_eq!(map.get(&path).unwrap(), "file_content");
    }

    #[test]
    fn snapshot_as_path_map_empty_store_returns_empty() {
        let store = DocumentStore::new();
        let map = store.snapshot_as_path_map();
        assert!(map.is_empty());
    }

    #[test]
    fn iter_returns_all_open_documents() {
        let mut store = DocumentStore::new();
        let uri_a = test_uri("alpha");
        let uri_b = test_uri("beta");
        let uri_c = test_uri("gamma");
        store.open(uri_a.clone(), "aaa".to_string(), 1);
        store.open(uri_b.clone(), "bbb".to_string(), 2);
        store.open(uri_c.clone(), "ccc".to_string(), 3);

        let items: std::collections::HashMap<Url, String> = store
            .iter()
            .map(|(uri, doc)| (uri.clone(), doc.text.clone()))
            .collect();

        assert_eq!(items.len(), 3);
        assert_eq!(items.get(&uri_a).unwrap(), "aaa");
        assert_eq!(items.get(&uri_b).unwrap(), "bbb");
        assert_eq!(items.get(&uri_c).unwrap(), "ccc");
    }

    // --- step-01: per-document parse cache ---

    /// Two `parsed_module` calls on the same `DocumentState` return the SAME
    /// `Arc` allocation — proving the parse is memoized (no re-parse) within a
    /// single document version. `Arc::ptr_eq` is a deterministic stand-in for
    /// "the parse ran only once".
    #[test]
    fn parsed_module_caches_parse() {
        let mut store = DocumentStore::new();
        let uri = test_uri("cache");
        let source = "structure A {\n    param x: Scalar = 1mm\n}";
        store.open(uri.clone(), source.to_string(), 1);
        let doc = store.get(&uri).expect("document should exist");
        let mp = reify_core::ModulePath::single("test");
        let a = doc.parsed_module(mp.clone());
        let b = doc.parsed_module(mp.clone());
        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "second parsed_module call should return the same cached Arc (cache hit)"
        );
        assert!(
            !a.declarations.is_empty(),
            "parsed module should have declarations for valid source"
        );
    }

    /// The cached `ParsedModule` reflects the document text: a two-declaration
    /// source yields two top-level declarations.
    #[test]
    fn parsed_module_reflects_text() {
        let mut store = DocumentStore::new();
        let uri = test_uri("reflect");
        let source = "structure A {\n    param x: Scalar = 1mm\n}\nstructure B {\n    param y: Scalar = 2mm\n}";
        store.open(uri.clone(), source.to_string(), 1);
        let doc = store.get(&uri).expect("document should exist");
        let parsed = doc.parsed_module(reify_core::ModulePath::single("test"));
        assert_eq!(
            parsed.declarations.len(),
            2,
            "cached parse should reflect the two top-level declarations in the text"
        );
    }

    /// A version bump replaces the whole `DocumentState` (and its empty cache),
    /// so the post-update parse is a DIFFERENT `Arc` allocation that reflects the
    /// new text — the cache is keyed by document version by construction.
    #[test]
    fn update_invalidates_parse_cache() {
        let mut store = DocumentStore::new();
        let uri = test_uri("invalidate");
        let mp = reify_core::ModulePath::single("test");

        // v1: a single declaration.
        store.open(
            uri.clone(),
            "structure A {\n    param x: Scalar = 1mm\n}".to_string(),
            1,
        );
        let a = {
            let doc = store.get(&uri).expect("document should exist at v1");
            doc.parsed_module(mp.clone())
        };
        assert_eq!(a.declarations.len(), 1, "v1 has one declaration");

        // v2: two declarations — fresh DocumentState with an empty cache.
        store.update(
            &uri,
            "structure A {\n    param x: Scalar = 1mm\n}\nstructure B {\n    param y: Scalar = 2mm\n}"
                .to_string(),
            2,
        );
        let c = {
            let doc = store.get(&uri).expect("document should exist at v2");
            doc.parsed_module(mp.clone())
        };

        assert!(
            !std::sync::Arc::ptr_eq(&a, &c),
            "version bump must invalidate the cache (different Arc allocation)"
        );
        assert_eq!(
            c.declarations.len(),
            2,
            "post-update cached parse should reflect the new text (two declarations)"
        );
    }
}
