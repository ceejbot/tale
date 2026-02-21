#!/usr/bin/env python3
"""
Generate realistic NDJSON log files for benchmarking tale.

Creates a mix of:
- Structured JSON logs (HTTP requests, app logs, DB queries)
- Plain text logs (stack traces, debug output)
- Malformed JSON (partial, truncated)
"""

import json
import random
import sys
from datetime import datetime, timedelta
from typing import List

# Sample data for realistic log generation
LOG_LEVELS = ["DEBUG", "INFO", "WARN", "ERROR", "CRITICAL"]
HTTP_METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"]
HTTP_PATHS = [
    "/api/users",
    "/api/orders",
    "/api/products",
    "/health",
    "/metrics",
    "/api/auth/login",
    "/api/auth/logout",
    "/static/css/main.css",
    "/api/v1/search",
    "/api/v2/recommendations",
    "/webhook/stripe",
]
HTTP_STATUS = [200, 201, 304, 400, 401, 403, 404, 500, 502, 503]
USER_AGENTS = [
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
    "curl/7.68.0",
    "Go-http-client/1.1",
    "Python-requests/2.28.1",
]
MODULES = [
    "auth.service",
    "db.connection",
    "http.server",
    "cache.redis",
    "payment.stripe",
    "email.sender",
    "metrics.collector",
    "user.manager",
]
ERROR_MESSAGES = [
    "Connection timeout after 30s",
    "Invalid JSON in request body",
    "Database constraint violation: unique_email",
    "Rate limit exceeded: 100 requests/minute",
    "Unauthorized access attempt detected",
    "Memory allocation failed",
    "Deadlock detected in transaction",
]


def random_timestamp() -> str:
    """Generate a random timestamp within the last 30 days."""
    base = datetime.now() - timedelta(days=30)
    offset = random.randint(0, 30 * 24 * 60 * 60)  # 30 days in seconds
    return (base + timedelta(seconds=offset)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def random_request_id() -> str:
    """Generate a realistic request ID."""
    return f"req_{random.randint(100000, 999999)}"


def generate_http_log() -> str:
    """Generate a realistic HTTP access log entry."""
    log = {
        "timestamp": random_timestamp(),
        "level": random.choice(["INFO", "WARN", "ERROR"]),
        "message": f"HTTP {random.choice(HTTP_METHODS)} {random.choice(HTTP_PATHS)}",
        "method": random.choice(HTTP_METHODS),
        "url": random.choice(HTTP_PATHS),
        "status": random.choice(HTTP_STATUS),
        "elapsed": f"{random.randint(1, 2000)}ms",
        "size": random.randint(100, 50000),
        "request_id": random_request_id(),
        "remote_host": f"10.0.{random.randint(1, 255)}.{random.randint(1, 255)}",
        "user_agent": random.choice(USER_AGENTS),
    }

    # Sometimes add extra fields
    if random.random() < 0.3:
        log["user_id"] = random.randint(1000, 9999)
    if random.random() < 0.2:
        log["session_id"] = f"sess_{random.randint(100000, 999999)}"

    return json.dumps(log)


def generate_app_log() -> str:
    """Generate a realistic application log entry."""
    level = random.choice(LOG_LEVELS)
    module = random.choice(MODULES)

    if level in ["ERROR", "CRITICAL"]:
        message = f"Error in {module}: {random.choice(ERROR_MESSAGES)}"
    else:
        actions = ["started", "completed", "cached", "validated", "processed"]
        message = f"Operation {random.choice(actions)} successfully"

    log = {
        "timestamp": random_timestamp(),
        "level": level,
        "message": message,
        "module": module,
        "request_id": random_request_id(),
    }

    # Add file/line info sometimes
    if random.random() < 0.4:
        log["file"] = f"{module.split('.')[0]}.py"
        log["line"] = random.randint(10, 500)

    # Add extra context for errors
    if level in ["ERROR", "CRITICAL"]:
        log["error_code"] = f"E{random.randint(1000, 9999)}"
        if random.random() < 0.5:
            log["stack_trace"] = generate_stack_trace()

    return json.dumps(log)


def generate_db_log() -> str:
    """Generate a database query log entry."""
    queries = [
        "SELECT * FROM users WHERE id = ?",
        "INSERT INTO orders (user_id, total) VALUES (?, ?)",
        "UPDATE products SET inventory = inventory - 1 WHERE id = ?",
        "DELETE FROM sessions WHERE expires_at < NOW()",
    ]

    log = {
        "timestamp": random_timestamp(),
        "level": random.choice(["DEBUG", "INFO", "WARN"]),
        "message": "Database query executed",
        "module": "db.connection",
        "query": random.choice(queries),
        "elapsed": f"{random.randint(1, 500)}ms",
        "rows_affected": random.randint(0, 100),
        "request_id": random_request_id(),
    }

    return json.dumps(log)


def generate_stack_trace() -> str:
    """Generate a realistic multi-line stack trace."""
    lines = [
        "Traceback (most recent call last):",
        '  File "app.py", line 123, in handle_request',
        "    result = process_data(data)",
        '  File "processor.py", line 45, in process_data',
        "    return validate_input(data)",
        '  File "validator.py", line 78, in validate_input',
        '    raise ValueError("Invalid input format")',
        "ValueError: Invalid input format",
    ]
    return "\n".join(lines)


def generate_plain_text() -> str:
    """Generate plain text log entries (non-JSON)."""
    templates = [
        f"[{random_timestamp()}] {random.choice(LOG_LEVELS)}: {random.choice(ERROR_MESSAGES)}",
        f"Starting server on port {random.randint(3000, 9000)}...",
        f"Connected to database: postgresql://localhost:5432/app",
        generate_stack_trace(),
        f"Memory usage: {random.randint(50, 95)}% ({random.randint(500, 2000)}MB used)",
        f"Processing batch of {random.randint(100, 1000)} items...",
    ]
    return random.choice(templates)


def generate_malformed_json() -> str:
    """Generate malformed JSON to test error handling."""
    malformed = [
        '{"timestamp": "2023-01-01T10:00:00Z", "level": "INFO", "message": "truncated',
        '{"timestamp": "2023-01-01T10:00:00Z", "level": "INFO", "message": "unescaped \\"quotes"}',
        '{"timestamp": "2023-01-01T10:00:00Z", "level": "INFO", "message": "missing quote}',
        '{timestamp: "2023-01-01T10:00:00Z", "level": "INFO"}',  # missing quotes on key
        '{"timestamp": "2023-01-01T10:00:00Z", "level": "INFO", "message": "extra comma",}',
    ]
    return random.choice(malformed)


def generate_log_line() -> str:
    """Generate a single log line based on weighted distribution."""
    rand = random.random()

    if rand < 0.4:  # 40% HTTP logs
        return generate_http_log()
    elif rand < 0.7:  # 30% app logs
        return generate_app_log()
    elif rand < 0.8:  # 10% DB logs
        return generate_db_log()
    elif rand < 0.9:  # 10% plain text
        return generate_plain_text()
    else:  # 10% malformed JSON
        return generate_malformed_json()


def generate_log_file(filename: str, num_lines: int):
    """Generate a log file with the specified number of lines."""
    print(f"Generating {filename} with {num_lines:,} lines...")

    with open(filename, "w") as f:
        for i in range(num_lines):
            if i > 0 and i % 10000 == 0:
                print(
                    f"  Progress: {i:,}/{num_lines:,} lines ({i / num_lines * 100:.1f}%)"
                )

            line = generate_log_line()
            f.write(line + "\n")

    print(f"  Complete! Generated {filename}")


def main():
    """Generate benchmark test files."""
    test_files = [
        ("fixtures/benchmarks/small.log", 1_000),  # ~100KB
        ("fixtures/benchmarks/medium.log", 100_000),  # ~10MB
        ("fixtures/benchmarks/large.log", 1_000_000),  # ~100MB
    ]

    for filename, num_lines in test_files:
        generate_log_file(filename, num_lines)

    print("\nBenchmark test files generated successfully!")
    print("Files created:")
    for filename, num_lines in test_files:
        print(f"  {filename}: {num_lines:,} lines")


if __name__ == "__main__":
    main()
