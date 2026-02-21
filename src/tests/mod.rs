//! Integration tests etc in their own module.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use jiff::Timestamp;

mod budget;
mod integration;
pub mod simulation;

/// Different log patterns for generating realistic test data
#[derive(Debug, Clone, Copy)]
pub enum TestLogPattern {
    /// Optimized HTTP server logs (fastest parsing path)
    Canonical,
    /// General structured logs with varied fields
    Message,
    /// Java application logs with stack traces
    _Java,
    /// Mixed patterns for realistic scenarios
    Mixed,
}

impl TestLogPattern {
    fn name(&self) -> &'static str {
        match self {
            TestLogPattern::Canonical => "canonical",
            TestLogPattern::Message => "message",
            TestLogPattern::_Java => "java",
            TestLogPattern::Mixed => "mixed",
        }
    }
}

/// Create a large test file with realistic log data, using smart caching
pub fn create_large_test_file(lines: usize) -> PathBuf {
    create_test_file(lines, TestLogPattern::Mixed)
}

/// Create a test file with specific log pattern, using smart caching
pub fn create_test_file(lines: usize, pattern: TestLogPattern) -> PathBuf {
    // Generate predictable filename based on pattern and size
    let filename = format!("test_{}_{}_lines.log", pattern.name(), lines);
    let path = PathBuf::from("fixtures/benchmarks").join(filename);

    // Return existing file if it exists and has correct line count
    if path.exists() && verify_line_count(&path, lines) {
        return path;
    }

    // Create directory if needed
    fs::create_dir_all("fixtures/benchmarks").expect("Failed to create test directory");

    // Generate new file with realistic log patterns
    generate_log_file(&path, lines, pattern);
    path
}

/// Verify that a test file has the expected number of lines
fn verify_line_count(path: &PathBuf, expected: usize) -> bool {
    fs::read_to_string(path)
        .map(|content| content.lines().count() == expected)
        .unwrap_or(false)
}

/// Generate a test log file with the specified pattern
fn generate_log_file(path: &PathBuf, lines: usize, pattern: TestLogPattern) {
    let mut file = fs::File::create(path).expect("Failed to create test file");

    let base_timestamp = Timestamp::now().as_second();

    for i in 0..lines {
        let timestamp = base_timestamp + (i as i64);
        let log_line = match pattern {
            TestLogPattern::Canonical => generate_canonical_log(i, timestamp),
            TestLogPattern::Message => generate_message_log(i, timestamp),
            TestLogPattern::_Java => generate_java_log(i, timestamp),
            TestLogPattern::Mixed => {
                // 70% Message, 20% Canonical, 10% Java
                match i % 10 {
                    0 | 9 => generate_java_log(i, timestamp),
                    1 | 2 => generate_canonical_log(i, timestamp),
                    _ => generate_message_log(i, timestamp),
                }
            }
        };

        writeln!(file, "{}", log_line).expect("Failed to write to test file");
    }
}

/// Generate a Canonical pattern log entry (HTTP server logs)
fn generate_canonical_log(index: usize, timestamp: i64) -> String {
    let methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];
    let status_codes = [200, 201, 204, 400, 401, 403, 404, 500];
    let urls = ["/api/users", "/api/orders", "/api/products", "/health", "/metrics"];

    let method = methods[index % methods.len()];
    let status = status_codes[index % status_codes.len()];
    let url = format!("{}/{}", urls[index % urls.len()], index);
    let elapsed = 10 + (index % 200); // 10-210ms
    let size = 512 + (index % 2048); // 512-2560 bytes

    let level = match status {
        200..=299 => "INFO",
        400..=499 => "WARN",
        500..=599 => "ERROR",
        _ => "INFO",
    };

    format!(
        r#"{{"timestamp":"{}","level":"{}","message":"Request processed","method":"{}","url":"{}","status":{},"elapsed":"{}ms","size":{},"request_id":"req_{}","remote_host":"192.168.1.{}","user_agent":"test-agent/1.0"}}"#,
        Timestamp::from_second(timestamp)
            .expect("test fixtures should work")
            .strftime("%Y-%m-%dT%H:%M:%SZ"),
        level,
        method,
        url,
        status,
        elapsed,
        size,
        index,
        100 + (index % 155) // 192.168.1.100-254
    )
}

/// Generate a Message pattern log entry (general structured logs)
fn generate_message_log(index: usize, timestamp: i64) -> String {
    let levels = ["DEBUG", "INFO", "INFO", "INFO", "WARN", "ERROR"]; // Weighted toward INFO
    let modules = ["auth", "database", "cache", "api", "worker", "scheduler"];
    let messages = [
        "User authentication successful",
        "Database query completed",
        "Cache miss, fetching from database",
        "API request processed",
        "Background job started",
        "Scheduled task executed",
    ];

    let level = levels[index % levels.len()];
    let module = modules[index % modules.len()];
    let message = messages[index % messages.len()];

    // Add some realistic additional fields with variation
    let mut extra_fields = Vec::new();

    if index.is_multiple_of(3) {
        extra_fields.push(format!(r#""user_id":{}"#, 1000 + (index % 9999)));
    }
    if index.is_multiple_of(4) {
        extra_fields.push(format!(r#""duration":"{}ms""#, 1 + (index % 100)));
    }
    if index.is_multiple_of(5) {
        extra_fields.push(format!(r#""cache_hit":{}"#, index.is_multiple_of(2)));
    }
    if index.is_multiple_of(7) {
        extra_fields.push(format!(r#""memory_usage":"{}MB""#, 10 + (index % 90)));
    }

    let extra = if extra_fields.is_empty() {
        String::new()
    } else {
        format!(",{}", extra_fields.join(","))
    };

    format!(
        r#"{{"timestamp":"{}","level":"{}","message":"{}","module":"{}"{}}}"#,
        Timestamp::from_second(timestamp)
            .expect("test fixtures should work")
            .strftime("%Y-%m-%dT%H:%M:%SZ"),
        level,
        message,
        module,
        extra
    )
}

/// Generate a Java pattern log entry (with occasional stack traces)
fn generate_java_log(index: usize, timestamp: i64) -> String {
    let levels = ["DEBUG", "INFO", "WARN", "ERROR"];
    let classes = [
        "com.example.service.UserService",
        "com.example.controller.ApiController",
        "com.example.repository.DataRepository",
        "com.example.worker.BackgroundWorker",
    ];

    let level = levels[index % levels.len()];
    let class = classes[index % classes.len()];

    // Every 20th line gets a stack trace for ERROR/WARN
    let has_stack_trace = index.is_multiple_of(20) && (level == "ERROR" || level == "WARN");

    let message = if has_stack_trace {
        format!(
            "Exception occurred in method processRequest: java.lang.RuntimeException: Simulated error #{}",
            index
        )
    } else {
        format!("Processing request {} in {}", index, class)
    };

    let mut log_entry = format!(
        r#"{{"timestamp":"{}","level":"{}","message":"{}","logger":"{}","thread":"pool-{}-thread-{}""#,
        Timestamp::from_second(timestamp)
            .expect("test fixtures should work")
            .strftime("%Y-%m-%dT%H:%M:%SZ"),
        level,
        message,
        class,
        1 + (index % 3),  // pool-1 to pool-3
        1 + (index % 10)  // thread-1 to thread-10
    );

    if has_stack_trace {
        log_entry.push_str(r#","stack_trace":"java.lang.RuntimeException: Simulated error\n\tat com.example.service.UserService.processRequest(UserService.java:42)\n\tat com.example.controller.ApiController.handleRequest(ApiController.java:123)""#);
    }

    log_entry.push('}');
    log_entry
}
