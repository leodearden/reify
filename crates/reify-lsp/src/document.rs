use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use reify_ast::ParsedModule;
use reify_core::ModulePath;
use tower_lsp::lsp_types::Url;

use crate::analysis::module_name_from_uri;

/// State of a single open document.
pub struct DocumentState {
    pub text: String,
    pub version: i32,
    /// This document's module identity, derived once from its URI at
    /// construction (by [`DocumentStore`]).
    ///
    /// Owned by the document rather than supplied per parse request: every parse
    /// of this document uses this one path, so the cached parse below can never
    /// be built under a module name that disagrees with a later caller's. This
    /// is what makes [`DocumentState::parsed_module`] argument-free and removes
    /// the footgun of a per-call module path that a cache hit would silently
    /// ignore.
    module_path: ModulePath,
    /// Lazily-filled, per-version cache of the prelude-aware parse of `text`.
    ///
    /// Filled on the first [`DocumentState::parsed_module`] call after this
    /// state is created, then shared (as a cheap `Arc` clone) by every later
    /// call for the same document version. A version bump replaces the whole
    /// `DocumentState` with a fresh one whose cache starts empty, so the cache
    /// is keyed by document version by construction — no version compare, no
    /// data race. The `Mutex` provides interior mutability so the cache can
    /// fill while a caller holds only a shared reference to the document.
    parsed_cache: Mutex<Option<Arc<ParsedModule>>>,
}

impl DocumentState {
    /// Create a new document state with an empty parse cache.
    ///
    /// `module_path` is the document's module identity (derived from its URI by
    /// the [`DocumentStore`]); it is stored once and reused for every parse of
    /// this document, so the cached parse is never keyed by a per-request value.
    pub fn new(text: String, version: i32, module_path: ModulePath) -> Self {
        Self {
            text,
            version,
            module_path,
            parsed_cache: Mutex::new(None),
        }
    }

    /// Return the prelude-aware parse of this document, parsing once and caching
    /// the result for the lifetime of this `DocumentState` (i.e. this document
    /// version).
    ///
    /// The parse uses the document's own [`ModulePath`] (fixed at construction),
    /// so repeated calls always describe the same module — there is no per-call
    /// module argument that a cache hit could silently ignore. The first call
    /// parses `text` via [`reify_compiler::parse_with_stdlib`] and stores the
    /// result behind an `Arc`; later calls return a clone of the SAME `Arc` (a
    /// cache hit — deterministically verifiable with `Arc::ptr_eq`). Lock
    /// poisoning is recovered rather than propagated, matching the crate's
    /// `eval_state` lock convention, so a panic mid-parse cannot wedge every
    /// subsequent request.
    pub fn parsed_module(&self) -> Arc<ParsedModule> {
        let mut cache = self.parsed_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(parsed) = cache.as_ref() {
            return Arc::clone(parsed);
        }
        let parsed = Arc::new(reify_compiler::parse_with_stdlib(
            &self.text,
            self.module_path.clone(),
        ));
        *cache = Some(Arc::clone(&parsed));
        parsed
    }

    /// Test-only peek at the parse cache WITHOUT forcing a parse.
    ///
    /// Returns the currently-cached parse, or `None` if the cache is still cold
    /// for this document version. Unlike [`DocumentState::parsed_module`] this
    /// never fills the cache, so a test can observe whether *something else*
    /// (e.g. a provider handler) populated it as a side effect — proving the
    /// provider consumed the shared cache rather than re-parsing internally.
    #[cfg(test)]
    pub(crate) fn peek_cached_parse(&self) -> Option<Arc<ParsedModule>> {
        self.parsed_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Stores open document contents, keyed by URI.
#[derive(Default)]
pub struct DocumentStore {
    documents: HashMap<Url, Arc<DocumentState>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Url, text: String, version: i32) {
        let module_path = ModulePath::single(module_name_from_uri(&uri));
        self.documents
            .insert(uri, Arc::new(DocumentState::new(text, version, module_path)));
    }

    pub fn update(&mut self, uri: &Url, text: String, version: i32) -> bool {
        if let Some(slot) = self.documents.get_mut(uri) {
            // Replace the whole state — and its empty cache — so the prior
            // version's parse is structurally invalidated (cache keyed by
            // document version). This is the invalidation point. The module
            // path is re-derived from the (unchanged) URI so the fresh state
            // parses under the same module identity.
            let module_path = ModulePath::single(module_name_from_uri(uri));
            *slot = Arc::new(DocumentState::new(text, version, module_path));
            true
        } else {
            false
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<Arc<DocumentState>> {
        self.documents.get(uri).cloned()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Url, &Arc<DocumentState>)> {
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
        let source = "structure A {\n    param x: Length = 1mm\n}";
        store.open(uri.clone(), source.to_string(), 1);
        let doc = store.get(&uri).expect("document should exist");
        let a = doc.parsed_module();
        let b = doc.parsed_module();
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
        let source = "structure A {\n    param x: Length = 1mm\n}\nstructure B {\n    param y: Length = 2mm\n}";
        store.open(uri.clone(), source.to_string(), 1);
        let doc = store.get(&uri).expect("document should exist");
        let parsed = doc.parsed_module();
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

        // v1: a single declaration.
        store.open(
            uri.clone(),
            "structure A {\n    param x: Length = 1mm\n}".to_string(),
            1,
        );
        let a = {
            let doc = store.get(&uri).expect("document should exist at v1");
            doc.parsed_module()
        };
        assert_eq!(a.declarations.len(), 1, "v1 has one declaration");

        // v2: two declarations — fresh DocumentState with an empty cache.
        store.update(
            &uri,
            "structure A {\n    param x: Length = 1mm\n}\nstructure B {\n    param y: Length = 2mm\n}"
                .to_string(),
            2,
        );
        let c = {
            let doc = store.get(&uri).expect("document should exist at v2");
            doc.parsed_module()
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
