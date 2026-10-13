//! Image helpers for comparison frames: placing a square playfield render of ours ([`crate::gpu`]) on the real
//! game's measured geometry, side-by-side composition and PNG encoding.

use crate::frame::Rgb;

/// Resamples `square` (our renderer's square viewport, where playfield radius `view_extent` reaches the edge) into a
/// `width x height` canvas so that the playfield centre lands on `(cx, cy)` and radius 1 (the judgement line) on
/// `rim` pixels. Bilinear; outside the square is black.
pub fn place(
    square: &Rgb,
    view_extent: f64,
    (width, height): (usize, usize),
    (cx, cy): (f64, f64),
    rim: f64,
) -> Rgb {
    let side = square.width as f64;
    let mut data = vec![0u8; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            // Playfield coordinates (y up) of the canvas pixel centre, then square texel coordinates.
            let px = (x as f64 + 0.5 - cx) / rim;
            let py = (cy - y as f64 - 0.5) / rim;
            let u = (px / view_extent + 1.0) / 2.0 * side - 0.5;
            let v = (1.0 - py / view_extent) / 2.0 * side - 0.5;
            if let Some(p) = bilinear(square, u, v) {
                data[(y * width + x) * 3..][..3].copy_from_slice(&p);
            }
        }
    }
    Rgb {
        width,
        height,
        data,
    }
}

fn bilinear(img: &Rgb, u: f64, v: f64) -> Option<[u8; 3]> {
    let (w, h) = (img.width as f64, img.height as f64);
    if u < -0.5 || v < -0.5 || u > w - 0.5 || v > h - 0.5 {
        return None;
    }
    let (u, v) = (u.clamp(0.0, w - 1.0), v.clamp(0.0, h - 1.0));
    let (x0, y0) = (u.floor() as usize, v.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.width - 1), (y0 + 1).min(img.height - 1));
    let (fx, fy) = (u - x0 as f64, v - y0 as f64);
    let (a, b, c, d) = (
        img.pixel(x0, y0),
        img.pixel(x1, y0),
        img.pixel(x0, y1),
        img.pixel(x1, y1),
    );
    let mut out = [0u8; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let top = f64::from(a[i]) * (1.0 - fx) + f64::from(b[i]) * fx;
        let bottom = f64::from(c[i]) * (1.0 - fx) + f64::from(d[i]) * fx;
        *o = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    Some(out)
}

/// Two frames of the same height next to each other (`left | right`).
pub fn side_by_side(left: &Rgb, right: &Rgb) -> Option<Rgb> {
    if left.height != right.height {
        return None;
    }
    let w = left.width + right.width;
    let mut data = Vec::with_capacity(w * left.height * 3);
    for y in 0..left.height {
        data.extend_from_slice(&left.data[y * left.width * 3..(y + 1) * left.width * 3]);
        data.extend_from_slice(&right.data[y * right.width * 3..(y + 1) * right.width * 3]);
    }
    Rgb::new(w, left.height, data)
}

/// RGB8 PNG bytes.
pub fn png(img: &Rgb) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width as u32, img.height as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(&img.data)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_the_square_on_the_measured_geometry() {
        // 4x4 square: left half red, right half blue; view extent 2 (radius 1 = a quarter of the side).
        let sq = Rgb::new(
            4,
            4,
            (0..16)
                .flat_map(|i| if i % 4 < 2 { [200, 0, 0] } else { [0, 0, 200] })
                .collect(),
        )
        .unwrap();
        // Radius 1 -> 10 px, centre (20, 20): the square spans 0..40.
        let img = place(&sq, 2.0, (50, 40), (20.0, 20.0), 10.0);
        assert_eq!(img.pixel(5, 5), [200, 0, 0]);
        assert_eq!(img.pixel(35, 30), [0, 0, 200]);
        assert_eq!(img.pixel(45, 20), [0, 0, 0]);
        assert_eq!(img.pixel(20, 20)[1], 0);
        let sbs = side_by_side(&img, &img).unwrap();
        assert_eq!((sbs.width, sbs.pixel(55, 5)), (100, [200, 0, 0]));
        assert!(side_by_side(&img, &Rgb::new(1, 1, vec![0; 3]).unwrap()).is_none());
        let bytes = png(&img).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(Rgb::from_png(&bytes), Some(img));
        assert_eq!(Rgb::from_png(b"not a png"), None);
    }
}
