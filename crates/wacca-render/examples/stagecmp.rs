//! Side-by-side check of `WACCA_SHOT` renders against RenderDoc captures of the real game
//! (doc 11 §2; `tools/research/rdc_rt_at.py` saves the back buffer after the main view's tonemap).
//!
//! `stagecmp planes <rdc dump tex dir> <out dir>`: the captured movie frame (R8 planes saved as
//! PNG: Y `176841`, U `176843`, V `176845`) as grey `y.png`, `u.png`, `v.png` for
//! `WACCA_SHOT_MOVIE`.
//! `stagecmp compare <real.png> <ours.png> <out.png>`: crops the real 675×1200 window to its view
//! square (0, 228, 675, 675), writes real | ours | |difference|×4 and prints the mean absolute
//! difference per radius band (px from the centre) and its pixel count.

use std::error::Error;
use std::path::Path;

use image::{GenericImage, GenericImageView, Rgba, RgbaImage};

const VIEW: (u32, u32, u32) = (0, 228, 675);

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["planes", tex, out] => planes(Path::new(tex), Path::new(out)),
        ["compare", real, ours, out] => compare(real, ours, out),
        ["profile", real, ours, deg, r0, r1] => {
            profile(real, ours, deg.parse()?, r0.parse()?, r1.parse()?)
        }
        _ => Err(
            "usage: stagecmp planes <tex dir> <out dir> | compare <real> <ours> <out> | \
                  profile <real> <ours> <deg> <r0> <r1>"
                .into(),
        ),
    }
}

/// Prints real and our RGB along the ray at `deg` (counter-clockwise from 3 o'clock) from
/// radius `r0` to `r1` px.
fn profile(real: &str, ours: &str, deg: f32, r0: u32, r1: u32) -> Result<(), Box<dyn Error>> {
    let (x0, y0, side) = VIEW;
    let real = image::open(real)?.to_rgba8();
    let ours = image::open(ours)?.to_rgba8();
    let c = side as f32 / 2.0;
    let (sin, cos) = deg.to_radians().sin_cos();
    for r in r0..=r1 {
        let (x, y) = ((c + r as f32 * cos) as u32, (c - r as f32 * sin) as u32);
        let a = real.get_pixel(x0 + x, y0 + y);
        let b = ours.get_pixel(x, y);
        println!(
            "r {r:>3}  real {:>3} {:>3} {:>3}  ours {:>3} {:>3} {:>3}",
            a[0], a[1], a[2], b[0], b[1], b[2]
        );
    }
    Ok(())
}

fn planes(tex: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(out)?;
    for (id, name) in [(176841, "y"), (176843, "u"), (176845, "v")] {
        let img = image::open(tex.join(format!("{id}.png")))?.to_rgba8();
        let grey = image::GrayImage::from_fn(img.width(), img.height(), |x, y| {
            image::Luma([img.get_pixel(x, y)[0]])
        });
        grey.save(out.join(format!("{name}.png")))?;
    }
    Ok(())
}

fn compare(real: &str, ours: &str, out: &str) -> Result<(), Box<dyn Error>> {
    let (x0, y0, side) = VIEW;
    let real = image::open(real)?.to_rgba8();
    let real = real.view(x0, y0, side, side).to_image();
    let ours = image::open(ours)?.to_rgba8();
    if ours.dimensions() != (side, side) {
        return Err(format!("ours must be {side}x{side} (WACCA_SHOT_SIZE={side})").into());
    }
    let mut sheet = RgbaImage::new(3 * side, side);
    sheet.copy_from(&real, 0, 0)?;
    sheet.copy_from(&ours, side, 0)?;
    const BAND: f32 = 25.0;
    let mut bands = vec![(0.0f64, 0u32); (side as f32 / BAND) as usize + 1];
    let c = side as f32 / 2.0;
    for y in 0..side {
        for x in 0..side {
            let (a, b) = (real.get_pixel(x, y), ours.get_pixel(x, y));
            let d: [u8; 3] = std::array::from_fn(|i| a[i].abs_diff(b[i]));
            let scaled = d.map(|v| v.saturating_mul(4));
            sheet.put_pixel(
                2 * side + x,
                y,
                Rgba([scaled[0], scaled[1], scaled[2], 255]),
            );
            let r = (x as f32 + 0.5 - c).hypot(y as f32 + 0.5 - c);
            let band = &mut bands[(r / BAND) as usize];
            band.0 += f64::from(d.iter().map(|&v| u32::from(v)).sum::<u32>()) / 3.0;
            band.1 += 1;
        }
    }
    sheet.save(out)?;
    for (i, (sum, n)) in bands.iter().enumerate().filter(|(_, b)| b.1 > 0) {
        let r0 = i as f32 * BAND;
        println!(
            "r {r0:>5.0}-{:<5.0} mean |diff| {:6.2} px {n}",
            r0 + BAND,
            sum / f64::from(*n)
        );
    }
    Ok(())
}
