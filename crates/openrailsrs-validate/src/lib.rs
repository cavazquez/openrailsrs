//! Quantitative comparison of `run.csv` series.

pub mod compare;
pub mod error;
pub mod trace;

pub use compare::{
    ComparisonReport, ValidationConfig, compare_csv_files, compare_csv_files_with_config,
    compare_or_dump_with_run, compare_traces,
};
pub use error::ValidateError;
pub use trace::{
    OrColumnMap, OrDistanceUnit, OrSpeedUnit, RunTrace, TraceSample, parse_openrailsrs_run_csv,
    parse_or_dump_csv, resample_traces, write_or_eval_driver_csv,
};
