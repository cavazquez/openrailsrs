//! Quantitative comparison of `run.csv` series.

pub mod compare;
pub mod error;

pub use compare::{ComparisonReport, compare_csv_files};
pub use error::ValidateError;
