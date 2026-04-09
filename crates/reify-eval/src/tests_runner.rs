/// Overall status of a single `@test` entity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Indeterminate,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_status_variants_exist_and_are_distinct() {
        use super::TestStatus;
        assert_ne!(TestStatus::Pass, TestStatus::Fail);
        assert_ne!(TestStatus::Pass, TestStatus::Indeterminate);
        assert_ne!(TestStatus::Fail, TestStatus::Indeterminate);
    }
}
