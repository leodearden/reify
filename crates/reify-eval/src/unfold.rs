// Recursive sub-component unfolding — unfold_recursive_sub and elaborate_child_* functions.

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion: elaborate_child_instance is accessible from this module.
    #[test]
    fn elaborate_child_instance_accessible() {
        let _: fn() -> String = || {
            // Reference the function to prove it exists in this module's namespace.
            let _ = elaborate_child_instance as fn(_, _, _, _, _, _, _, _, _, _, _);
            String::new()
        };
    }

    /// Compile-time assertion: unfold_recursive_sub is accessible from this module.
    #[test]
    fn unfold_recursive_sub_accessible() {
        let _: fn() -> String = || {
            let _ = unfold_recursive_sub as fn(_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _);
            String::new()
        };
    }
}
