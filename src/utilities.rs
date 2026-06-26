use std::{collections::HashMap, error::Error, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::PathBuf};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use egui::Color32;
use tempfile::NamedTempFile;

use crate::color::ColorScheme;

/// Build a TOML array `[a, b]` of two floats for the config file. Used for the
/// `coordinates` and `window_size_startup` pairs so both the startup writer and
/// the runtime setters emit a real numeric array (not a stringified one).
pub fn float_pair_array(pair: [f32; 2]) -> toml_edit::Array {
    let mut arr = toml_edit::Array::new();
    arr.push(pair[0] as f64);
    arr.push(pair[1] as f64);
    arr
}

pub fn ordinal_suffix(day: u32) -> &'static str {
    match day % 100 {
        11 | 12 | 13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}

pub fn format_date(naive: NaiveDate) -> (String, String) {
    let day = naive.day();
    let weekday = naive.format("%A").to_string().to_uppercase();
    let month = naive.format("%B").to_string();
    let year = naive.year();

    let full_date = format!("{} {}{}, {}", month, day, ordinal_suffix(day), year);

    (weekday, full_date)
}

pub fn parse_time_input(day: i32, month: i32, year: i32, hour: i32, minute: i32) -> Result<DateTime<Local>, Box<dyn Error>> {
    let string_method = format!("{}-{}-{} {}:{}", year, month, day, hour, minute);
    let naive_date_time = NaiveDateTime::parse_from_str(&string_method, "%Y-%-m-%-d %-H:%-M")?;
    let date_time = Local.from_local_datetime(&naive_date_time).single()
        .ok_or("Failed to convert to local datetime")?;
    Ok(date_time)
}

pub fn next_three_weekdays(now: DateTime<Local>) -> (String, String, String) {
    (
        now.format("%A").to_string(),
        (now + Duration::days(1)).format("%A").to_string(),
        (now + Duration::days(2)).format("%A").to_string(),
    )
}

/// Number of days in `month` (1..=12) of `year`, accounting for leap years.
/// Falls back to 31 for an out-of-range month so callers never get a 0-length
/// day range.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month >= 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    match (
        NaiveDate::from_ymd_opt(year, month, 1),
        NaiveDate::from_ymd_opt(next_year, next_month, 1),
    ) {
        (Some(first), Some(next_first)) => {
            next_first.signed_duration_since(first).num_days() as u32
        }
        _ => 31,
    }
}

pub fn resolve_colorscheme(
    schemes: &HashMap<u32, ColorScheme>,
    selected_id: u32,
) -> [Color32; 6] {
    schemes
        .get(&selected_id)
        .unwrap_or(&ColorScheme::default_scheme())
        .colors
        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
}


pub fn save_notepad_text(payload: String, exe_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    // Determine the path to the target JSON file
    let data_dir = crate::tasks::get_data_dir(exe_path)?;

    let final_path = data_dir.join("notepad_text.json");

    // Ensure the directory exists
    fs::create_dir_all(&data_dir)?;

    // Serialize first to avoid writing an invalid file
    let json = serde_json::to_string_pretty(&payload)?;

    // Write to a temporary file first
    let mut temp_file = NamedTempFile::new_in(&data_dir)?;
    {
        let mut writer = BufWriter::new(&mut temp_file);
        writer.write_all(json.as_bytes())?;
        writer.flush()?; // Ensure everything's written to the OS buffers
    }

    // Ensure file contents hit disk
    temp_file.as_file_mut().sync_all()?; 

    // Atomically replace the original file
    temp_file.persist(&final_path)?;

    Ok(())
}

pub fn read_notepad_text(exe_path: &PathBuf) -> Result<String, Box<dyn Error>> {
    let dir_path: PathBuf = crate::tasks::get_data_dir(exe_path)?;
    
    let file_path = dir_path.join("notepad_text.json");
    
    if !file_path.exists() {
        let mut file = File::create(&file_path).expect("failed to create notepad_text JSON file");
        file.write_all(b"{}").expect("failed to write to notepad_text JSON file");
    }

    let file = File::open(&file_path)?;
    let reader = BufReader::new(file);

    let text: String = serde_json::from_reader(reader)?;
    return Ok(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn float_pair_array_emits_numeric_toml_array() {
        let arr = float_pair_array([12.5, -3.0]);
        // Two real float entries, not a stringified "[..]".
        assert_eq!(arr.len(), 2);
        assert_eq!(arr.get(0).and_then(|v| v.as_float()), Some(12.5));
        assert_eq!(arr.get(1).and_then(|v| v.as_float()), Some(-3.0));
        // Rendered as a TOML array, parseable back by the config reader.
        let rendered = toml_edit::value(arr).to_string();
        assert!(rendered.contains('['), "rendered as array: {rendered}");
    }

    #[test]
    fn ordinal_suffix_basic_and_teens() {
        assert_eq!(ordinal_suffix(1), "st");
        assert_eq!(ordinal_suffix(2), "nd");
        assert_eq!(ordinal_suffix(3), "rd");
        assert_eq!(ordinal_suffix(4), "th");
        // The 11/12/13 teens are always "th" despite ending in 1/2/3.
        assert_eq!(ordinal_suffix(11), "th");
        assert_eq!(ordinal_suffix(12), "th");
        assert_eq!(ordinal_suffix(13), "th");
        assert_eq!(ordinal_suffix(21), "st");
        assert_eq!(ordinal_suffix(22), "nd");
        assert_eq!(ordinal_suffix(23), "rd");
        assert_eq!(ordinal_suffix(111), "th");
    }

    #[test]
    fn days_in_month_handles_leap_years_and_bounds() {
        assert_eq!(days_in_month(2024, 2), 29); // leap year
        assert_eq!(days_in_month(2025, 2), 28); // common year
        assert_eq!(days_in_month(2000, 2), 29); // divisible by 400
        assert_eq!(days_in_month(1900, 2), 28); // divisible by 100, not 400
        assert_eq!(days_in_month(2025, 1), 31);
        assert_eq!(days_in_month(2025, 4), 30);
        assert_eq!(days_in_month(2025, 12), 31); // December wraps to next year internally
        assert_eq!(days_in_month(2025, 0), 31); // out-of-range month -> safe fallback
        assert_eq!(days_in_month(2025, 13), 31); // out-of-range month -> safe fallback
    }

    #[test]
    fn parse_time_input_accepts_valid_noon() {
        // Noon is never inside a DST spring-forward gap, so this is unambiguous in
        // any local timezone the test happens to run in.
        let dt = parse_time_input(15, 6, 2025, 12, 30).expect("valid date should parse");
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn parse_time_input_rejects_impossible_dates() {
        assert!(parse_time_input(31, 2, 2025, 12, 0).is_err()); // Feb 31
        assert!(parse_time_input(30, 2, 2024, 12, 0).is_err()); // Feb 30, even in a leap year
        assert!(parse_time_input(15, 13, 2025, 12, 0).is_err()); // month 13
    }
}