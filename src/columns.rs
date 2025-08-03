//! Custom column layout module optimized for tale's needs.
//!
//! Replaces term_grid with a simpler, more efficient implementation
//! that writes directly to buffers without intermediate allocations.

use ansi_width::ansi_width;
use bytes::BytesMut;

/// Write cells in columns to buffer with left-to-right layout.
///
/// Cells are laid out left-to-right with the specified padding between columns.
/// When the total width would exceed the specified width limit, a new line is
/// started.
///
/// # Arguments
/// * `buffer` - The buffer to write formatted columns to
/// * `cells` - The text cells to arrange in columns
/// * `width` - Maximum width before wrapping to new line
/// * `padding` - Number of spaces between columns
///
/// # Returns
/// Number of bytes written to the buffer
pub fn write_columns(buffer: &mut BytesMut, cells: &[String], width: usize, padding: usize) -> usize {
    if cells.is_empty() {
        return 0;
    }

    let start_len = buffer.len();
    let mut current_line_width = 0;
    let mut first_in_line = true;

    for cell in cells {
        let cell_width = display_width(cell);

        // Calculate total width needed if we add this cell to current line
        let needed_width = if first_in_line {
            cell_width
        } else {
            current_line_width + padding + cell_width
        };

        // If adding this cell would exceed width limit, start new line
        if needed_width > width && !first_in_line {
            buffer.extend_from_slice(b"\n");
            current_line_width = cell_width;
            first_in_line = false; // Fixed: Mark that we've written to the new line
        } else {
            // Add padding before cell (except for first in line)
            if !first_in_line {
                write_padding(buffer, padding);
                current_line_width += padding;
            }
            current_line_width += cell_width;
            first_in_line = false;
        }

        // Write the cell content
        buffer.extend_from_slice(cell.as_bytes());
    }

    buffer.len() - start_len
}

/// Calculate display width of text, accounting for ANSI escape sequences.
///
/// Uses ansi_width crate to properly handle colored text without counting
/// the escape sequences toward the display width.
fn display_width(text: &str) -> usize {
    ansi_width(text)
}

/// Write the specified number of space characters to the buffer.
///
/// Optimized for small padding counts (typically 2-5 spaces).
fn write_padding(buffer: &mut BytesMut, count: usize) {
    match count {
        0 => {}
        1 => buffer.extend_from_slice(b" "),
        2 => buffer.extend_from_slice(b"  "),
        3 => buffer.extend_from_slice(b"   "),
        4 => buffer.extend_from_slice(b"    "),
        5 => buffer.extend_from_slice(b"     "),
        _ => {
            // For larger counts, use a loop (rare case)
            for _ in 0..count {
                buffer.extend_from_slice(b" ");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::*;

    #[test]
    fn empty_cells() {
        let mut buffer = BytesMut::new();
        let cells = vec![];
        let written = write_columns(&mut buffer, &cells, 80, 2);
        assert_eq!(written, 0);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn single_cell() {
        let mut buffer = BytesMut::new();
        let cells = vec!["hello".to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 2);
        assert_eq!(written, 5);
        assert_eq!(String::from_utf8_lossy(&buffer), "hello");
    }

    #[test]
    fn simple_columns() {
        let mut buffer = BytesMut::new();
        let cells = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 2);
        let result = String::from_utf8_lossy(&buffer);
        println!("Simple test result: '{}'", result);
        assert_eq!(result, "a  b  c");
        assert_eq!(written, 7); // "a  b  c" = 7 bytes
    }

    #[test]
    fn line_wrapping() {
        let mut buffer = BytesMut::new();
        let cells = vec!["long".to_string(), "cell".to_string(), "text".to_string()];
        let written = write_columns(&mut buffer, &cells, 10, 2);
        // "long  cell" = 10 chars exactly, so "text" wraps
        assert_eq!(String::from_utf8_lossy(&buffer), "long  cell\ntext");
        assert_eq!(written, 15); // "long  cell\ntext" = 15 bytes
    }

    #[test]
    fn line_wrapping_tight_fit() {
        let mut buffer = BytesMut::new();
        let cells = vec!["abc".to_string(), "def".to_string(), "ghi".to_string()];
        let written = write_columns(&mut buffer, &cells, 9, 2);
        // "abc  def" = 8 chars, "ghi" = 3 chars, 8+2+3 = 13 > 9, so wrap
        assert_eq!(String::from_utf8_lossy(&buffer), "abc  def\nghi");
        assert_eq!(written, 12);
    }

    #[test]
    fn single_cell_too_wide() {
        let mut buffer = BytesMut::new();
        let cells = vec!["verylongcelltext".to_string()];
        let written = write_columns(&mut buffer, &cells, 10, 2);
        // Single cell that exceeds width should still be written
        assert_eq!(String::from_utf8_lossy(&buffer), "verylongcelltext");
        assert_eq!(written, 16);
    }

    #[test]
    fn zero_padding() {
        let mut buffer = BytesMut::new();
        let cells = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 0);
        assert_eq!(String::from_utf8_lossy(&buffer), "abc");
        assert_eq!(written, 3);
    }

    #[test]
    fn debug_wrapping_issue() {
        let mut buffer = BytesMut::new();
        // Reproduce the exact failing case: fit exactly 10 fields on first line
        // Width 100, fields are 8 chars each, padding 2
        // Line 1: 8+2+8+2+8+2+8+2+8+2+8+2+8+2+8+2+8+2+8 = 98 chars (fits)
        // Line 2: 98+2+8 = 108 > 100, so field11k should wrap
        let cells = vec![
            "field01a".to_string(),
            "field02b".to_string(),
            "field03c".to_string(),
            "field04d".to_string(),
            "field05e".to_string(),
            "field06f".to_string(),
            "field07g".to_string(),
            "field08h".to_string(),
            "field09i".to_string(),
            "field10j".to_string(), // 10 fields = 98 chars
            "field11k".to_string(),
            "field12l".to_string(), // These should wrap
        ];

        write_columns(&mut buffer, &cells, 100, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        println!("Debug exact failing case:");
        for (i, line) in lines.iter().enumerate() {
            println!("Line {}: '{}' (width: {})", i, line, display_width(line));
        }

        // Check that field11k and field12l are properly spaced
        assert_eq!(lines.len(), 2);
        assert!(
            lines[1].contains("field11k  field12l"),
            "Line 1 should have proper spacing: {}",
            lines[1]
        );
    }

    #[test]
    fn large_padding() {
        let mut buffer = BytesMut::new();
        let cells = vec!["a".to_string(), "b".to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 10);
        assert_eq!(String::from_utf8_lossy(&buffer), "a          b");
        assert_eq!(written, 12);
    }

    #[test]
    fn ansi_colors() {
        use owo_colors::OwoColorize;

        let mut buffer = BytesMut::new();
        let cells = vec!["normal".to_string(), "red".red().to_string(), "blue".blue().to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 2);

        // The display width should be calculated ignoring ANSI codes
        // "normal  red  blue" = 17 display characters
        let result = String::from_utf8_lossy(&buffer);

        // Should contain the ANSI codes but display width calculation should ignore
        // them
        assert!(result.contains("normal"));
        assert!(result.contains("red"));
        assert!(result.contains("blue"));
        assert!(written > 17); // More bytes than display chars due to ANSI codes

        // Verify the structure is correct (normal spacing despite ANSI codes)
        let parts: Vec<&str> = result.split("  ").collect();
        assert_eq!(parts.len(), 3); // Should be split by double spaces
    }

    #[test]
    fn display_width_function() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);

        // Test with ANSI codes - should only count visible characters
        use owo_colors::OwoColorize;
        let colored = "test".red().to_string();
        assert_eq!(display_width(&colored), 4); // Only "test" counts, not ANSI codes
    }

    #[test]
    fn write_padding_function() {
        let mut buffer = BytesMut::new();

        // Test optimized cases
        write_padding(&mut buffer, 0);
        assert_eq!(buffer.len(), 0);

        write_padding(&mut buffer, 1);
        assert_eq!(String::from_utf8_lossy(&buffer), " ");

        buffer.clear();
        write_padding(&mut buffer, 5);
        assert_eq!(String::from_utf8_lossy(&buffer), "     ");

        // Test fallback case
        buffer.clear();
        write_padding(&mut buffer, 7);
        assert_eq!(String::from_utf8_lossy(&buffer), "       ");
    }

    #[test]
    fn multiline_column_alignment_uniform() {
        let mut buffer = BytesMut::new();

        // Create enough uniform-length cells to span multiple lines
        // Each cell is 8 chars, with 2-space padding = 10 chars per column
        // Width 100 should fit about 10 columns per line
        let cells = vec![
            "field01a".to_string(),
            "field02b".to_string(),
            "field03c".to_string(),
            "field04d".to_string(),
            "field05e".to_string(),
            "field06f".to_string(),
            "field07g".to_string(),
            "field08h".to_string(),
            "field09i".to_string(),
            "field10j".to_string(),
            "field11k".to_string(),
            "field12l".to_string(),
            "field13m".to_string(),
            "field14n".to_string(),
            "field15o".to_string(),
            "field16p".to_string(),
            "field17q".to_string(),
            "field18r".to_string(),
            "field19s".to_string(),
            "field20t".to_string(),
            "field21u".to_string(),
            "field22v".to_string(),
            "field23w".to_string(),
            "field24x".to_string(),
            "field25y".to_string(),
            "field26z".to_string(),
            "field27A".to_string(),
            "field28B".to_string(),
            "field29C".to_string(),
            "field30D".to_string(),
            "field31E".to_string(),
            "field32F".to_string(),
            "field33G".to_string(),
            "field34H".to_string(),
            "field35I".to_string(),
            "field36J".to_string(),
            "field37K".to_string(),
            "field38L".to_string(),
            "field39M".to_string(),
            "field40N".to_string(), // 40 cells total
        ];

        write_columns(&mut buffer, &cells, 100, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        // Should span multiple lines
        assert!(lines.len() >= 4, "Expected at least 4 lines, got {}", lines.len());

        // Verify each line doesn't exceed width
        for (i, line) in lines.iter().enumerate() {
            let line_width = display_width(line);
            assert!(
                line_width <= 100,
                "Line {} width {} exceeds 100: {}",
                i,
                line_width,
                line
            );
        }

        // For uniform cells, verify consistent structure
        // Each field is 8 chars + 2 spaces = 10 chars per column (except last in line)
        // So we should fit 10 fields per line: 8+2+8+2+...+8 = 98 chars
        for (i, line) in lines.iter().enumerate() {
            let fields: Vec<&str> = line.split("  ").collect(); // Split by 2-space padding
            if i < lines.len() - 1 {
                // Not the last line
                assert!(
                    fields.len() >= 9,
                    "Line {} should have multiple fields: {:?}",
                    i,
                    fields
                );
            }
        }

        // Print for visual inspection
        println!("Uniform column test output:");
        for (i, line) in lines.iter().enumerate() {
            println!("Line {}: '{}' (width: {})", i, line, display_width(line));
        }
    }

    #[test]
    fn multiline_column_alignment_mixed_lengths() {
        let mut buffer = BytesMut::new();

        // Create cells with varying lengths to test alignment with mixed widths
        let cells = vec![
            "a".to_string(),
            "medium_field".to_string(),
            "x".to_string(),
            "very_long_field_name".to_string(),
            "short".to_string(),
            "b".to_string(),
            "another_medium_length".to_string(),
            "c".to_string(),
            "tiny".to_string(),
            "extremely_long_field_that_takes_space".to_string(),
            "d".to_string(),
            "normal".to_string(),
            "e".to_string(),
            "quite_a_long_one".to_string(),
            "f".to_string(),
            "mid".to_string(),
            "g".to_string(),
            "long_enough".to_string(),
            "h".to_string(),
            "average_size".to_string(),
            "i".to_string(),
            "super_duper_long_field_name_here".to_string(),
            "j".to_string(),
            "regular".to_string(),
            "k".to_string(),
            "somewhat_longer".to_string(),
            "l".to_string(),
            "brief".to_string(),
            "m".to_string(),
            "extended_field".to_string(),
            "n".to_string(),
            "moderate_length_field".to_string(),
            "o".to_string(),
            "p".to_string(),
            "incredibly_long_field_name_that_should_wrap".to_string(),
            "q".to_string(),
            "standard".to_string(),
            "r".to_string(),
            "final".to_string(),
        ];

        write_columns(&mut buffer, &cells, 100, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        // Should span multiple lines due to varying field lengths
        assert!(lines.len() >= 4, "Expected at least 4 lines, got {}", lines.len());

        // Verify each line doesn't exceed width
        for (i, line) in lines.iter().enumerate() {
            let line_width = display_width(line);
            assert!(
                line_width <= 100,
                "Line {} width {} exceeds 100: {}",
                i,
                line_width,
                line
            );
        }

        // Verify that fields are properly separated by 2-space padding
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                // Check that we don't have single spaces (should be 2+ or field boundaries)
                let mut space_count = 0;
                for ch in line.chars() {
                    if ch == ' ' {
                        space_count += 1;
                    } else {
                        if space_count == 1 {
                            panic!("Line {} has single space (should be 2+ for padding): {}", i, line);
                        }
                        space_count = 0;
                    }
                }
            }
        }

        // Print for visual inspection
        println!("Mixed-length column test output:");
        for (i, line) in lines.iter().enumerate() {
            println!("Line {}: '{}' (width: {})", i, line, display_width(line));
        }

        // Test specific case: ensure that very long fields don't prevent proper
        // wrapping
        let long_field_lines: Vec<&&str> = lines
            .iter()
            .filter(|line| line.contains("incredibly_long_field_name_that_should_wrap"))
            .collect();
        assert!(!long_field_lines.is_empty(), "Long field should appear in output");
    }

    #[test]
    fn column_width_boundary_conditions() {
        let mut buffer = BytesMut::new();

        // Test exact width boundary conditions
        let cells = vec![
            "12345678".to_string(), // 8 chars
            "90123456".to_string(), // 8 chars
            "78901234".to_string(), // 8 chars
        ];

        // With 2-space padding: 8 + 2 + 8 + 2 + 8 = 28 chars total
        // Test with width exactly 28
        write_columns(&mut buffer, &cells, 28, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        assert_eq!(lines.len(), 1, "Should fit exactly on one line");
        assert_eq!(display_width(lines[0]), 28, "Width should be 8+2+8+2+8=28");

        // Test with width 27 (one less than needed) - should wrap
        buffer.clear();
        write_columns(&mut buffer, &cells, 27, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "Should wrap to two lines with width 27");

        // Test with width 18 (fits first two fields: 8+2+8=18)
        buffer.clear();
        write_columns(&mut buffer, &cells, 18, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "Should wrap last field to second line with width 18");

        println!("Boundary condition test:");
        for (i, line) in lines.iter().enumerate() {
            println!("Line {}: {} (width: {})", i, line, display_width(line));
        }
    }
}
