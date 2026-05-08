// Step-1 scaffolding: the trait + struct land in step-2; this file currently
// holds only the test module so the build can record a RED iteration before
// the implementation appears.

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check that `T` implements `PersistentlyCacheable`.
    fn assert_persistently_cacheable<T: PersistentlyCacheable>() {}

    #[test]
    fn elastic_result_implements_persistently_cacheable() {
        assert_persistently_cacheable::<ElasticResult>();
    }

    #[test]
    fn elastic_result_constructor_pins_six_field_shape() {
        let er = ElasticResult {
            displacement: vec![1.0, 2.0],
            stress: vec![3.0],
            max_von_mises: 42.0,
            converged: true,
            iterations: 17,
            solve_time_ms: 250,
        };
        assert_eq!(er.displacement, vec![1.0, 2.0]);
        assert_eq!(er.stress, vec![3.0]);
        assert_eq!(er.max_von_mises, 42.0);
        assert!(er.converged);
        assert_eq!(er.iterations, 17);
        assert_eq!(er.solve_time_ms, 250);
    }
}
