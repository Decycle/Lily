//! CPU rasteriser for our playfield ([`wacca_render::build_scene`] triangles), so comparison frames need no GPU and
//! can be placed on the real game's measured geometry: playfield point `(x, y)` (rim radius 1, y up) goes to pixel
//! `(cx + x s, cy - y s)`. Colours are blended "over" in sRGB like the renderer's non-sRGB target.

use crate::frame::Rgb;
use wacca_render::Vertex;

/// Draws `tris` (a triangle list) onto `img`, pixel centres sampled, alpha-blended in order.
pub fn draw(img: &mut Rgb, tris: &[Vertex], cx: f64, cy: f64, s: f64) {
    let px = |v: &Vertex| (cx + f64::from(v.pos[0]) * s, cy - f64::from(v.pos[1]) * s);
    for t in tris.as_chunks::<3>().0 {
        let p = [px(&t[0]), px(&t[1]), px(&t[2])];
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-12 {
            continue;
        }
        let x0 = p
            .iter()
            .map(|q| q.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let y0 = p
            .iter()
            .map(|q| q.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (p
            .iter()
            .map(|q| q.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .max(0.0) as usize)
            .min(img.width);
        let y1 = (p
            .iter()
            .map(|q| q.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .max(0.0) as usize)
            .min(img.height);
        let c = t[0].color;
        let a = c[3].clamp(0.0, 1.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let q = (x as f64 + 0.5, y as f64 + 0.5);
                let w = [
                    edge(p[1], p[2], q),
                    edge(p[2], p[0], q),
                    edge(p[0], p[1], q),
                ];
                let inside = w.iter().all(|&e| e * area >= 0.0);
                if !inside {
                    continue;
                }
                let i = (y * img.width + x) * 3;
                for (dst, src) in img.data[i..i + 3].iter_mut().zip(c) {
                    let v = src.clamp(0.0, 1.0) * a + f32::from(*dst) / 255.0 * (1.0 - a);
                    *dst = (v * 255.0).round() as u8;
                }
            }
        }
    }
}

fn edge(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
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

    fn v(x: f32, y: f32, color: [f32; 4]) -> Vertex {
        Vertex {
            pos: [x, y],
            color,
            ..Vertex::default()
        }
    }

    #[test]
    fn fills_and_blends_triangles_in_playfield_space() {
        let mut img = Rgb::new(10, 10, vec![0; 300]).unwrap();
        let red = [1.0, 0.0, 0.0, 1.0];
        // Quad covering x in [-1, 0], y in [0, 1] (top-left quarter after the y flip).
        let quad = [
            v(-1.0, 0.0, red),
            v(0.0, 0.0, red),
            v(0.0, 1.0, red),
            v(-1.0, 0.0, red),
            v(0.0, 1.0, red),
            v(-1.0, 1.0, red),
        ];
        draw(&mut img, &quad, 5.0, 5.0, 5.0);
        assert_eq!(img.pixel(2, 2), [255, 0, 0]);
        assert_eq!(img.pixel(7, 2), [0, 0, 0]);
        assert_eq!(img.pixel(2, 7), [0, 0, 0]);
        let half_blue = [0.0, 0.0, 1.0, 0.5];
        draw(
            &mut img,
            &[
                v(-1.0, 0.0, half_blue),
                v(0.0, 1.0, half_blue),
                v(0.0, 0.0, half_blue),
            ],
            5.0,
            5.0,
            5.0,
        );
        assert_eq!(img.pixel(4, 4), [128, 0, 128]);
        let sbs = side_by_side(&img, &img).unwrap();
        assert_eq!((sbs.width, sbs.pixel(10, 0)), (20, [255, 0, 0]));
        assert!(side_by_side(&img, &Rgb::new(1, 1, vec![0; 3]).unwrap()).is_none());
        let bytes = png(&img).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(Rgb::from_png(&bytes), Some(img));
        assert_eq!(Rgb::from_png(b"not a png"), None);
    }
}
