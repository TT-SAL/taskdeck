use std::{collections::HashMap, error::Error, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::PathBuf};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use egui::Color32;
use tempfile::NamedTempFile;

use crate::color::ColorScheme;

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