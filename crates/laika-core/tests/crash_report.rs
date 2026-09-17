//! V32: the panic hook writes an opt-in crash report beside the log.
//! (Its own test binary: logging and the panic hook are process-global.)

#[test]
fn opt_in_crash_report_is_written_and_scrubbed() {
    let dir = std::env::temp_dir().join(format!("laika-crash-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    // No stderr capture inside the test harness.
    unsafe { std::env::set_var("LAIKA_LOG", "0") };
    laika_core::logging::init(&dir, "0.0.0-test").unwrap();
    laika_core::logging::install_panic_hook();
    laika_core::logging::register_secret("s3cr3t-value-XYZ");
    laika_core::logging::set_crash_context("Laika 0.0.0-test\nGPU: test adapter");

    // Off: a panic writes no report.
    let _ = std::panic::catch_unwind(|| panic!("quiet failure"));
    assert!(laika_core::logging::crash_reports().is_empty());

    laika_core::logging::set_crash_reports(true);
    let _ = std::panic::catch_unwind(|| panic!("decode blew up with s3cr3t-value-XYZ"));
    let reports = laika_core::logging::crash_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    assert!(
        text.contains("Panic: decode blew up with [redacted]"),
        "{text}"
    );
    assert!(!text.contains("s3cr3t-value-XYZ"));
    assert!(text.contains("GPU: test adapter"));
    assert!(text.contains("Backtrace:"));
    // The launch line is in the live log the report quotes.
    assert!(text.contains("---- start Laika 0.0.0-test"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}
