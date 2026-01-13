use std::{error::Error, fs::{self, File, OpenOptions}, io::{BufReader, BufWriter, Write}, path::PathBuf};
use chrono::{DateTime, Local};
use rev_lines::RevLines;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Active {
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
}

impl Active {
    pub fn importance_score(&self, time_now: DateTime<Local>) -> f32 {
        let  score = match (self.importance, self.time_importance, self.deadline) {
            (Some(importance), _, Some(deadline)) => {
                let days_since_creation = (deadline - time_now).num_hours() as f32 / 24.0;

                match importance {
                    4 => 1.2_f32.powf(0.5 * days_since_creation + 20.0) + 5.0,
                    3 => 1.17_f32.powf(0.5 * days_since_creation + 20.0) + 5.0,
                    2 => 0.1747502645671 * days_since_creation + 11.3587671968606,
                    1 => 0.0965675735297 * days_since_creation + 6.276892278847,
                    _ => 0.0402194752135 * days_since_creation + 2.6142658953751,
                }
            },
            (_, Some(time_importance), _) => {
                let days_since_creation = (time_now - self.created).num_hours() as f32 / 24.0;

                match time_importance {
                    2 => 1.15_f32.powf(0.4 * days_since_creation + 20.0) - 5.0,
                    1 => 0.5403960772338 * days_since_creation + 8.3798162245677,
                    _ => 0.0440665332331 * days_since_creation + 0.6833311078751,
                }
            },
            (None, None, Some(deadline)) => {
                let time_until_event = (deadline - time_now).abs().num_hours() as f32 / 24.0 + 1.0;
                1000000000.0 / time_until_event
            }
            _ => 1000000000.0, //highlight broken entries
        };

        let random_time = chrono::Local::now();
        let random_variation = (random_time.timestamp_subsec_millis() as f32 / 10000.0) + 1.0;

        return score * random_variation;
    }
    pub fn to_inactive(self) -> InActive {
        InActive { 
            importance: self.importance,
            name: self.name,
            created: self.created,
            deadline: self.deadline,
            is_event: self.is_event,
            inactivated: chrono::Local::now(),
        }
    }
    pub fn calendar_item_color(&self) -> usize {
        if self.is_event {
            5
        } else if let Some(importance) = self.importance {
            importance as usize
        } else if let Some(time_importance) = self.time_importance {
            time_importance as usize
        } else {
            0
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InActive {
    pub importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
    pub inactivated: DateTime<Local>,
}


pub fn get_data_dir(exe_path: &PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = exe_path.parent().ok_or("Could not find exe directory")?;
    let data_in_exe_dir = exe_dir.join("taskdeck_data");

    if data_in_exe_dir.exists() {
        return Ok(data_in_exe_dir);
    }

    // Fallback for development (e.g., target/debug/app)
    let maybe_project_root = exe_dir
        .parent() // target/
        .and_then(|p| p.parent()); // project root

    let dev_data_path = maybe_project_root
        .ok_or("Could not determine project root for dev mode")?
        .join("taskdeck_data");

    if dev_data_path.exists() {
        Ok(dev_data_path)
    } else {
        Err("Could not locate 'data' directory".into())
    }
}

pub fn read_at_startup(exe_path: &PathBuf) -> Result<Vec<Active>, Box<dyn Error>> {
    let dir_path: PathBuf = get_data_dir(exe_path)?;
    
    let file_path = dir_path.join("read_at_startup.json");
    
    if !file_path.exists() {
        let mut file = File::create(&file_path).expect("failed to create active save JSON file");
        file.write_all(b"[]").expect("failed to write to JSON file");
    }

    let file = File::open(&file_path)?;
    let reader = BufReader::new(file);

    let read_at_startup: Vec<Active> = serde_json::from_reader(reader)?;

    return Ok(read_at_startup);
}

pub fn oversafe_activesave(payload: &Vec<Active>, exe_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    // Determine the path to the target JSON file
    let data_dir = get_data_dir(exe_path)?;

    let final_path = data_dir.join("read_at_startup.json");

    // Ensure the directory exists
    fs::create_dir_all(&data_dir)?;

    // Serialize first to avoid writing an invalid file
    let json = serde_json::to_string_pretty(payload)?;

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

pub fn save_inactive(payload: &InActive, exe_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let data_dir = get_data_dir(exe_path)?;
    let final_path = data_dir.join("archived.jsonl");

    // Ensure the directory exists
    fs::create_dir_all(&data_dir)?;

    let mut json = serde_json::to_string(payload)?;
    json.push_str("\n");

    let mut file = OpenOptions::new().create(true).append(true).open(final_path)?;

    {
        let mut writer = BufWriter::new(&mut file);
        writer.write_all(json.as_bytes())?;
        writer.flush()?;
    }

    Ok(file.sync_all()?)
}

pub fn read_lines_range(offset: usize, limit: usize, exe_path: &PathBuf) -> Result<Vec<InActive>, Box<dyn Error>> {
    let data_dir = get_data_dir(exe_path)?;
    let path = data_dir.join("archived.jsonl");

    let file = File::open(path)?;
    let rev_lines = RevLines::new(file);

    let archives: Vec<InActive> = rev_lines
        .skip(offset)
        .take(limit)
        .filter_map(|line| serde_json::from_str::<InActive>(&line.ok()?).ok())
        .collect();

    Ok(archives)
}

