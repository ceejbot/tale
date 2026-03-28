use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::NamedTempFile;

/// Verify that tailing a file delivers every line without gaps, including lines
/// appended in a burst after the initial read.
///
/// This exercises the polling loop in BackSeekingProcessor. Previously, a new
/// BufReader was created on each loop iteration: BufReader reads up to 8KB
/// ahead, advancing the OS file position past many lines at once. When it was
/// dropped after consuming just one line, stream_position() recorded that
/// inflated offset and the next iteration seeked past all the buffered lines.
#[test]
fn tailing_delivers_every_line() {
    let mut file = NamedTempFile::new().expect("failed to create temp file");

    // Write initial content before spawning so tale reads it at startup.
    for i in 0..5_usize {
        writeln!(file, r#"{{"level":"INFO","message":"msg-{i:02}"}}"#)
            .expect("failed to write initial line");
    }
    file.flush().expect("failed to flush initial lines");

    let mut child = Command::new(env!("CARGO_BIN_EXE_tale"))
        .args(["-f", file.path().to_str().expect("path must be valid utf-8")])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn tale");

    // Let it start and consume the initial content.
    std::thread::sleep(Duration::from_millis(300));

    // Append a burst of lines. The old code would skip most of these because
    // BufReader's 8KB read-ahead advanced the OS file position past all of
    // them in a single iteration, then stream_position() captured that
    // inflated offset.
    for i in 5..15_usize {
        writeln!(file, r#"{{"level":"INFO","message":"msg-{i:02}"}}"#)
            .expect("failed to write appended line");
    }
    file.flush().expect("failed to flush appended lines");

    // Give the polling loop (100ms tick) time to pick everything up.
    std::thread::sleep(Duration::from_millis(500));

    child.kill().expect("failed to kill tale");
    let output = child.wait_with_output().expect("failed to collect output");
    let stdout = String::from_utf8_lossy(&output.stdout);

    for i in 0..15_usize {
        let marker = format!("msg-{i:02}");
        assert!(stdout.contains(&marker), "output missing {marker};\nstdout:\n{stdout}");
    }
}
