//! The version every shipped Reify binary reports.

#[cfg(test)]
mod tests {
    use super::reported_version;

    #[test]
    fn the_release_version_wins() {
        assert_eq!(reported_version(Some("0.3.0-alpha"), "0.1.0"), "0.3.0-alpha");
    }

    #[test]
    fn an_unset_release_version_falls_back_to_the_workspace_version() {
        assert_eq!(reported_version(None, "0.1.0"), "0.1.0");
    }

    #[test]
    fn an_empty_release_version_counts_as_unset() {
        assert_eq!(reported_version(Some(""), "0.1.0"), "0.1.0");
    }
}
