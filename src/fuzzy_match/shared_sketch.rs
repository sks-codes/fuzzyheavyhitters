pub enum SketchData {
    IntervalFSS {
        // Enable computing z1^4 - z2^2
        pub(crate) a: Vec<u128>, // Secret share of a
        pub(crate) a2: Vec<u128>, // Secret share of 6a^2
        pub(crate) a3: Vec<u128>, // Secret share of -4a^3
        pub(crate) a4: Vec<u128>, // Secret share of a^4-b^2
        pub(crate) b: Vec<u128>, // Secret share of b
    },
}