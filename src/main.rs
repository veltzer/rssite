fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    /// Placeholder: this crate is still an unimplemented stub, so there is no
    /// behaviour to assert yet. The test exists so `cargo nextest run` finds at
    /// least one test and exits zero instead of failing with "no tests to run".
    /// Replace it with real coverage once main() does something.
    #[test]
    fn main_runs_without_panicking() {
        super::main();
    }
}
