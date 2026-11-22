//! `taskdeck-device`: the device runtime. The sealed store, the transport, the
//! key stores, the fetchers, blobs, backup scheduling, the facade, and the
//! adapters that implement `taskdeck_sync::StateMachine` over `taskdeck-core`
//! and `taskdeck_sync::Sealer` over `taskdeck-seal` (decision D6).
//!
//! Spec: `docs/sync/spec/device.md`. Owner: stage `device` (SYNC_PLAN.md §4).
//! Created as a shell by stage `foundation`; the skeleton types come from the
//! spec's Public API (§25).
//!
//! `unsafe` is denied: the device stage allows it in `platform::windows` alone
//! (DEV-GEN-001). There is none yet.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use base64 as _;
use flate2 as _;
use serde_json as _;
use subtle as _;
use taskdeck_backup as _;
use taskdeck_core as _;
use taskdeck_seal as _;
use taskdeck_sync as _;
use url as _;
use zeroize as _;
const _: () = {
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared(chrono::DateTime<chrono::FixedOffset>);
    // `clock`: the device reads the clock and the local zone.
    let _ = chrono::Local::now;
    let _ = <sha2::Sha256 as sha2::Digest>::new;
    // `jpeg` and `png`.
    let _ = image::ImageFormat::Jpeg;
    let _ = image::codecs::jpeg::JpegEncoder::<Vec<u8>>::new_with_quality;
    let _ = image::codecs::png::PngDecoder::<std::io::Cursor<Vec<u8>>>::new;
    // The ring provider, and TLS 1.2 next to 1.3.
    let _ = rustls::crypto::ring::default_provider;
    let _ = &rustls::version::TLS12;
    let _ = rustls_platform_verifier::Verifier::new;
    // ureq over rustls with the platform verifier.
    let _ = ureq::tls::RootCerts::PlatformVerifier;
    let _ = ureq::tls::TlsProvider::Rustls;
    // The client handshake over the device's own rustls stream.
    let _ = tungstenite::client::client::<
        rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>,
        tungstenite::handshake::client::Request,
    >;
};
#[cfg(windows)]
const _: () = {
    let _ = windows::Win32::Security::Cryptography::CryptProtectData::<windows::core::PCWSTR>;
    let _ = windows::Win32::Security::Cryptography::CryptUnprotectData;
    let _ = windows::Win32::Foundation::LocalFree;
    let _ = windows::Win32::Storage::FileSystem::MoveFileExW::<windows::core::PCWSTR, windows::core::PCWSTR>;
};
#[cfg(target_os = "macos")]
const _: () = {
    let _ = security_framework::passwords::get_generic_password;
};

#[cfg(test)]
mod tests {
    use getrandom as _;
    use tempfile as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-device");
    }
}
