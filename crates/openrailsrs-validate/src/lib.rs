//! Quantitative comparison of `run.csv` series.

pub mod compare;
pub mod error;

pub use compare::{
    ComparisonReport, ValidationConfig, compare_csv_files, compare_csv_files_with_config,
};
pub use error::ValidateError;
