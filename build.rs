use embed_resource;
use chrono::Utc;

fn main() {
    // Compile resources
    embed_resource::compile("resources.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();

    // Use chrono instead of external command for cross-platform safety
    let date_string = Utc::now().format("%Y-%m-%d").to_string();
    println!("cargo:rustc-env=BUILD_DATE={}", date_string);
}