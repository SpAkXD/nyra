//! `use web` (v0.8, designed in `docs/WEB.md`): the parts of the web library written in Rust.
//!
//! - `ease`: easing presets and springs compiled to CSS `linear()` (WP4).
//! - `motion`: the motion words, their CSS and JS chunks, the `keyframes` checker (WP4).
//! - `gzip`: the gzipped size of a file, for the size budgets.

pub mod ease;
pub mod gzip;
pub mod motion;
