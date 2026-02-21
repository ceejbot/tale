//! Column layout module using elastic tabstops via tabwriter.
//! Cells are assigned to rows, tab-separated, and tabwriter aligns
//! columns to their widest cell for consistent vertical alignment.

use std::io::Write;

use ansi_width::ansi_width;
use bytes::BytesMut;
use tabwriter::TabWriter;

/// Write cells in aligned columns using elastic tabstops.
///
/// Cells are distributed across rows with a computed column count based on
/// average cell width. tabwriter aligns each column to its widest cell,
/// producing consistent vertical alignment across wrapped rows.
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

    let widths: Vec<usize> = cells.iter().map(|c| display_width(c)).collect();
    let total_display: usize = widths.iter().sum();

    // Determine column count
    let num_cols = if cells.len() == 1 {
        1
    } else {
        let total_with_padding = total_display + padding * (cells.len() - 1);
        if total_with_padding <= width {
            // All cells fit on one line
            cells.len()
        } else {
            // Estimate from average, then verify elastic tabstop widths fit
            let avg_width = total_display / cells.len();
            let mut cols = ((width + padding) / (avg_width + padding)).max(1).min(cells.len());
            while cols > 1 && max_row_width(&widths, cols, padding) > width {
                cols -= 1;
            }
            cols
        }
    };

    let mut tw = TabWriter::new(vec![]).minwidth(0).padding(padding).ansi(true);

    for (i, cell) in cells.iter().enumerate() {
        if i > 0 && i % num_cols == 0 {
            let _ = tw.write_all(b"\n");
        } else if i > 0 {
            let _ = tw.write_all(b"\t");
        }
        let _ = tw.write_all(cell.as_bytes());
    }

    match tw.into_inner() {
        Ok(result) => buffer.extend_from_slice(&result),
        Err(_) => {
            // Fallback: write cells space-separated (should never happen with Vec<u8>)
            for (i, cell) in cells.iter().enumerate() {
                if i > 0 {
                    buffer.extend_from_slice(b" ");
                }
                buffer.extend_from_slice(cell.as_bytes());
            }
        }
    }

    buffer.len() - start_len
}

/// Compute the widest possible row when cells are distributed into `num_cols`
/// columns. Each column is sized to its widest cell (elastic tabstop behavior).
fn max_row_width(widths: &[usize], num_cols: usize, padding: usize) -> usize {
    let mut col_maxes = vec![0usize; num_cols];
    for (i, &w) in widths.iter().enumerate() {
        let col = i % num_cols;
        col_maxes[col] = col_maxes[col].max(w);
    }
    let total: usize = col_maxes.iter().sum();
    total + padding * num_cols.saturating_sub(1)
}

/// Calculate display width of text, accounting for ANSI escape sequences.
fn display_width(text: &str) -> usize {
    ansi_width(text)
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
        assert_eq!(result, "a  b  c");
        assert_eq!(written, 7);
    }

    #[test]
    fn line_wrapping() {
        let mut buffer = BytesMut::new();
        let cells = vec!["long".to_string(), "cell".to_string(), "text".to_string()];
        let written = write_columns(&mut buffer, &cells, 10, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        // With elastic tabstops, 2 columns fit (4+2+4=10)
        assert_eq!(lines.len(), 2, "Should wrap to 2 lines");
        assert!(lines[0].contains("long") && lines[0].contains("cell"));
        assert!(lines[1].contains("text"));
        assert_eq!(written, result.len());
    }

    #[test]
    fn line_wrapping_tight_fit() {
        let mut buffer = BytesMut::new();
        let cells = vec!["abc".to_string(), "def".to_string(), "ghi".to_string()];
        let written = write_columns(&mut buffer, &cells, 9, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        // 2 columns: 3+2+3=8 ≤ 9, third cell wraps
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("abc") && lines[0].contains("def"));
        assert!(lines[1].contains("ghi"));
        assert_eq!(written, result.len());
    }

    #[test]
    fn single_cell_too_wide() {
        let mut buffer = BytesMut::new();
        let cells = vec!["verylongcelltext".to_string()];
        let written = write_columns(&mut buffer, &cells, 10, 2);
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
        ];

        write_columns(&mut buffer, &cells, 100, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        // 10 uniform 8-char fields fit per row: 10*8 + 9*2 = 98 ≤ 100
        assert_eq!(lines.len(), 2);
        // Wrapped line should contain both remaining fields
        assert!(
            lines[1].contains("field11k") && lines[1].contains("field12l"),
            "Wrapped line should contain field11k and field12l: {}",
            lines[1]
        );
    }

    #[test]
    fn large_padding() {
        let mut buffer = BytesMut::new();
        let cells = vec!["a".to_string(), "b".to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 10);
        let result = String::from_utf8_lossy(&buffer);
        // tabwriter with padding=10: "a" + 10 spaces + "b"
        assert!(result.contains("a") && result.contains("b"));
        assert_eq!(display_width(&result), 12); // 1 + 10 + 1
        assert_eq!(written, 12);
    }

    #[test]
    fn ansi_colors() {
        use owo_colors::OwoColorize;

        let mut buffer = BytesMut::new();
        let cells = vec!["normal".to_string(), "red".red().to_string(), "blue".blue().to_string()];
        let written = write_columns(&mut buffer, &cells, 80, 2);
        let result = String::from_utf8_lossy(&buffer);

        // All cells present
        assert!(result.contains("normal"));
        assert!(result.contains("red"));
        assert!(result.contains("blue"));
        // More bytes than display chars due to ANSI codes
        assert!(written > 17);
        // All on one line
        assert_eq!(result.lines().count(), 1);
    }

    #[test]
    fn display_width_function() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);

        use owo_colors::OwoColorize;
        let colored = "test".red().to_string();
        assert_eq!(display_width(&colored), 4);
    }

    #[test]
    fn multiline_column_alignment_uniform() {
        let mut buffer = BytesMut::new();

        // 40 uniform 8-char cells, width 100, padding 2
        // Each column takes 8+2=10 chars, so 10 columns fit (10*8+9*2=98 ≤ 100)
        let cells: Vec<String> = (1..=40).map(|i| format!("field{:02}x", i)).collect();

        write_columns(&mut buffer, &cells, 100, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        // 40 cells / 10 cols = 4 rows
        assert_eq!(lines.len(), 4, "Expected 4 lines, got {}", lines.len());

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

        // Verify consistent column count on full rows
        for line in &lines[..3] {
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 10, "Full row should have 10 fields: {}", line);
        }
    }

    #[test]
    fn multiline_column_alignment_mixed_lengths() {
        let mut buffer = BytesMut::new();

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

        // Should span multiple lines
        assert!(lines.len() >= 4, "Expected at least 4 lines, got {}", lines.len());

        // Verify each line respects width limit
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

        // All cells should appear in output
        for cell in &cells {
            assert!(result.contains(cell.as_str()), "Missing cell: {}", cell);
        }
    }

    #[test]
    fn column_width_boundary_conditions() {
        let mut buffer = BytesMut::new();
        let cells = vec!["12345678".to_string(), "90123456".to_string(), "78901234".to_string()];

        // Width 28: 8+2+8+2+8=28, should fit exactly
        write_columns(&mut buffer, &cells, 28, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 1, "Should fit exactly on one line");
        assert_eq!(display_width(lines[0]), 28);

        // Width 27: doesn't fit, should wrap
        buffer.clear();
        write_columns(&mut buffer, &cells, 27, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "Should wrap to two lines with width 27");

        // Width 18: 8+2+8=18, two columns fit
        buffer.clear();
        write_columns(&mut buffer, &cells, 18, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "Should wrap last field to second line with width 18");
    }

    #[test]
    fn columns_align_across_wrapped_rows() {
        let mut buffer = BytesMut::new();

        // Cells with varying widths: elastic tabstops should align columns
        let cells = vec![
            "short".to_string(),
            "medium_len".to_string(),
            "x".to_string(),
            "a_longer_cell".to_string(),
            "med".to_string(),
            "y".to_string(),
        ];

        write_columns(&mut buffer, &cells, 40, 2);
        let result = String::from_utf8_lossy(&buffer);
        let lines: Vec<&str> = result.lines().collect();

        assert!(lines.len() >= 2, "Should wrap to multiple lines");

        // With elastic tabstops, column N on row 0 should start at the same
        // position as column N on row 1 (this is the key alignment benefit)
        if lines.len() >= 2 {
            // Find where the second column starts on each line
            let line0_parts: Vec<&str> = lines[0].split_whitespace().collect();
            let line1_parts: Vec<&str> = lines[1].split_whitespace().collect();

            // Both lines should have the same column count (except possibly the last row)
            if line0_parts.len() == line1_parts.len() {
                // The second column should start at the same byte offset
                let col1_start_0 = lines[0].find(line0_parts[1]);
                let col1_start_1 = lines[1].find(line1_parts[1]);
                assert_eq!(
                    col1_start_0, col1_start_1,
                    "Column 1 should align: row0={:?} row1={:?}",
                    col1_start_0, col1_start_1
                );
            }
        }
    }

    #[test]
    fn single_cell_no_tabwriter_overhead() {
        let mut buffer = BytesMut::new();
        let cells = vec!["only_one".to_string()];
        write_columns(&mut buffer, &cells, 80, 5);
        let result = String::from_utf8_lossy(&buffer);
        // Single cell should have no padding or extra whitespace
        assert_eq!(result, "only_one");
    }
}
