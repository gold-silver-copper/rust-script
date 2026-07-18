#![forbid(unsafe_code)]
#![doc = "Native rustc differential-testing support for rustscript."]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub seed: u64,
    pub cases: usize,
}
