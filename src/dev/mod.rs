//! Developer-only harnesses: lint-style tests that walk the source tree.
//! Nothing here ships in the app binary (`#[cfg(test)]` gated).

#[cfg(test)]
pub mod lint;
