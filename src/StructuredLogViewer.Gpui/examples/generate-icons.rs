//! Turns `assets/icon.svg` into every platform's icon container.
//!
//! ```sh
//! cargo run --example generate-icons
//! ```
//!
//! There is no cross-platform icon format — each platform wants its own
//! container — so the portable thing is the *source*: one SVG, and this
//! program to fan it out. It runs anywhere; nothing here shells out to
//! `iconutil`, `magick` or `png2ico`. The rasteriser is resvg, which gpui
//! already depends on for its `svg()` element, so this costs no new crates.
//!
//! | Output                       | Used by                                  |
//! |------------------------------|------------------------------------------|
//! | `assets/AppIcon.icns`        | the macOS bundle (`scripts/bundle-mac.sh`) |
//! | `assets/icon.ico`            | a Windows executable resource            |
//! | `assets/icons/icon-<N>.png`  | Linux `hicolor`, and anything else       |
//! | `web/favicon.png`, `web/apple-touch-icon.png` | the browser head       |

use std::io::Write;
use std::path::{Path, PathBuf};
use tiny_skia::{Pixmap, Transform};

/// macOS draws app icons on an 824pt tile inside a 1024pt canvas, so the
/// grid alignment matches every other icon in the Dock. Everywhere else the
/// icon is edge to edge.
const MACOS_TILE: f32 = 824. / 1024.;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let svg = std::fs::read(root.join("assets/icon.svg")).expect("assets/icon.svg");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("parse icon.svg");

    let full = |size: u32| render(&tree, size, 1.0);
    let inset = |size: u32| render(&tree, size, MACOS_TILE);

    // --- macOS: .icns ------------------------------------------------
    // Type codes are (family, pixel size); the `ic**` ones carry PNG.
    let icns_slots: [(&[u8; 4], u32); 10] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"ic11", 32),  // 16@2x
        (b"ic12", 64),  // 32@2x
        (b"ic07", 128),
        (b"ic13", 256), // 128@2x
        (b"ic08", 256),
        (b"ic14", 512), // 256@2x
        (b"ic09", 512),
        (b"ic10", 1024), // 512@2x
    ];
    let icns: Vec<(&[u8; 4], Vec<u8>)> =
        icns_slots.iter().map(|(kind, size)| (*kind, png(&inset(*size)))).collect();
    write(&root.join("assets/AppIcon.icns"), &icns_container(&icns));

    // --- Windows: .ico -----------------------------------------------
    // PNG-in-ICO, which every Windows since Vista reads.
    let ico_sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let ico: Vec<(u32, Vec<u8>)> = ico_sizes.iter().map(|s| (*s, png(&full(*s)))).collect();
    write(&root.join("assets/icon.ico"), &ico_container(&ico));

    // --- Linux hicolor, and a general-purpose set ---------------------
    std::fs::create_dir_all(root.join("assets/icons")).expect("assets/icons");
    for size in [16u32, 32, 48, 64, 128, 256, 512] {
        write(&root.join(format!("assets/icons/icon-{size}.png")), &png(&full(size)));
    }

    // --- Web ----------------------------------------------------------
    write(&root.join("web/favicon.png"), &png(&full(32)));
    write(&root.join("web/apple-touch-icon.png"), &png(&full(180)));

    println!("wrote AppIcon.icns, icon.ico, 7 PNGs and the web icons");
}

/// Rasterises the tree into a `size`-square pixmap, occupying `fraction` of
/// it and centred.
fn render(tree: &usvg::Tree, size: u32, fraction: f32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 * fraction / tree.size().width();
    let offset = size as f32 * (1. - fraction) / 2.;
    resvg::render(
        tree,
        Transform::from_translate(offset, offset).pre_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

fn png(pixmap: &Pixmap) -> Vec<u8> {
    pixmap.encode_png().expect("encode png")
}

fn write(path: &Path, bytes: &[u8]) {
    let mut file = std::fs::File::create(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    file.write_all(bytes).expect("write");
    println!("  {:>9} bytes  {}", bytes.len(), path.file_name().unwrap().to_string_lossy());
}

/// `icns`: a magic, a total length, then `[type][length][payload]` chunks,
/// all big-endian, with the length counting its own 8-byte header.
fn icns_container(entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body: usize = entries.iter().map(|(_, data)| 8 + data.len()).sum();
    let mut out = Vec::with_capacity(8 + body);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&((8 + body) as u32).to_be_bytes());
    for (kind, data) in entries {
        out.extend_from_slice(*kind);
        out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// `ico`: a 6-byte directory header, one 16-byte entry per image, then the
/// payloads. Little-endian, and 256 is spelled `0` in the size bytes.
fn ico_container(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut header = Vec::with_capacity(6 + entries.len() * 16);
    header.extend_from_slice(&0u16.to_le_bytes()); // reserved
    header.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon
    header.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    let mut offset = 6 + entries.len() * 16;
    let mut body = Vec::new();
    for (size, data) in entries {
        let dimension = if *size >= 256 { 0u8 } else { *size as u8 };
        header.push(dimension);
        header.push(dimension);
        header.push(0); // palette size
        header.push(0); // reserved
        header.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        header.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        header.extend_from_slice(&(data.len() as u32).to_le_bytes());
        header.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    header.extend_from_slice(&body);
    header
}
