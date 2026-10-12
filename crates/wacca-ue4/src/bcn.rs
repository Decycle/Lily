//! BC1 (DXT1) and BC3 (DXT5) block decoding to RGBA8.

/// 16 RGBA pixels of a 4×4 block, row-major.
pub(crate) type Block = [[u8; 4]; 16];

fn rgb565(c: u16) -> [u16; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

fn mix(a: [u16; 3], b: [u16; 3], wa: u16, wb: u16) -> [u8; 4] {
    let c = |i: usize| ((a[i] * wa + b[i] * wb) / (wa + wb)) as u8;
    [c(0), c(1), c(2), 255]
}

/// One 8-byte BC1 colour block. `four_color` forces the 4-colour mode, as BC3 colour blocks always use it.
pub(crate) fn bc1(block: &[u8; 8], four_color: bool) -> Block {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let palette = if c0 > c1 || four_color {
        [
            mix(a, b, 1, 0),
            mix(a, b, 0, 1),
            mix(a, b, 2, 1),
            mix(a, b, 1, 2),
        ]
    } else {
        [mix(a, b, 1, 0), mix(a, b, 0, 1), mix(a, b, 1, 1), [0; 4]]
    };
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    std::array::from_fn(|i| palette[((indices >> (2 * i)) & 3) as usize])
}

/// One 16-byte BC3 block: 8 bytes interpolated alpha, then a BC1 colour block.
pub(crate) fn bc3(block: &[u8; 16]) -> Block {
    let (a0, a1) = (u32::from(block[0]), u32::from(block[1]));
    let alpha = |code: u64| -> u8 {
        let code = code as u32;
        let v = match code {
            0 => a0,
            1 => a1,
            _ if a0 > a1 => ((8 - code) * a0 + (code - 1) * a1) / 7,
            6 => 0,
            7 => 255,
            _ => ((6 - code) * a0 + (code - 1) * a1) / 5,
        };
        v as u8
    };
    let mut bits = [0; 8];
    bits[..6].copy_from_slice(&block[2..8]);
    let bits = u64::from_le_bytes(bits);
    let mut colour = [0; 8];
    colour.copy_from_slice(&block[8..]);
    let mut out = bc1(&colour, true);
    for (i, px) in out.iter_mut().enumerate() {
        px[3] = alpha((bits >> (3 * i)) & 7);
    }
    out
}

/// Decodes a block-compressed image (`N`-byte blocks, row-major) to tightly packed RGBA8.
/// The caller guarantees `data` holds at least `ceil(w/4) * ceil(h/4)` blocks.
pub(crate) fn decode<const N: usize>(
    data: &[u8],
    width: usize,
    height: usize,
    block: impl Fn(&[u8; N]) -> Block,
) -> Vec<u8> {
    let mut out = vec![0; width * height * 4];
    let blocks_x = width.div_ceil(4);
    for (i, bytes) in data.as_chunks::<N>().0.iter().enumerate() {
        let (bx, by) = (i % blocks_x.max(1) * 4, i / blocks_x.max(1) * 4);
        if by >= height {
            break;
        }
        for (p, px) in block(bytes).iter().enumerate() {
            let (x, y) = (bx + p % 4, by + p / 4);
            if x < width && y < height {
                let o = (y * width + x) * 4;
                out[o..o + 4].copy_from_slice(px);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u16 = 0xF800;
    const BLUE: u16 = 0x001F;

    fn bc1_block(c0: u16, c1: u16, indices: u32) -> [u8; 8] {
        let mut b = [0; 8];
        b[..2].copy_from_slice(&c0.to_le_bytes());
        b[2..4].copy_from_slice(&c1.to_le_bytes());
        b[4..].copy_from_slice(&indices.to_le_bytes());
        b
    }

    #[test]
    fn bc1_four_colour_mode_interpolates_thirds() {
        // Pixels 0..3 use indices 0,1,2,3; the rest index 0.
        let out = bc1(&bc1_block(RED, BLUE, 0b11_10_01_00), false);
        assert_eq!(out[0], [255, 0, 0, 255]);
        assert_eq!(out[1], [0, 0, 255, 255]);
        assert_eq!(out[2], [170, 0, 85, 255]);
        assert_eq!(out[3], [85, 0, 170, 255]);
        assert_eq!(out[15], [255, 0, 0, 255]);
    }

    #[test]
    fn bc1_three_colour_mode_has_transparent_black() {
        let out = bc1(&bc1_block(BLUE, RED, 0b11_10_00), false);
        assert_eq!(out[1], [127, 0, 127, 255]);
        assert_eq!(out[2], [0, 0, 0, 0]);
        // BC3 colour blocks ignore the ordering and stay in 4-colour mode.
        let forced = bc1(&bc1_block(BLUE, RED, 0b11_10_00), true);
        assert_eq!(forced[2], [170, 0, 85, 255]);
    }

    #[test]
    fn bc3_decodes_eight_and_six_level_alpha() {
        let mut block = [0; 16];
        block[0] = 255;
        block[1] = 0;
        // Alpha codes per pixel: 0, 1, 2, 7 (3 bits each).
        let codes: u64 = 1 << 3 | 2 << 6 | 7 << 9;
        block[2..8].copy_from_slice(&codes.to_le_bytes()[..6]);
        block[8..].copy_from_slice(&bc1_block(RED, BLUE, 0));
        let out = bc3(&block);
        let alpha: Vec<u8> = out[..4].iter().map(|p| p[3]).collect();
        assert_eq!(alpha, [255, 0, 218, 36]);
        assert_eq!(out[0][..3], [255, 0, 0]);

        block[0] = 0;
        block[1] = 255; // a0 <= a1: 6-level mode with explicit 0 and 255
        let codes: u64 = 2 | 6 << 3 | 7 << 6;
        block[2..8].copy_from_slice(&codes.to_le_bytes()[..6]);
        let alpha: Vec<u8> = bc3(&block)[..3].iter().map(|p| p[3]).collect();
        assert_eq!(alpha, [51, 0, 255]);
    }

    #[test]
    fn decode_places_blocks_and_crops_partial_blocks() {
        // 6x2 image: two blocks, the second cropped to 2x2.
        let mut data = bc1_block(RED, RED, 0).to_vec();
        data.extend(bc1_block(BLUE, BLUE, 0));
        let out = decode::<8>(&data, 6, 2, |b| bc1(b, false));
        assert_eq!(out.len(), 6 * 2 * 4);
        assert_eq!(out[..4], [255, 0, 0, 255]);
        assert_eq!(out[4 * 4..4 * 5], [0, 0, 255, 255]);
        assert_eq!(out[(6 + 5) * 4..], [0, 0, 255, 255]);
    }
}
