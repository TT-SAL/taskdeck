//! Today's weather report tests (weather.rs:1049-1215), ported.
//!
//! Not ported: `every_wmo_code_has_a_symbol_to_draw` and
//! `symbol_svg_is_a_table_and_not_a_path`, which test the app's symbol table
//! and SVG route (`SYMBOLS`, `symbol_svg`), not ported here.

use super::*;

/// The night is a different picture from the day, and only where there is
/// one to be had: overcast has no sun in it either way.
#[test]
fn day_and_night_differ_only_where_the_sun_shows() {
    assert_eq!(sky_symbol(0, true), "01d");
    assert_eq!(sky_symbol(0, false), "01n");
    assert_eq!(sky_symbol(3, true), sky_symbol(3, false));
    assert_eq!(sky_symbol(65, true), sky_symbol(65, false));
}

/// A response with holes in it is a shorter report, never an error: the
/// probability model is missing at some stations, the UV index stops a few
/// days out, and `current` is a block the API can decline to send. None of
/// that is a reason to leave the phone with no forecast at all.
#[test]
fn a_half_null_answer_still_builds_a_report() {
    let json = serde_json::json!({
        "timezone": "Europe/Helsinki",
        "utc_offset_seconds": 10800,
        "hourly": {
            "time": ["2026-09-10T00:00", "2026-09-10T01:00"],
            "temperature_2m": [11.0, 10.5],
            "weather_code": [3, 61],
            "is_day": [0, 0],
            "apparent_temperature": [9.4, null],
            "precipitation_probability": [null, 70],
            "precipitation": [0.0, 1.2],
            "uv_index": [],
        },
        "daily": {
            "time": ["2026-09-10"],
            "weather_code": [61],
            "temperature_2m_max": [15.1],
            "temperature_2m_min": [9.2],
            "precipitation_sum": [3.2],
            "precipitation_probability_max": [null],
            "sunrise": ["2026-09-10T06:32"],
            "sunset": [null],
        },
    });
    let wire: WeatherResponse = serde_json::from_value(json).expect("the shape is the API's");
    let report = build_report(&wire, [60.17, 24.94]);

    assert_eq!(report.place, "Helsinki");
    assert_eq!(report.utc_offset_seconds, 10800);
    assert_eq!(report.hours.len(), 2);
    // No apparent temperature published for this hour: it feels like what
    // it is, rather than like absolute zero.
    assert_eq!(report.hours[1].feels, 10.5);
    assert_eq!(report.hours[0].pop, 0);
    assert_eq!(report.hours[1].pop, 70);
    // Asked for and answered with nothing at all.
    assert_eq!(report.hours[0].uv, 0.0);
    // The symbol travels with the hour, so the page never maps a code.
    assert_eq!(report.hours[0].sym, "04");
    assert_eq!(report.hours[1].sym, "46");
    assert_eq!(report.days.len(), 1);
    assert_eq!(report.days[0].feels_high, 15.1, "falls back on the real high");
    assert_eq!(report.days[0].sunrise, "2026-09-10T06:32");
    assert!(report.days[0].sunset.is_empty(), "a sunset that never comes is not a time");
    assert!(report.now.is_none(), "no current block was sent");
}

/// The report carries the place's own clock, not the reader's: the
/// timestamps are passed through as the API wrote them and the offset says
/// what they mean.
#[test]
fn hours_keep_the_places_own_local_time() {
    let json = serde_json::json!({
        "timezone": "Pacific/Auckland",
        "utc_offset_seconds": 43200,
        "hourly": {
            "time": ["2026-09-10T13:00"],
            "temperature_2m": [17.0],
            "weather_code": [0],
            "is_day": [1],
        },
    });
    let wire: WeatherResponse = serde_json::from_value(json).expect("the shape is the API's");
    let report = build_report(&wire, [-36.85, 174.76]);
    assert_eq!(report.hours[0].t, "2026-09-10T13:00");
    assert!(report.hours[0].day);
    assert_eq!(report.hours[0].sym, "01d", "the sun is up in Auckland at one");
    assert_eq!(report.timezone, "Pacific/Auckland");
    assert!(report.days.is_empty(), "no daily block was sent");
}

#[test]
fn nearest_city_finds_the_obvious_one() {
    let helsinki = nearest_city(60.17, 24.94).expect("the list is not empty");
    assert_eq!(helsinki.name, "Helsinki");
}

#[test]
fn nearest_city_crosses_the_antimeridian() {
    // A point in the Pacific just *east* of the 180th meridian. It is a few
    // hundred kilometres from New Zealand and most of the way round the
    // world from it in raw longitude, so comparing coordinates instead of
    // angles answers "Tijuana".
    let city = nearest_city(-36.8, -179.0).expect("the list is not empty");
    assert_eq!(city.name, "Hamilton");
    assert!(city.longitude > 170.0, "the answer came from the other side of the line");
}

#[test]
fn nearest_city_measures_along_the_globe_not_across_the_grid() {
    // At 69°N a degree of longitude is a third of a degree of latitude, so
    // the two ways of measuring genuinely disagree here — which is the
    // whole reason this isn't a subtraction.
    let point = (69.0_f32, 40.0_f32);

    let flat = CITIES
        .iter()
        .min_by(|a, b| {
            let distance = |city: &City| {
                (city.latitude - point.0).powi(2) + (city.longitude - point.1).powi(2)
            };
            distance(a).partial_cmp(&distance(b)).expect("no NaNs in the table")
        })
        .expect("the list is not empty");

    let round = nearest_city(point.0, point.1).expect("the list is not empty");

    assert_eq!(round.name, "Oulu");
    assert_eq!(flat.name, "Kuopio", "the flat answer, kept here to show they differ");
}

// spec: CORE-GEN-003
#[test]
fn build_report_reads_no_clock_and_leaves_fetched_at_to_the_caller() {
    // Today's `build_report` stamped the system clock; the core's writes 0
    // (today's own fallback) and two builds of one answer are equal.
    let json = serde_json::json!({
        "hourly": { "time": ["2026-09-10T13:00"], "temperature_2m": [17.0], "weather_code": [0], "is_day": [1] },
    });
    let wire: WeatherResponse = serde_json::from_value(json).expect("the shape is the API's");
    let one = build_report(&wire, [60.17, 24.94]);
    let two = build_report(&wire, [60.17, 24.94]);
    assert_eq!(one.fetched_at, 0);
    assert_eq!(serde_json::to_string(&one).ok(), serde_json::to_string(&two).ok());
}

#[test]
fn every_wmo_code_has_today_s_symbol() {
    // The match of today's `sky_symbol`, worked through code by code.
    let table: [(i32, &str, &str); 28] = [
        (0, "01d", "01n"), (1, "02d", "02n"), (2, "03d", "03n"), (3, "04", "04"),
        (45, "15", "15"), (48, "15", "15"),
        (51, "46", "46"), (53, "46", "46"), (55, "09", "09"), (56, "47", "47"), (57, "47", "47"),
        (61, "46", "46"), (63, "09", "09"), (65, "10", "10"), (66, "47", "47"), (67, "48", "48"),
        (71, "49", "49"), (77, "49", "49"), (73, "13", "13"), (75, "50", "50"),
        (80, "40d", "40n"), (81, "05d", "05n"), (82, "41d", "41n"),
        (85, "44d", "44n"), (86, "45d", "45n"),
        (95, "22", "22"), (96, "11", "11"), (99, "11", "11"),
    ];
    for (code, day, night) in table {
        assert_eq!(sky_symbol(code, true), day, "WMO {code} by day");
        assert_eq!(sky_symbol(code, false), night, "WMO {code} by night");
    }
    for code in [-1, 4, 44, 100, 1000] {
        assert_eq!(sky_symbol(code, true), "04", "WMO {code} falls back");
    }
}
