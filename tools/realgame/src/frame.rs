//! Decoded video frames (FFmpeg `rawvideo` output) and the pixel access the screen checks and the input-test
//! analysis need.

/// Packed RGB24.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Rgb {
    /// `None` unless `data` holds exactly `width * height` pixels.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Option<Self> {
        (data.len() == width * height * 3).then_some(Self {
            width,
            height,
            data,
        })
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
}

/// 8-bit luma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Gray {
    /// `None` unless `data` holds exactly `width * height` pixels.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Option<Self> {
        (data.len() == width * height).then_some(Self {
            width,
            height,
            data,
        })
    }

    /// Bilinear sample at pixel-centre coordinates; 0 outside the frame.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f64, y: f64| -> f64 {
            if x < 0.0 || y < 0.0 || x >= self.width as f64 || y >= self.height as f64 {
                return 0.0;
            }
            f64::from(self.data[y as usize * self.width + x as usize])
        };
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_checked() {
        assert!(Rgb::new(2, 1, vec![0; 6]).is_some());
        assert!(Rgb::new(2, 1, vec![0; 5]).is_none());
        assert!(Gray::new(2, 2, vec![0; 4]).is_some());
        assert!(Gray::new(2, 2, vec![0; 3]).is_none());
    }

    #[test]
    fn rgb_pixel() {
        let f = Rgb::new(2, 1, vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(f.pixel(1, 0), [4, 5, 6]);
    }

    #[test]
    fn bilinear_sampling() {
        let g = Gray::new(2, 2, vec![0, 100, 100, 200]).unwrap();
        assert_eq!(g.sample(0.0, 0.0), 0.0);
        assert_eq!(g.sample(1.0, 1.0), 200.0);
        assert_eq!(g.sample(0.5, 0.0), 50.0);
        assert_eq!(g.sample(0.5, 0.5), 100.0);
        assert_eq!(g.sample(-1.0, 0.0), 0.0);
        assert_eq!(g.sample(5.0, 5.0), 0.0);
    }
}
