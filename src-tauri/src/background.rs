//! Terminal background images. Picked files are copied into `<config>/backgrounds/` under a content-hash
//! name ("<16 hex>.<ext>"): the synced settings can then name the image, and the file itself travels with
//! the sync (encrypted, record `file:<name>`). A path typed by hand still works, on this machine only.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Above this, images are re-encoded (JPEG, 4K at most): lighter to draw, to store in sync.json and to sync.
const MAX_BYTES: usize = 2 << 20;
const MAX_SIDE: u32 = 3840;
/// Largest file accepted at all (it is shrunk before being kept).
const MAX_INPUT: u64 = 40 << 20;

fn mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif", // shown as is, but cannot be shrunk (no AVIF decoder)
        _ => return None,
    })
}

fn ext_of(path: &str) -> Result<String> {
    let ext = std::path::Path::new(path).extension().and_then(|x| x.to_str()).unwrap_or_default().to_lowercase();
    if mime(&ext).is_none() {
        bail!("format d'image non pris en charge : .{ext}");
    }
    Ok(ext)
}

pub fn dir() -> PathBuf {
    crate::config::dir().join("backgrounds")
}

/// What the settings hold for an imported image; anything else is a path on this machine.
pub fn is_name(s: &str) -> bool {
    let Some((hash, ext)) = s.split_once('.') else { return false };
    hash.len() == 16 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) && mime(ext).is_some()
}

pub fn name_for(bytes: &[u8], ext: &str) -> String {
    let hash = Sha256::digest(bytes);
    format!("{}.{ext}", hash[..8].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Copies (and shrinks if needed) an image file into the backgrounds folder; returns its name.
pub fn import(path: &str) -> Result<String> {
    let ext = ext_of(path)?;
    if std::fs::metadata(path).with_context(|| format!("image introuvable : {path}"))?.len() > MAX_INPUT {
        bail!("image trop lourde (40 Mo maximum)");
    }
    let (bytes, ext) = shrink(std::fs::read(path)?, ext, MAX_BYTES, MAX_SIDE)?;
    let name = name_for(&bytes, &ext);
    save(&name, &bytes)?;
    Ok(name)
}

/// Writes an image under its name and drops the previous ones (one background at a time).
pub fn save(name: &str, bytes: &[u8]) -> Result<()> {
    if !is_name(name) {
        bail!("nom d'image invalide : {name}");
    }
    let d = dir();
    std::fs::create_dir_all(&d)?;
    crate::config::write_atomic(&d.join(name), bytes)?;
    for f in std::fs::read_dir(&d)?.flatten() {
        if f.file_name() != name {
            let _ = std::fs::remove_file(f.path());
        }
    }
    Ok(())
}

/// The bytes of an imported image, if this device has it.
pub fn read(name: &str) -> Option<Vec<u8>> {
    is_name(name).then(|| std::fs::read(dir().join(name)).ok()).flatten()
}

/// The image as a data: URL (the CSP only allows 'self' and data: images).
pub fn data_url(name_or_path: &str) -> Result<String> {
    use base64::Engine;
    let path = if is_name(name_or_path) { dir().join(name_or_path) } else { PathBuf::from(name_or_path) };
    let ext = ext_of(name_or_path)?;
    let len = match std::fs::metadata(&path) {
        Ok(m) => m.len(),
        Err(_) if is_name(name_or_path) => bail!("image de fond pas encore reçue par la synchro (coffre verrouillé ?)"),
        Err(_) => bail!("image de fond introuvable : {name_or_path}"),
    };
    if len > MAX_INPUT {
        bail!("image trop lourde (40 Mo maximum)");
    }
    let bytes = std::fs::read(&path)?;
    Ok(format!("data:{};base64,{}", mime(&ext).unwrap_or("image/png"), base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Re-encodes big images as JPEG, smaller and smaller until they fit `max` (transparency is lost).
fn shrink(bytes: Vec<u8>, ext: String, max: usize, side: u32) -> Result<(Vec<u8>, String)> {
    if bytes.len() <= max {
        return Ok((bytes, ext));
    }
    let img = image::load_from_memory(&bytes).context("image trop lourde, et impossible à réduire dans ce format")?;
    let mut out = Vec::new();
    for (side, quality) in [(side, 85), (side, 72), (side * 3 / 4, 72), (side / 2, 72)] {
        let small = if img.width().max(img.height()) > side { img.resize(side, side, image::imageops::FilterType::Triangle) } else { img.clone() };
        out.clear();
        image::DynamicImage::ImageRgb8(small.to_rgb8())
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality))?;
        if out.len() <= max {
            break;
        }
    }
    Ok((out, "jpg".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_content_hashes_and_nothing_else() {
        let n = name_for(b"img", "png");
        assert!(is_name(&n), "{n}");
        assert_eq!(n, name_for(b"img", "png"));
        for bad in ["../../.ssh/id_ed25519", "0123456789abcdef.exe", "0123456789ABCDEF.png", "/tmp/a.png", "0123.png"] {
            assert!(!is_name(bad), "{bad}");
        }
    }

    #[test]
    fn big_images_are_shrunk_to_jpeg() {
        // Noise: the worst case for compression.
        let mut seed = 1u32;
        let img = image::RgbImage::from_fn(900, 600, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            image::Rgb([(seed >> 8) as u8, (seed >> 16) as u8, (seed >> 24) as u8])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (out, ext) = shrink(png.clone(), "png".into(), 200_000, 800).unwrap();
        assert_eq!(ext, "jpg");
        assert!(out.len() < png.len());
        let back = image::load_from_memory(&out).unwrap();
        assert!(back.width() <= 800 && back.height() <= 800);
        // Small enough already: kept untouched.
        assert_eq!(shrink(b"tiny".to_vec(), "png".into(), 200_000, 800).unwrap(), (b"tiny".to_vec(), "png".into()));
    }
}
