//! Quantitative comparison of `run.csv` series.

pub mod compare;
pub mod error;
pub mod trace;

pub use compare::{
    ComparisonReport, PhaseReport, ValidationConfig, compare_csv_files,
    compare_csv_files_with_config, compare_or_dump_phases, compare_or_dump_with_run,
    compare_traces, compare_traces_by_phases, phase_report_passes,
};
pub use error::ValidateError;
pub use trace::{
    OrColumnMap, OrDistanceUnit, OrSpeedUnit, RunTrace, TraceSample, infer_brake_full_scale,
    normalize_trace_brake_to_fraction, parse_openrailsrs_run_csv, parse_or_dump_csv,
    resample_traces, write_or_eval_driver_csv,
};
