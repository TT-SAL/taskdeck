use palette::{Srgb};
use std::{collections::HashMap, error::Error, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::Path};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::paths::AppDirs;
use image::{GenericImageView, Pixel};
use kmeans_colors::{get_kmeans_hamerly};
use palette::{FromColor, Lab};

#[derive(Serialize, Deserialize, Clone)]
pub struct ColorScheme {
    pub name: String,
    pub colors: [[u8; 4]; 6],
    pub is_user_configurable: bool,
}

impl ColorScheme {
    pub fn default_scheme() -> Self {
        let colors: [[u8; 4]; 6] = [
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ];

        Self { name: "COLORSCHEME ZERO".to_string(), colors, is_user_configurable: true }
    }

    /// The schemes a fresh install starts with.
    ///
    /// Previously a first run got only `default_scheme` — six fully transparent
    /// entries — so the colour-scheme manager opened on a single palette that
    /// tinted nothing, and looked broken rather than empty. `COLORSCHEME ZERO`
    /// stays first (id 0) so the untinted look remains the default and nobody's
    /// existing appearance changes; the rest are there to pick from.
    ///
    /// Indices are the palette slots `Active::calendar_item_color` selects:
    /// 0–4 run from least to most important, and 5 is events. Each ramp goes
    /// quiet→loud in that order so a glance at the calendar reads as urgency,
    /// and events sit clearly apart from the ramp. Alphas stay in the 70–110
    /// range: these tint items over a background photo, so they have to colour
    /// without hiding it.
    pub fn builtin_schemes() -> Vec<Self> {
        let scheme = |name: &str, colors: [[u8; 4]; 6]| Self {
            name: name.to_string(),
            colors,
            is_user_configurable: true,
        };

        vec![
            Self::default_scheme(),
            scheme("EMBER", [
                [ 96, 108, 122,  75],   // slate
                [126, 122,  96,  80],   // ochre
                [176, 132,  62,  90],   // amber
                [196,  92,  46, 100],   // burnt orange
                [188,  56,  50, 110],   // ember red
                [ 92, 132, 168,  90],   // events: cool blue against the warm ramp
            ]),
            scheme("TIDE", [
                [ 74, 110, 118,  75],   // deep teal
                [ 78, 132, 128,  80],   // sea green
                [ 96, 152, 152,  90],   // shallow water
                [150, 160, 116, 100],   // kelp
                [206, 158,  86, 110],   // sand, the loudest thing on a coast
                [116,  96, 156,  90],   // events: violet
            ]),
            scheme("MOSS", [
                [ 88, 100,  84,  75],
                [104, 124,  88,  80],
                [128, 148,  92,  90],
                [166, 158,  84, 100],
                [188, 124,  64, 110],
                [ 96, 124, 156,  90],
            ]),
            scheme("DUSK", [
                [ 84,  92, 124,  75],
                [104,  98, 140,  80],
                [134, 104, 152,  90],
                [168, 108, 148, 100],
                [198, 104, 122, 110],
                [ 96, 152, 160,  90],
            ]),
        ]
    }
    pub fn duplicate(&self) -> Self {
        Self {
            name: format!("DUPLICATE - '{}'", self.name),
            colors: self.colors,
            is_user_configurable: true,
        }
    }
    pub fn rename(&mut self, new_name: String) {
        self.name = new_name;
    }
}

pub fn save_colorschemes(payload: &HashMap<u32, ColorScheme>, data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let final_path = data_dir.join("colorschemes.json");

    // Ensure the directory exists (it may have been removed while running)
    fs::create_dir_all(data_dir)?;

    // Serialize first to avoid writing an invalid file
    let json = serde_json::to_string_pretty(payload)?;

    // Write to a temporary file first
    let mut temp_file = NamedTempFile::new_in(data_dir)?;
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

pub fn read_colorschemes(data_dir: &Path) -> Result<HashMap<u32, ColorScheme>, Box<dyn Error>> {
    let file_path = data_dir.join("colorschemes.json");

    if !file_path.exists() {
        // An empty JSON object is what a "no saved schemes" file looks like; a
        // failure to seed it is reported like any other read failure rather than
        // aborting the boot.
        fs::write(&file_path, b"{}")?;
    }

    let file = File::open(&file_path)?;
    let reader = BufReader::new(file);

    let schemes: HashMap<u32, ColorScheme> = serde_json::from_reader(reader)?;

    return Ok(schemes);
}

pub fn generate_colorscheme(dirs: &AppDirs, name: String) -> Option<ColorScheme> {
    // Confine the lookup to `images/` (defends against path traversal).
    let path = dirs.image_path(&name)?;

    let image_bytes = fs::read(&path).ok()?;
    let image = image::load_from_memory(&image_bytes).ok()?;

    // --- 1. Resize to suppress noise ---
    let image = image.resize(200, 200, image::imageops::FilterType::Triangle);

    // --- 2. Collect Lab pixels ---
    let mut pixels = Vec::new();

    for (_, _, pixel) in image.pixels() {
        let rgba = pixel.to_rgba();
        let alpha = rgba[3];

        // Ignore transparent pixels
        if alpha < 200 {
            continue;
        }

        let srgb = Srgb::new(
            rgba[0] as f32 / 255.0,
            rgba[1] as f32 / 255.0,
            rgba[2] as f32 / 255.0,
        );

        pixels.push(Lab::from_color(srgb));
    }

    if pixels.len() < 500 {
        #[cfg(debug_assertions)]
        eprintln!("Not enough usable pixels in {:?}", path);
        return None;
    }

    // --- 3. K-means in Lab space ---
    let kmeans = get_kmeans_hamerly(
        6,      // number of clusters
        20,     // max iterations
        0.002,  // convergence threshold
        false,  // no verbose output
        &pixels,
        42,     // deterministic seed
    );

    // --- 4. Compute cluster populations ---
    let mut counts = vec![0usize; kmeans.centroids.len()];
    for &cluster_idx in &kmeans.indices {
        counts[cluster_idx as usize] += 1;
    }

    let mut clusters: Vec<(Lab, usize)> = kmeans
        .centroids
        .into_iter()
        .zip(counts)
        .collect();

    // --- 5. Sort by UI visual significance (least → most) ---
    clusters.sort_by(|(a_lab, a_count), (b_lab, b_count)| {
        let a_score = cluster_score(*a_lab, *a_count);
        let b_score = cluster_score(*b_lab, *b_count);
        a_score.partial_cmp(&b_score).unwrap()
    });

    // --- 6. Convert to RGBA fills ---
    let colors: [[u8; 4]; 6] = clusters
        .iter()
        .map(|(lab, _)| {
            let srgb: Srgb = Srgb::from_color(*lab);

            let r = (srgb.red.clamp(0.0, 1.0) * 255.0) as u8;
            let g = (srgb.green.clamp(0.0, 1.0) * 255.0) as u8;
            let b = (srgb.blue.clamp(0.0, 1.0) * 255.0) as u8;

            // Tuned for background UI overlays
            [r, g, b, 80]
        })
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;

    Some(ColorScheme {
        colors,
        name: format!("Scheme from \"{}\"", name),
        is_user_configurable: true,
    })
}

/// Higher score = more visually prominent
fn cluster_score(lab: Lab, population: usize) -> f32 {
    let pop = population as f32;

    let saturation = (lab.a * lab.a + lab.b * lab.b).sqrt();
    let luminance = lab.l;

    // Heuristic tuned for UI fills over the same image
    pop * 0.6
        + saturation * 0.2
        + (luminance - 50.0).abs() * 0.2
}