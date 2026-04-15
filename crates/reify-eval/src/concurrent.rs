// Concurrent edit support: structs and Engine methods for prepare/apply/rollback/resolve
// concurrent edits. Methods use `self.eval_state`, `self.cache`, `self.journal` etc.

#[cfg(test)]
mod tests {
    use super::ConcurrentEditSetup;

    /// Compile-time assertion: ConcurrentEditSetup is accessible from this module.
    #[test]
    fn concurrent_edit_setup_accessible() {
        let _: fn() -> String = || {
            format!("{}", std::mem::size_of::<ConcurrentEditSetup>())
        };
    }
}
