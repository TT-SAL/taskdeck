use std::{
    sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}},
    thread,
    time::Duration,
};

use chrono::NaiveDateTime;
use egui::ImageSource;
use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use serde::Deserialize;
use winit::event_loop::EventLoopProxy;

use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug, Deserialize)]
struct WeatherResponse {
    hourly: HourlyData,
}

#[derive(Debug, Deserialize)]
struct HourlyData {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<i32>,
    is_day: Vec<i32>,
}

#[derive(Clone)]
pub struct WeatherData {
    pub is_day: i32,
    pub temp: f64,
    pub weather_code: i32,
    pub time: String,
}

enum WeatherCommand {
    SetCoordinates([f32; 2]),
    Stop,
}

pub struct WeatherService {
    pub data: Arc<RwLock<Vec<Vec<WeatherData>>>>,
    pub version: Arc<AtomicU64>,
    tx: Sender<WeatherCommand>,
}

impl WeatherService {
    pub fn set_coordinates(&self, coords: [f32; 2]) {
        let _ = self.tx.send(WeatherCommand::SetCoordinates(coords));
    }
}

impl Drop for WeatherService {
    fn drop(&mut self) {
        let _ = self.tx.send(WeatherCommand::Stop);
    }
}

fn fetch_weather_once(
    client: &Client,
    coordinates: [f32; 2],
) -> Result<Vec<Vec<WeatherData>>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
        ?latitude={}&longitude={}\
        &hourly=temperature_2m,weather_code,is_day\
        &timezone=auto&forecast_days=3",
        coordinates[0], coordinates[1]
    );

    let resp = client
        .get(&url)
        .header(USER_AGENT, "egui-weather-app")
        .send()?
        .error_for_status()?;

    let bytes = resp.bytes()?;
    let json = serde_json::from_slice::<WeatherResponse>(&bytes)?;

    let mut new_data = vec![vec![]; 24];

    for i in 0..json.hourly.time.len() {
        let item = WeatherData {
            time: {
                let input = json.hourly.time.get(i).cloned().unwrap_or_default();
                NaiveDateTime::parse_from_str(&input, "%Y-%m-%dT%H:%M")?.format("%H:%M").to_string()
            },
            temp: *json.hourly.temperature_2m.get(i).unwrap_or(&0.0),
            weather_code: *json.hourly.weather_code.get(i).unwrap_or(&0),
            is_day: *json.hourly.is_day.get(i).unwrap_or(&0),
        };

        new_data[i % 24].push(item);
    }

    Ok(new_data)
}

pub fn get_weather(initial_coordinates: [f32; 2], proxy: EventLoopProxy<()>) -> WeatherService {
    const REFRESH_INTERVAL: Duration = Duration::from_secs(600);
    const MAX_RETRIES: u32 = 3;

    let data = Arc::new(RwLock::new(vec![vec![]]));
    let data_clone = Arc::clone(&data);

    let version = Arc::new(AtomicU64::new(0));
    let version_clone = Arc::clone(&version);

    let (tx, rx): (Sender<WeatherCommand>, Receiver<WeatherCommand>) = channel();

    thread::spawn(move || {
        let client = match Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to build HTTP client: {}", e);
                return;
            }
        };

        let mut coordinates = initial_coordinates;

        loop {
            let mut success = false;

            for attempt in 0..MAX_RETRIES {
                match fetch_weather_once(&client, coordinates) {
                    Ok(new_data) => {
                        if let Ok(mut w) = data_clone.write() {
                            *w = new_data;
                        }
                        version_clone.fetch_add(1, Ordering::Relaxed);

                        let _ = proxy.send_event(());

                        #[cfg(debug_assertions)] {
                            println!("Weather thread updating!");
                        }

                        success = true;
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "Weather fetch failed (attempt {}): {}",
                            attempt + 1,
                            e
                        );

                        let backoff = Duration::from_secs(2u64.pow(attempt));
                        thread::sleep(backoff);
                    }
                }
            }

            if !success {
                eprintln!("Weather update failed after retries; keeping old data");
            }

            match rx.recv_timeout(REFRESH_INTERVAL) {
                Ok(WeatherCommand::SetCoordinates(c)) => {
                    coordinates = c;
                    continue;
                }
                Ok(WeatherCommand::Stop) => break,
                Err(_) => {}
            }
        }
    });

    WeatherService {
        data,
        version,
        tx,
    }
}

// SVG Weather Symbol File Naming Reference
// ==========================================
// The weather_svgs_2 directory contains SVG files using the following naming scheme:
//
// Day/Night/Twilight variants (with d/n/m suffixes):
// - {code}d.svg: Day variant (sun is visible)
// - {code}n.svg: Night variant (sun is not visible)
// - {code}m.svg: Polar twilight variant (sun slightly below horizon)
//
// Weather codes and their meanings:
// 01 - Clear sky (clearsky)
// 02 - Fair/Light clouds (fair)
// 03 - Partly cloudy (partlycloudy)
// 04 - Overcast/Cloudy (cloudy) - no day/night variant
// 05 - Rain showers (rainshowers)
// 06 - Rain showers and thunder (rainshowersandthunder)
// 07 - Sleet showers (sleetshowers)
// 08 - Snow showers (snowshowers)
// 09 - Rain (rain) - no day/night variant
// 10 - Heavy rain (heavyrain) - no day/night variant
// 11 - Heavy rain and thunder (heavyrainandthunder) - no day/night variant
// 12 - Sleet (sleet) - no day/night variant
// 13 - Snow (snow) - no day/night variant
// 14 - Snow and thunder (snowandthunder) - no day/night variant
// 15 - Fog (fog) - no day/night variant
// 20 - Sleet showers and thunder (sleetshowersandthunder)
// 21 - Snow showers and thunder (snowshowersandthunder)
// 22 - Rain and thunder (rainandthunder) - no day/night variant
// 23 - Sleet and thunder (sleetandthunder) - no day/night variant
// 24 - Light rain showers and thunder (lightrainshowersandthunder)
// 25 - Heavy rain showers and thunder (heavyrainshowersandthunder)
// 26 - Light sleet showers and thunder (lightssleetshowersandthunder)
// 27 - Heavy sleet showers and thunder (heavysleetshowersandthunder)
// 28 - Light snow showers and thunder (lightssnowshowersandthunder)
// 29 - Heavy snow showers and thunder (heavysnowshowersandthunder)
// 30 - Light rain and thunder (lightrainandthunder) - no day/night variant
// 31 - Light sleet and thunder (lightsleetandthunder) - no day/night variant
// 32 - Heavy sleet and thunder (heavysleetandthunder) - no day/night variant
// 33 - Light snow and thunder (lightsnowandthunder) - no day/night variant
// 34 - Heavy snow and thunder (heavysnowandthunder) - no day/night variant
// 40 - Light rain showers (lightrainshowers)
// 41 - Heavy rain showers (heavyrainshowers)
// 42 - Light sleet showers (lightsleetshowers)
// 43 - Heavy sleet showers (heavysleetshowers)
// 44 - Light snow showers (lightsnowshowers)
// 45 - Heavy snow showers (heavysnowshowers)
// 46 - Light rain (lightrain) - no day/night variant
// 47 - Light sleet (lightsleet) - no day/night variant
// 48 - Heavy sleet (heavysleet) - no day/night variant
// 49 - Light snow (lightsnow) - no day/night variant
// 50 - Heavy snow (heavysnow) - no day/night variant

static CLEAR_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/01d.svg");
static CLEAR_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/01n.svg");

static FAIR_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/02d.svg");
static FAIR_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/02n.svg");

static PARTLY_CLOUDY_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/03d.svg");
static PARTLY_CLOUDY_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/03n.svg");

static CLOUDY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/04.svg");

static FOG: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/15.svg");

// --- Rain ---
static LIGHT_RAIN: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/46.svg");
static RAIN: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/09.svg");
static HEAVY_RAIN: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/10.svg");

// --- Sleet ---
static LIGHT_SLEET: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/47.svg");
static HEAVY_SLEET: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/48.svg");

// --- Snow ---
static LIGHT_SNOW: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/49.svg");
static SNOW: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/13.svg");
static HEAVY_SNOW: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/50.svg");

// --- Showers ---
static LIGHT_RAIN_SHOWERS_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/40d.svg");
static LIGHT_RAIN_SHOWERS_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/40n.svg");

static RAIN_SHOWERS_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/05d.svg");
static RAIN_SHOWERS_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/05n.svg");

static HEAVY_RAIN_SHOWERS_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/41d.svg");
static HEAVY_RAIN_SHOWERS_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/41n.svg");

static LIGHT_SNOW_SHOWERS_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/44d.svg");
static LIGHT_SNOW_SHOWERS_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/44n.svg");

static HEAVY_SNOW_SHOWERS_DAY: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/45d.svg");
static HEAVY_SNOW_SHOWERS_NIGHT: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/45n.svg");

// --- Thunder ---
static RAIN_THUNDER: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/22.svg");
static HEAVY_RAIN_THUNDER: ImageSource<'static> =
    egui::include_image!("../weather_svgs_2/11.svg");


pub fn icon_for_wmo(
    code: i32,
    is_day: bool,
) -> &'static egui::ImageSource<'static> {
    match code {
        // --- Clear & clouds ---
        0 => if is_day { &CLEAR_DAY } else { &CLEAR_NIGHT },
        1 => if is_day { &FAIR_DAY } else { &FAIR_NIGHT },
        2 => if is_day { &PARTLY_CLOUDY_DAY } else { &PARTLY_CLOUDY_NIGHT },
        3 => &CLOUDY,

        // --- Fog ---
        45 | 48 => &FOG,

        // --- Drizzle ---
        51 | 53 => &LIGHT_RAIN,
        55 => &RAIN,
        56 | 57 => &LIGHT_SLEET,

        // --- Rain ---
        61 => &LIGHT_RAIN,
        63 => &RAIN,
        65 => &HEAVY_RAIN,
        66 => &LIGHT_SLEET,
        67 => &HEAVY_SLEET,

        // --- Snow ---
        71 | 77 => &LIGHT_SNOW,
        73 => &SNOW,
        75 => &HEAVY_SNOW,

        // --- Rain showers ---
        80 => if is_day { &LIGHT_RAIN_SHOWERS_DAY } else { &LIGHT_RAIN_SHOWERS_NIGHT },
        81 => if is_day { &RAIN_SHOWERS_DAY } else { &RAIN_SHOWERS_NIGHT },
        82 => if is_day { &HEAVY_RAIN_SHOWERS_DAY } else { &HEAVY_RAIN_SHOWERS_NIGHT },

        // --- Snow showers ---
        85 => if is_day { &LIGHT_SNOW_SHOWERS_DAY } else { &LIGHT_SNOW_SHOWERS_NIGHT },
        86 => if is_day { &HEAVY_SNOW_SHOWERS_DAY } else { &HEAVY_SNOW_SHOWERS_NIGHT },

        // --- Thunderstorms ---
        95 => &RAIN_THUNDER,
        96 | 99 => &HEAVY_RAIN_THUNDER,

        // --- Fallback ---
        _ => &CLOUDY,
    }
}

pub struct City {
    pub name: &'static str,
    pub latitude: f32,
    pub longitude: f32,
}

pub static CITIES: &[City] = &[
    City { name: "Mumbai", latitude: 19.0760, longitude: 72.8777 },
    City { name: "Delhi", latitude: 28.7041, longitude: 77.1025 },
    City { name: "Bangalore", latitude: 12.9716, longitude: 77.5946 },
    City { name: "Hyderabad", latitude: 17.3850, longitude: 78.4867 },
    City { name: "Ahmedabad", latitude: 23.0225, longitude: 72.5714 },

    City { name: "Shanghai", latitude: 31.2304, longitude: 121.4737 },
    City { name: "Beijing", latitude: 39.9042, longitude: 116.4074 },
    City { name: "Guangzhou", latitude: 23.1291, longitude: 113.2644 },
    City { name: "Shenzhen", latitude: 22.5431, longitude: 114.0579 },
    City { name: "Chengdu", latitude: 30.5728, longitude: 104.0668 },

    City { name: "Jakarta", latitude: -6.2088, longitude: 106.8456 },
    City { name: "Surabaya", latitude: -7.2575, longitude: 112.7521 },
    City { name: "Bandung", latitude: -6.9175, longitude: 107.6191 },
    City { name: "Medan", latitude: 3.5952, longitude: 98.6722 },
    City { name: "Semarang", latitude: -6.9667, longitude: 110.4167 },

    City { name: "Karachi", latitude: 24.8607, longitude: 67.0011 },
    City { name: "Lahore", latitude: 31.5546, longitude: 74.3572 },
    City { name: "Faisalabad", latitude: 31.4504, longitude: 73.1350 },
    City { name: "Rawalpindi", latitude: 33.5651, longitude: 73.0169 },
    City { name: "Multan", latitude: 30.1575, longitude: 71.5249 },

    City { name: "Lagos", latitude: 6.5244, longitude: 3.3792 },
    City { name: "Kano", latitude: 12.0022, longitude: 8.5919 },
    City { name: "Ibadan", latitude: 7.3775, longitude: 3.9470 },
    City { name: "Abuja", latitude: 9.0765, longitude: 7.3986 },
    City { name: "Port Harcourt", latitude: 4.8156, longitude: 7.0498 },

    City { name: "São Paulo", latitude: -23.5505, longitude: -46.6333 },
    City { name: "Rio de Janeiro", latitude: -22.9068, longitude: -43.1729 },
    City { name: "Brasília", latitude: -15.7939, longitude: -47.8828 },
    City { name: "Salvador", latitude: -12.9777, longitude: -38.5016 },
    City { name: "Fortaleza", latitude: -3.7319, longitude: -38.5267 },

    City { name: "Dhaka", latitude: 23.8103, longitude: 90.4125 },
    City { name: "Chittagong", latitude: 22.3569, longitude: 91.7832 },
    City { name: "Khulna", latitude: 22.8456, longitude: 89.5403 },
    City { name: "Rajshahi", latitude: 24.3700, longitude: 88.6241 },
    City { name: "Sylhet", latitude: 24.8949, longitude: 91.8687 },

    City { name: "Mexico City", latitude: 19.4326, longitude: -99.1332 },
    City { name: "Guadalajara", latitude: 20.6597, longitude: -103.3496 },
    City { name: "Monterrey", latitude: 25.6866, longitude: -100.3161 },
    City { name: "Puebla", latitude: 19.0413, longitude: -98.2062 },
    City { name: "Tijuana", latitude: 32.5149, longitude: -117.0382 },

    City { name: "Addis Ababa", latitude: 9.0300, longitude: 38.7400 },
    City { name: "Dire Dawa", latitude: 9.6000, longitude: 41.8500 },
    City { name: "Mekelle", latitude: 13.4999, longitude: 39.4758 },
    City { name: "Gondar", latitude: 12.6000, longitude: 37.4667 },
    City { name: "Bahir Dar", latitude: 11.5936, longitude: 37.3905 },

    City { name: "Manila", latitude: 14.5995, longitude: 120.9842 },
    City { name: "Quezon City", latitude: 14.6760, longitude: 121.0437 },
    City { name: "Caloocan", latitude: 14.7566, longitude: 120.9822 },
    City { name: "Davao", latitude: 7.1907, longitude: 125.4553 },
    City { name: "Cebu City", latitude: 10.3157, longitude: 123.8854 },

    City { name: "Tokyo", latitude: 35.6895, longitude: 139.6917 },
    City { name: "Yokohama", latitude: 35.4437, longitude: 139.6380 },
    City { name: "Osaka", latitude: 34.6937, longitude: 135.5023 },
    City { name: "Nagoya", latitude: 35.1815, longitude: 136.9066 },
    City { name: "Sapporo", latitude: 43.0618, longitude: 141.3545 },

    City { name: "Adelaide", latitude: -34.9285, longitude: 138.6007 },
    City { name: "Brisbane", latitude: -27.4705, longitude: 153.0260 },
    City { name: "Canberra", latitude: -35.2809, longitude: 149.1300 },
    City { name: "Melbourne", latitude: -37.8136, longitude: 144.9631 },
    City { name: "Sydney", latitude: -33.8688, longitude: 151.2093 },

    City { name: "Gaborone", latitude: -24.6282, longitude: 25.9231 },
    City { name: "Francistown", latitude: -21.1700, longitude: 27.5072 },

    City { name: "Calgary", latitude: 51.0447, longitude: -114.0719 },
    City { name: "Edmonton", latitude: 53.5461, longitude: -113.4938 },
    City { name: "Montreal", latitude: 45.5017, longitude: -73.5673 },
    City { name: "Ottawa", latitude: 45.4215, longitude: -75.6972 },
    City { name: "Toronto", latitude: 43.6532, longitude: -79.3832 },

    City { name: "Douala", latitude: 4.0511, longitude: 9.7679 },
    City { name: "Garoua", latitude: 9.3000, longitude: 13.4000 },
    City { name: "Kumba", latitude: 4.6400, longitude: 9.4500 },
    City { name: "Maroua", latitude: 10.5950, longitude: 14.3244 },
    City { name: "Yaoundé", latitude: 3.8480, longitude: 11.5021 },

    City { name: "Banjul", latitude: 13.4529, longitude: -16.5780 },
    City { name: "Serekunda", latitude: 13.4495, longitude: -16.6775 },

    City { name: "Accra", latitude: 5.6037, longitude: -0.1870 },
    City { name: "Kumasi", latitude: 6.6666, longitude: -1.6163 },
    City { name: "Tamale", latitude: 9.4000, longitude: -0.8393 },
    City { name: "Takoradi", latitude: 4.8997, longitude: -1.7600 },
    City { name: "Tema", latitude: 5.6667, longitude: -0.0167 },

    City { name: "Ahmedabad", latitude: 23.0225, longitude: 72.5714 },
    City { name: "Bangalore", latitude: 12.9716, longitude: 77.5946 },
    City { name: "Chennai", latitude: 13.0827, longitude: 80.2707 },
    City { name: "Delhi", latitude: 28.7041, longitude: 77.1025 },
    City { name: "Mumbai", latitude: 19.0760, longitude: 72.8777 },

    City { name: "Cork", latitude: 51.8985, longitude: -8.4756 },
    City { name: "Dublin", latitude: 53.3498, longitude: -6.2603 },
    City { name: "Galway", latitude: 53.2707, longitude: -9.0568 },
    City { name: "Limerick", latitude: 52.6680, longitude: -8.6305 },
    City { name: "Waterford", latitude: 52.2593, longitude: -7.1101 },

    City { name: "Kingston", latitude: 17.9712, longitude: -76.7936 },
    City { name: "Montego Bay", latitude: 18.4769, longitude: -77.9115 },

    City { name: "Eldoret", latitude: 0.5204, longitude: 35.2696 },
    City { name: "Kisumu", latitude: -0.0917, longitude: 34.7680 },
    City { name: "Machakos", latitude: -1.5167, longitude: 37.2667 },
    City { name: "Mombasa", latitude: -4.0435, longitude: 39.6682 },
    City { name: "Nairobi", latitude: -1.2921, longitude: 36.8219 },

    City { name: "Mafeteng", latitude: -29.8200, longitude: 27.4570 },
    City { name: "Maseru", latitude: -29.3158, longitude: 27.4854 },

    City { name: "Bensonville", latitude: 6.3400, longitude: -10.7600 },
    City { name: "Gbarnga", latitude: 7.0000, longitude: -9.5040 },
    City { name: "Harper", latitude: 4.3667, longitude: -7.7167 },
    City { name: "Monrovia", latitude: 6.3156, longitude: -10.8074 },
    City { name: "Tubmanburg", latitude: 6.9962, longitude: -10.1719 },

    City { name: "Blantyre", latitude: -15.7861, longitude: 35.0058 },
    City { name: "Lilongwe", latitude: -13.9833, longitude: 33.7833 },
    City { name: "Mzuzu", latitude: -11.4610, longitude: 34.0201 },
    City { name: "Zomba", latitude: -15.3833, longitude: 35.3333 },
    City { name: "Mangochi", latitude: -14.4814, longitude: 35.2644 },

    City { name: "Windhoek", latitude: -22.5609, longitude: 17.0658 },
    City { name: "Walvis Bay", latitude: -22.9576, longitude: 14.5058 },

    City { name: "Auckland", latitude: -36.8485, longitude: 174.7633 },
    City { name: "Christchurch", latitude: -43.5321, longitude: 172.6362 },
    City { name: "Dunedin", latitude: -45.8788, longitude: 170.5028 },
    City { name: "Hamilton", latitude: -37.7870, longitude: 175.2793 },
    City { name: "Wellington", latitude: -41.2865, longitude: 174.7762 },

    City { name: "Abuja", latitude: 9.0765, longitude: 7.3986 },
    City { name: "Ibadan", latitude: 7.3775, longitude: 3.9470 },
    City { name: "Kano", latitude: 12.0022, longitude: 8.5919 },
    City { name: "Lagos", latitude: 6.5244, longitude: 3.3792 },
    City { name: "Port Harcourt", latitude: 4.8156, longitude: 7.0498 },

    City { name: "Faisalabad", latitude: 31.4504, longitude: 73.1350 },
    City { name: "Islamabad", latitude: 33.6844, longitude: 73.0479 },
    City { name: "Karachi", latitude: 24.8607, longitude: 67.0011 },
    City { name: "Lahore", latitude: 31.5546, longitude: 74.3572 },
    City { name: "Multan", latitude: 30.1575, longitude: 71.5249 },

    City { name: "Cebu City", latitude: 10.3157, longitude: 123.8854 },
    City { name: "Davao City", latitude: 7.1907, longitude: 125.4553 },
    City { name: "Manila", latitude: 14.5995, longitude: 120.9842 },
    City { name: "Quezon City", latitude: 14.6760, longitude: 121.0437 },
    City { name: "Zamboanga City", latitude: 6.9214, longitude: 122.0790 },

    City { name: "Jurong East", latitude: 1.3330, longitude: 103.7420 },
    City { name: "Orchard", latitude: 1.3048, longitude: 103.8318 },
    City { name: "Pasir Ris", latitude: 1.3727, longitude: 103.9458 },
    City { name: "Singapore", latitude: 1.3521, longitude: 103.8198 },
    City { name: "Woodlands", latitude: 1.4369, longitude: 103.7861 },

    City { name: "Cape Town", latitude: -33.9249, longitude: 18.4241 },
    City { name: "Durban", latitude: -29.8587, longitude: 31.0218 },
    City { name: "Johannesburg", latitude: -26.2041, longitude: 28.0473 },
    City { name: "Port Elizabeth", latitude: -33.9715, longitude: 25.6022 },
    City { name: "Pretoria", latitude: -25.7479, longitude: 28.2293 },

    City { name: "Port of Spain", latitude: 10.6667, longitude: -61.5167 },

    City { name: "Entebbe", latitude: 0.0500, longitude: 32.4600 },
    City { name: "Gulu", latitude: 2.7724, longitude: 32.2881 },
    City { name: "Jinja", latitude: 0.4244, longitude: 33.2048 },
    City { name: "Kampala", latitude: 0.3476, longitude: 32.5825 },
    City { name: "Mbarara", latitude: -0.6076, longitude: 30.6548 },

    City { name: "Birmingham", latitude: 52.4862, longitude: -1.8904 },
    City { name: "Glasgow", latitude: 55.8642, longitude: -4.2518 },
    City { name: "Leeds", latitude: 53.8008, longitude: -1.5491 },
    City { name: "Liverpool", latitude: 53.4084, longitude: -2.9916 },
    City { name: "London", latitude: 51.5074, longitude: -0.1278 },

    City { name: "Chicago", latitude: 41.8781, longitude: -87.6298 },
    City { name: "Houston", latitude: 29.7604, longitude: -95.3698 },
    City { name: "Los Angeles", latitude: 34.0522, longitude: -118.2437 },
    City { name: "New York City", latitude: 40.7128, longitude: -74.0060 },
    City { name: "Phoenix", latitude: 33.4484, longitude: -112.0740 },

    City { name: "Durrës", latitude: 41.3231, longitude: 19.4414 },
    City { name: "Tirana", latitude: 41.3275, longitude: 19.8189 },

    City { name: "Antwerp", latitude: 51.2194, longitude: 4.4025 },
    City { name: "Bruges", latitude: 51.2093, longitude: 3.2247 },
    City { name: "Brussels", latitude: 50.8503, longitude: 4.3517 },
    City { name: "Charleroi", latitude: 50.4108, longitude: 4.4446 },
    City { name: "Liège", latitude: 50.6326, longitude: 5.5797 },

    City { name: "Burgas", latitude: 42.5048, longitude: 27.4626 },
    City { name: "Plovdiv", latitude: 42.1354, longitude: 24.7453 },
    City { name: "Ruse", latitude: 43.8510, longitude: 25.9740 },
    City { name: "Sofia", latitude: 42.6977, longitude: 23.3219 },
    City { name: "Varna", latitude: 43.2141, longitude: 27.9147 },

    City { name: "Rijeka", latitude: 45.3271, longitude: 14.4422 },
    City { name: "Split", latitude: 43.5081, longitude: 16.4402 },
    City { name: "Zagreb", latitude: 45.8150, longitude: 15.9785 },

    City { name: "Brno", latitude: 49.1951, longitude: 16.6068 },
    City { name: "Ostrava", latitude: 49.8347, longitude: 18.2920 },
    City { name: "Plzen", latitude: 49.7475, longitude: 13.3776 },
    City { name: "Prague", latitude: 50.0755, longitude: 14.4378 },
    City { name: "Usti nad Labem", latitude: 50.6600, longitude: 14.0410 },

    City { name: "Aarhus", latitude: 56.1629, longitude: 10.2039 },
    City { name: "Aalborg", latitude: 57.0488, longitude: 9.9217 },
    City { name: "Copenhagen", latitude: 55.6761, longitude: 12.5683 },
    City { name: "Odense", latitude: 55.4038, longitude: 10.4024 },
    City { name: "Esbjerg", latitude: 55.4765, longitude: 8.4594 },

    City { name: "Tallinn", latitude: 59.4370, longitude: 24.7536 },

    City { name: "Bordeaux", latitude: 44.8378, longitude: -0.5792 },
    City { name: "Lille", latitude: 50.6292, longitude: 3.0573 },
    City { name: "Lyon", latitude: 45.7640, longitude: 4.8357 },
    City { name: "Marseille", latitude: 43.2965, longitude: 5.3698 },
    City { name: "Paris", latitude: 48.8566, longitude: 2.3522 },

    City { name: "Berlin", latitude: 52.5200, longitude: 13.4050 },
    City { name: "Cologne", latitude: 50.9375, longitude: 6.9603 },
    City { name: "Frankfurt", latitude: 50.1109, longitude: 8.6821 },
    City { name: "Hamburg", latitude: 53.5511, longitude: 9.9937 },
    City { name: "Munich", latitude: 48.1351, longitude: 11.5820 },

    City { name: "Athens", latitude: 37.9838, longitude: 23.7275 },
    City { name: "Heraklion", latitude: 35.3387, longitude: 25.1442 },
    City { name: "Patras", latitude: 38.2466, longitude: 21.7346 },
    City { name: "Thessaloniki", latitude: 40.6401, longitude: 22.9444 },
    City { name: "Volos", latitude: 39.3617, longitude: 22.9424 },

    City { name: "Debrecen", latitude: 47.5316, longitude: 21.6273 },
    City { name: "Miskolc", latitude: 48.1031, longitude: 20.7784 },
    City { name: "Pécs", latitude: 46.0727, longitude: 18.2323 },
    City { name: "Szeged", latitude: 46.2530, longitude: 20.1414 },
    City { name: "Budapest", latitude: 47.4979, longitude: 19.0402 },

    City { name: "Bologna", latitude: 44.4949, longitude: 11.3426 },
    City { name: "Florence", latitude: 43.7696, longitude: 11.2558 },
    City { name: "Milan", latitude: 45.4642, longitude: 9.1900 },
    City { name: "Naples", latitude: 40.8518, longitude: 14.2681 },
    City { name: "Rome", latitude: 41.9028, longitude: 12.4964 },

    City { name: "Riga", latitude: 56.9496, longitude: 24.1052 },

    City { name: "Kaunas", latitude: 54.8985, longitude: 23.9036 },
    City { name: "Vilnius", latitude: 54.6872, longitude: 25.2797 },

    City { name: "Amsterdam", latitude: 52.3676, longitude: 4.9041 },
    City { name: "Eindhoven", latitude: 51.4416, longitude: 5.4697 },
    City { name: "Rotterdam", latitude: 51.9225, longitude: 4.47917 },
    City { name: "The Hague", latitude: 52.0705, longitude: 4.3007 },
    City { name: "Utrecht", latitude: 52.0907, longitude: 5.1214 },

    City { name: "Bitola", latitude: 41.0333, longitude: 21.3333 },
    City { name: "Skopje", latitude: 41.9981, longitude: 21.4254 },

    City { name: "Gdańsk", latitude: 54.3520, longitude: 18.6466 },
    City { name: "Kraków", latitude: 50.0647, longitude: 19.9450 },
    City { name: "Łódź", latitude: 51.7592, longitude: 19.4550 },
    City { name: "Poznań", latitude: 52.4064, longitude: 16.9252 },
    City { name: "Warsaw", latitude: 52.2297, longitude: 21.0122 },

    City { name: "Braga", latitude: 41.5454, longitude: -8.4265 },
    City { name: "Coimbra", latitude: 40.2033, longitude: -8.4103 },
    City { name: "Lisbon", latitude: 38.7169, longitude: -9.1396 },
    City { name: "Porto", latitude: 41.1579, longitude: -8.6291 },
    City { name: "Funchal", latitude: 32.6669, longitude: -16.9241 },

    City { name: "Bucharest", latitude: 44.4268, longitude: 26.1025 },
    City { name: "Cluj-Napoca", latitude: 46.7712, longitude: 23.6236 },
    City { name: "Iași", latitude: 47.1585, longitude: 27.6014 },
    City { name: "Timișoara", latitude: 45.7489, longitude: 21.2087 },
    City { name: "Constanța", latitude: 44.1598, longitude: 28.6348 },

    City { name: "Bratislava", latitude: 48.1486, longitude: 17.1077 },
    City { name: "Košice", latitude: 48.7164, longitude: 21.2611 },
    City { name: "Nitra", latitude: 48.3091, longitude: 18.0866 },
    City { name: "Prešov", latitude: 49.0000, longitude: 21.2333 },
    City { name: "Žilina", latitude: 49.2231, longitude: 18.7396 },

    City { name: "Ljubljana", latitude: 46.0569, longitude: 14.5058 },
    City { name: "Maribor", latitude: 46.5547, longitude: 15.6459 },

    City { name: "Barcelona", latitude: 41.3825, longitude: 2.1769 },
    City { name: "Madrid", latitude: 40.4168, longitude: -3.7038 },
    City { name: "Seville", latitude: 37.3886, longitude: -5.9823 },
    City { name: "Valencia", latitude: 39.4667, longitude: -0.3750 },
    City { name: "Zaragoza", latitude: 41.6561, longitude: -0.8773 },

    City { name: "Ankara", latitude: 39.9334, longitude: 32.8597 },
    City { name: "Bursa", latitude: 40.1950, longitude: 29.0600 },
    City { name: "Istanbul", latitude: 41.0082, longitude: 28.9784 },
    City { name: "Izmir", latitude: 38.4192, longitude: 27.1287 },
    City { name: "Konya", latitude: 37.8716, longitude: 32.4840 },

    City { name: "Aalborg", latitude: 57.0488, longitude: 9.9217 },
    City { name: "Aarhus", latitude: 56.1629, longitude: 10.2039 },
    City { name: "Copenhagen", latitude: 55.6761, longitude: 12.5683 },
    City { name: "Esbjerg", latitude: 55.4765, longitude: 8.4594 },
    City { name: "Frederiksberg", latitude: 55.6803, longitude: 12.5333 },
    City { name: "Helsingør", latitude: 56.0333, longitude: 12.6167 },
    City { name: "Odense", latitude: 55.4038, longitude: 10.4024 },
    City { name: "Randers", latitude: 56.4608, longitude: 10.0364 },
    City { name: "Silkeborg", latitude: 56.1705, longitude: 9.5452 },
    City { name: "Vejle", latitude: 55.7110, longitude: 9.5369 },

    City { name: "Espoo", latitude: 60.2055, longitude: 24.6559 },
    City { name: "Helsinki", latitude: 60.1695, longitude: 24.9355 },
    City { name: "Jyväskylä", latitude: 62.2426, longitude: 25.7473 },
    City { name: "Kuopio", latitude: 62.8924, longitude: 27.6780 },
    City { name: "Lahti", latitude: 60.9827, longitude: 25.6615 },
    City { name: "Oulu", latitude: 65.0121, longitude: 25.4651 },
    City { name: "Porvoo", latitude: 60.3938, longitude: 25.6636 },
    City { name: "Tampere", latitude: 61.4978, longitude: 23.7610 },
    City { name: "Turku", latitude: 60.4518, longitude: 22.2666 },
    City { name: "Vantaa", latitude: 60.2934, longitude: 25.0378 },

    City { name: "Bergen", latitude: 60.3913, longitude: 5.3221 },
    City { name: "Drammen", latitude: 59.7439, longitude: 10.2040 },
    City { name: "Fredrikstad", latitude: 59.2181, longitude: 10.9296 },
    City { name: "Kristiansand", latitude: 58.1467, longitude: 7.9956 },
    City { name: "Kristiansund", latitude: 63.1113, longitude: 7.7303 },
    City { name: "Oslo", latitude: 59.9139, longitude: 10.7522 },
    City { name: "Sandnes", latitude: 58.8517, longitude: 5.7385 },
    City { name: "Stavanger", latitude: 58.9690, longitude: 5.7331 },
    City { name: "Tromsø", latitude: 69.6496, longitude: 18.9560 },
    City { name: "Trondheim", latitude: 63.4305, longitude: 10.3951 },

    City { name: "Gothenburg", latitude: 57.7089, longitude: 11.9746 },
    City { name: "Helsingborg", latitude: 56.0465, longitude: 12.6945 },
    City { name: "Jönköping", latitude: 57.7815, longitude: 14.1562 },
    City { name: "Linköping", latitude: 58.4108, longitude: 15.6214 },
    City { name: "Lund", latitude: 55.7047, longitude: 13.1910 },
    City { name: "Malmö", latitude: 55.6050, longitude: 13.0038 },
    City { name: "Norrköping", latitude: 58.5877, longitude: 16.1929 },
    City { name: "Stockholm", latitude: 59.3293, longitude: 18.0686 },
    City { name: "Uppsala", latitude: 59.8586, longitude: 17.6389 },
    City { name: "Västerås", latitude: 59.6099, longitude: 16.5448 },
];