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

/// Alpha of each step of an urgency ramp, least to most pressing, and of the
/// events slot.
///
/// These tint items drawn over a background photo, so opacity is half of what
/// makes a step read as louder than the one below it — the other half is the
/// colour. The top step is deliberately the most solid thing on the calendar.
const RAMP_ALPHA: [u8; 5] = [74, 88, 104, 120, 136];
const EVENT_ALPHA: u8 = 104;

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

        Self { name: "COLORSCHEME ZERO".to_string(), colors, is_user_configurable: false }
    }

    /// The schemes every install has, reinstalled at every startup by
    /// [`install_builtins`].
    ///
    /// `COLORSCHEME ZERO` stays first (id 0) so the untinted look remains the
    /// default and nobody's existing appearance changes; the rest are there to
    /// pick from. None of them is user-configurable: they are the floor you can
    /// always get back to, so they are duplicated rather than edited, and the
    /// manager lists them apart from the schemes you made.
    ///
    /// Indices are the palette slots `Active::calendar_item_color` selects:
    /// 0–4 run from least to most important, and 5 is events.
    ///
    /// **Each step has to be tellable from its neighbours at a glance**, which
    /// is the whole job of the ramp and what the first version of these got
    /// wrong: EMBER's amber and burnt orange differed by a hue nudge at nearly
    /// the same lightness, and on a small calendar pill over a photo they were
    /// one colour. Every step now moves on three axes at once — hue, lightness
    /// and alpha (`RAMP_ALPHA`) — so no two adjacent steps rely on any single
    /// one of them being noticed. Events sit off the ramp entirely, in the
    /// complementary direction, because an event is not a degree of urgency.
    pub fn builtin_schemes() -> Vec<Self> {
        // Takes the ramp as plain RGB and applies the shared alpha curve, so a
        // palette is edited as five colours rather than as twenty numbers, and
        // no scheme can quietly disagree with the others about opacity.
        let scheme = |name: &str, ramp: [[u8; 3]; 5], event: [u8; 3]| {
            let mut colors = [[0u8; 4]; 6];
            for (index, rgb) in ramp.iter().enumerate() {
                colors[index] = [rgb[0], rgb[1], rgb[2], RAMP_ALPHA[index]];
            }
            colors[5] = [event[0], event[1], event[2], EVENT_ALPHA];
            Self { name: name.to_string(), colors, is_user_configurable: false }
        };

        vec![
            Self::default_scheme(),
            // Cold ash climbing into a fire: grey → gold → orange → red.
            scheme(
                "EMBER",
                [
                    [ 84, 100, 118],   // slate, barely warm at all
                    [124, 124,  96],   // ochre
                    [190, 150,  56],   // gold
                    [212,  96,  36],   // orange
                    [206,  44,  56],   // red
                ],
                [ 72, 140, 196],       // events: cold blue against the whole ramp
            ),
            // Out at sea and coming ashore: deep water → shallows → sand → coral.
            scheme(
                "TIDE",
                [
                    [ 58,  86, 116],
                    [ 56, 128, 132],
                    [ 92, 170, 142],
                    [198, 176,  94],
                    [222,  92,  76],
                ],
                [124,  96, 190],       // events: violet
            ),
            // Forest floor to autumn: moss → lichen → gorse → rowan.
            scheme(
                "MOSS",
                [
                    [ 64,  82,  76],
                    [ 98, 126,  78],
                    [140, 166,  74],
                    [206, 174,  62],
                    [204,  92,  44],
                ],
                [ 86, 130, 178],       // events: sky
            ),
            // Nightfall: indigo → violet → orchid → magenta → rose.
            scheme(
                "DUSK",
                [
                    [ 70,  82, 124],
                    [100,  88, 158],
                    [162, 100, 172],
                    [206,  94, 142],
                    [228,  70,  92],
                ],
                [ 72, 162, 168],       // events: cyan
            ),
        ]
    }

    /// True for the schemes `builtin_schemes` ships. The stored flag is the
    /// inverse of "the user may edit, rename or delete this", which is exactly
    /// what being built in means here.
    pub fn is_builtin(&self) -> bool {
        !self.is_user_configurable
    }

    pub fn duplicate(&self) -> Self {
        Self {
            name: format!("{} (copy)", self.name),
            colors: self.colors,
            is_user_configurable: true,
        }
    }
    pub fn rename(&mut self, new_name: String) {
        self.name = new_name;
    }
}

/// Put the built-in schemes into `schemes`, at the ids reserved for them
/// (`builtin_schemes()[i]` lives at id `i`), and report whether anything
/// changed — the caller saves when it did.
///
/// Run at **every** startup, not only on a fresh install. Seeding them once
/// into an empty map meant anyone who already had a single scheme never saw
/// them at all, an edit to a built-in palette could never reach an existing
/// install, and a `colorschemes.json` written before the built-ins existed
/// stayed a one-entry file forever.
///
/// A user's own scheme sitting on a reserved id — which is what an install
/// predating the built-ins looks like — is **moved**, never overwritten:
/// nobody loses a palette to this. `selected_id` follows it, so the scheme the
/// user is looking at is still the one selected afterwards.
pub fn install_builtins(schemes: &mut HashMap<u32, ColorScheme>, selected_id: &mut u32) -> bool {
    let builtins = ColorScheme::builtin_schemes();
    let mut changed = false;

    // First id no built-in claims and no existing scheme occupies.
    let mut next_free = schemes
        .keys()
        .copied()
        .max()
        .unwrap_or(0)
        .max(builtins.len() as u32 - 1)
        + 1;

    for (index, builtin) in builtins.into_iter().enumerate() {
        let id = index as u32;

        match schemes.get(&id) {
            // The built-in is already there, possibly in an older definition:
            // replace it, so palette corrections reach installs that have run
            // before. Nothing is lost — a built-in holds no user decisions.
            Some(existing) if existing.name == builtin.name => {
                if existing.colors != builtin.colors
                    || existing.is_user_configurable != builtin.is_user_configurable
                {
                    schemes.insert(id, builtin);
                    changed = true;
                }
            }
            // Something else is on the id. Move it out of the way and keep the
            // selection pointing at it.
            Some(_) => {
                let displaced = schemes.remove(&id).expect("just matched");
                let new_id = next_free;
                next_free += 1;

                if *selected_id == id {
                    *selected_id = new_id;
                }
                schemes.insert(new_id, displaced);
                schemes.insert(id, builtin);
                changed = true;
            }
            None => {
                schemes.insert(id, builtin);
                changed = true;
            }
        }
    }

    changed
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
#[cfg(test)]
mod tests {
    use super::*;

    /// How far apart two palette entries have to be to count as different
    /// colours, as the sum of their per-channel differences. Small pills over a
    /// tinted photograph are a hostile place to read a colour, so the bar is
    /// well above "a nudge of hue": at 60 the two are visibly different
    /// swatches rather than two shades of one.
    const MIN_STEP_DISTANCE: i32 = 60;

    fn distance(a: [u8; 4], b: [u8; 4]) -> i32 {
        (0..3)
            .map(|channel| (a[channel] as i32 - b[channel] as i32).abs())
            .sum()
    }

    #[test]
    fn every_urgency_step_is_tellable_from_its_neighbour() {
        for scheme in ColorScheme::builtin_schemes() {
            if scheme.name == "COLORSCHEME ZERO" {
                continue; // the untinted one is transparent by design
            }
            for step in 0..4 {
                let gap = distance(scheme.colors[step], scheme.colors[step + 1]);
                assert!(
                    gap >= MIN_STEP_DISTANCE,
                    "{}: steps {step} and {} are {gap} apart",
                    scheme.name,
                    step + 1
                );
            }
        }
    }

    #[test]
    fn events_sit_off_the_urgency_ramp() {
        for scheme in ColorScheme::builtin_schemes() {
            if scheme.name == "COLORSCHEME ZERO" {
                continue;
            }
            for step in 0..5 {
                let gap = distance(scheme.colors[5], scheme.colors[step]);
                assert!(
                    gap >= MIN_STEP_DISTANCE,
                    "{}: events read as urgency step {step} ({gap} apart)",
                    scheme.name
                );
            }
        }
    }

    #[test]
    fn urgency_gets_more_solid_as_it_rises() {
        for scheme in ColorScheme::builtin_schemes() {
            if scheme.name == "COLORSCHEME ZERO" {
                continue;
            }
            for step in 0..4 {
                assert!(
                    scheme.colors[step][3] < scheme.colors[step + 1][3],
                    "{}: step {step} is no less solid than the one above it",
                    scheme.name
                );
            }
        }
    }

    #[test]
    fn builtins_are_installed_once_and_then_left_alone() {
        let mut schemes = HashMap::new();
        let mut selected = 0;

        assert!(install_builtins(&mut schemes, &mut selected));
        assert_eq!(schemes.len(), ColorScheme::builtin_schemes().len());
        assert_eq!(schemes[&0].name, "COLORSCHEME ZERO");
        assert!(schemes.values().all(ColorScheme::is_builtin));

        // Idempotent: a second run has nothing to do, so nothing is saved.
        assert!(!install_builtins(&mut schemes, &mut selected));
    }

    #[test]
    fn an_older_definition_of_a_builtin_is_refreshed() {
        let mut schemes = HashMap::new();
        let mut selected = 1;
        install_builtins(&mut schemes, &mut selected);

        // What an install that ran an earlier version holds: the right name,
        // last version's colours, and editable.
        let stale = ColorScheme {
            name: "EMBER".to_string(),
            colors: [[1, 2, 3, 4]; 6],
            is_user_configurable: true,
        };
        schemes.insert(1, stale);

        assert!(install_builtins(&mut schemes, &mut selected));
        assert_eq!(schemes[&1].colors, ColorScheme::builtin_schemes()[1].colors);
        assert!(schemes[&1].is_builtin());
        assert_eq!(selected, 1, "refreshing in place must not move the selection");
    }

    #[test]
    fn a_users_scheme_on_a_reserved_id_is_moved_not_overwritten() {
        // An install from before the built-ins existed: id 0 is the untinted
        // default, and the user's own scheme took id 1.
        let mine = ColorScheme {
            name: "MINE".to_string(),
            colors: [[9, 9, 9, 90]; 6],
            is_user_configurable: true,
        };
        let mut schemes = HashMap::from([
            (0, ColorScheme::default_scheme()),
            (1, mine.clone()),
        ]);
        let mut selected = 1;

        assert!(install_builtins(&mut schemes, &mut selected));

        assert_eq!(schemes[&1].name, "EMBER", "the built-in takes its reserved id");
        assert_ne!(selected, 1, "the selection follows the scheme that moved");
        let moved = &schemes[&selected];
        assert_eq!(moved.name, "MINE");
        assert_eq!(moved.colors, mine.colors);
        assert!(moved.is_user_configurable);
    }
}
