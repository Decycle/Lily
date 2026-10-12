//! Cooked `Texture2D` export → top mip as RGBA8. Spec: docs/research/06-visuals.md §4,
//! 03-audio-and-jackets.md §4; reference `tools/research/ue4_texture.py`.
//!
//! Export layout (UE 4.19, cooked): tagged properties, `i32` object-GUID flag, two `FStripDataFlags`
//! (UTexture, UTexture2D), `i32 bCooked`, then per pixel format `FName PixelFormat` + `i32 SkipOffset` +
//! `FTexturePlatformData`, terminated by `None`. Only the first platform data is read.

use crate::Ue4Error;
use crate::bcn;
use crate::package::Package;
use crate::property::read_properties;
use crate::reader::Reader;

/// Bulk data flags (`EBulkDataFlags`).
const BULK_AT_END_OF_FILE: u32 = 0x1;
const BULK_COMPRESSED_ZLIB: u32 = 0x2;
const BULK_UNUSED: u32 = 0x20;
const BULK_SEPARATE_FILE: u32 = 0x100;
const BULK_SIZE_64BIT: u32 = 0x2000;
/// `FTexturePlatformData::PackedData` bit: an `FOptTexturePlatformData` (2 × u32) follows the format.
const HAS_OPT_DATA: u32 = 1 << 30;
/// Trailing package tag at the end of every `.uexp`; `.ubulk` offsets are relative to the combined
/// `.uasset` + `.uexp` size without it (Summary.BulkDataStartOffset).
const PACKAGE_TAG_SIZE: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PixelFormat {
    B8G8R8A8,
    G8,
    Dxt1,
    Dxt5,
}

impl PixelFormat {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "PF_B8G8R8A8" => Self::B8G8R8A8,
            "PF_G8" => Self::G8,
            "PF_DXT1" => Self::Dxt1,
            "PF_DXT5" => Self::Dxt5,
            _ => return None,
        })
    }

    /// UE4 name, e.g. `PF_DXT1`.
    pub fn name(self) -> &'static str {
        match self {
            Self::B8G8R8A8 => "PF_B8G8R8A8",
            Self::G8 => "PF_G8",
            Self::Dxt1 => "PF_DXT1",
            Self::Dxt5 => "PF_DXT5",
        }
    }

    /// Bytes of a `width` × `height` mip (saturating, so absurd sizes fail the length check).
    fn data_size(self, width: usize, height: usize) -> usize {
        let blocks = || width.div_ceil(4).saturating_mul(height.div_ceil(4));
        match self {
            Self::B8G8R8A8 => width.saturating_mul(height).saturating_mul(4),
            Self::G8 => width.saturating_mul(height),
            Self::Dxt1 => blocks().saturating_mul(8),
            Self::Dxt5 => blocks().saturating_mul(16),
        }
    }
}

/// The top available mip of a texture, decoded to RGBA8 (rows top-down, no padding).
#[derive(Clone, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// Source pixel format.
    pub format: PixelFormat,
    pub rgba8: Vec<u8>,
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Texture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("rgba8", &format_args!("[{} bytes]", self.rgba8.len()))
            .finish()
    }
}

#[derive(Debug)]
struct Mip<'a> {
    flags: u32,
    size: usize,
    offset: i64,
    inline: Option<&'a [u8]>,
    width: u32,
    height: u32,
}

fn read_mip<'a>(r: &mut Reader<'a>) -> Result<Mip<'a>, Ue4Error> {
    r.i32()?; // bCooked
    let flags = r.u32()?;
    if flags & (BULK_COMPRESSED_ZLIB | BULK_SIZE_64BIT) != 0 {
        return Err(Ue4Error::UnsupportedBulkData(flags));
    }
    r.i32()?; // ElementCount
    let size = r.count()?; // SizeOnDisk
    let offset = r.i64()?;
    let inline = if flags & (BULK_AT_END_OF_FILE | BULK_SEPARATE_FILE | BULK_UNUSED) == 0 {
        Some(r.bytes(size)?)
    } else {
        None
    };
    Ok(Mip {
        flags,
        size,
        offset,
        inline,
        // `count` is at most i32::MAX, so these never truncate.
        width: r.count()? as u32,
        height: r.count()? as u32,
    })
}

/// The payload of a mip: inline, at the end of the `.uexp`, or in the `.ubulk`.
fn mip_payload<'a>(
    mip: &Mip<'a>,
    uasset_len: usize,
    uexp: &'a [u8],
    ubulk: Option<&'a [u8]>,
) -> Result<&'a [u8], Ue4Error> {
    if let Some(data) = mip.inline {
        return Ok(data);
    }
    // Offsets are relative to the combined uasset+uexp (ubulk: to its end minus the package tag).
    let (file, delta) = if mip.flags & BULK_SEPARATE_FILE != 0 {
        let ubulk = ubulk.ok_or(Ue4Error::MissingBulkFile)?;
        (
            ubulk,
            (uasset_len + uexp.len()).saturating_sub(PACKAGE_TAG_SIZE) as i64,
        )
    } else {
        (uexp, -(uasset_len as i64))
    };
    let start = mip
        .offset
        .checked_add(delta)
        .and_then(|o| usize::try_from(o).ok())
        .ok_or(Ue4Error::BadBulkOffset(mip.offset))?;
    start
        .checked_add(mip.size)
        .and_then(|end| file.get(start..end))
        .ok_or(Ue4Error::Eof {
            offset: start,
            need: mip.size,
            len: file.len(),
        })
}

/// Decodes `data` (one mip in `format`) to RGBA8.
fn decode(
    format: PixelFormat,
    width: usize,
    height: usize,
    data: &[u8],
) -> Result<Vec<u8>, Ue4Error> {
    let need = format.data_size(width, height);
    if data.len() < need {
        return Err(Ue4Error::MipTooSmall {
            need,
            len: data.len(),
        });
    }
    let data = &data[..need];
    Ok(match format {
        PixelFormat::B8G8R8A8 => data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[b, g, r, a]| [r, g, b, a])
            .collect(),
        PixelFormat::G8 => data.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        PixelFormat::Dxt1 => bcn::decode::<8>(data, width, height, |b| bcn::bc1(b, false)),
        PixelFormat::Dxt5 => bcn::decode::<16>(data, width, height, bcn::bc3),
    })
}

/// Reads the last `Texture2D` export of a cooked package and decodes its largest available mip.
///
/// `ubulk` is the package's `.ubulk` file, needed only when that mip is stored there.
/// UNVERIFIED: packages with several Texture2D exports (jackets `S03_52/53`) carry a stale export
/// first; the last one is the asset named like the file.
pub fn read_texture2d(
    uasset: &[u8],
    uexp: &[u8],
    ubulk: Option<&[u8]>,
) -> Result<Texture, Ue4Error> {
    let pkg = Package::parse(uasset)?;
    let export = pkg
        .exports_of_class("Texture2D")
        .last()
        .ok_or(Ue4Error::NotATexture)?;
    let start = usize::try_from(export.serial_offset)
        .ok()
        .and_then(|o| o.checked_sub(uasset.len()))
        .ok_or(Ue4Error::BadExportOffset(export.serial_offset))?;
    let mut r = Reader::at(uexp, start)?;
    read_properties(&pkg, &mut r, None)?;
    r.i32()?; // object GUID flag
    r.bytes(4)?; // FStripDataFlags of UTexture and UTexture2D
    if r.i32()? == 0 {
        return Err(Ue4Error::NoPlatformData);
    }
    if pkg.read_fname(&mut r)? == "None" {
        return Err(Ue4Error::NoPlatformData);
    }
    r.i32()?; // SkipOffset
    r.count()?; // SizeX
    r.count()?; // SizeY
    let packed = r.u32()?;
    let format_name = r.fstring()?;
    if packed & HAS_OPT_DATA != 0 {
        r.bytes(8)?; // ExtData, NumMipsInTail
    }
    r.i32()?; // FirstMipToSerialize
    let mip_count = r.count()?;
    let mut mips = Vec::new();
    for _ in 0..mip_count {
        mips.push(read_mip(&mut r)?);
    }
    let format = PixelFormat::from_name(&format_name)
        .ok_or_else(|| Ue4Error::UnsupportedPixelFormat(format_name.clone()))?;
    let mip = mips
        .iter()
        .find(|m| m.flags & BULK_UNUSED == 0 && m.size > 0)
        .ok_or(Ue4Error::NoPlatformData)?;
    let data = mip_payload(mip, uasset.len(), uexp, ubulk)?;
    let rgba8 = decode(format, mip.width as usize, mip.height as usize, data)?;
    log::debug!(
        "{}: {format_name} {}x{} ({} mips)",
        export.object_name,
        mip.width,
        mip.height,
        mips.len()
    );
    Ok(Texture {
        width: mip.width,
        height: mip.height,
        format,
        rgba8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::Fixture;

    /// One mip: (bulk flags, payload, offset, width, height). Inline payloads are written in place.
    type MipSpec<'a> = (u32, &'a [u8], i64, i32, i32);

    /// A package with one Texture2D export (or `class` export) holding one platform data.
    fn texture(class: &str, format: &str, mips: &[MipSpec], trailer: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut fx = Fixture::default();
        let class_idx = fx.import("/Script/CoreUObject", "Class", class);
        let mut b = Vec::new();
        fx.tag(&mut b, "LODBias", "IntProperty", &0i32.to_le_bytes());
        fx.none(&mut b);
        b.extend(0i32.to_le_bytes()); // GUID flag
        b.extend([1, 0, 1, 0]); // strip flags
        b.extend(1i32.to_le_bytes()); // bCooked
        let pf = fx.fname(format);
        b.extend(pf);
        b.extend(0i32.to_le_bytes()); // SkipOffset
        let (w, h) = mips.first().map_or((0, 0), |m| (m.3, m.4));
        for v in [w, h, 1] {
            b.extend(v.to_le_bytes());
        }
        b.extend(Fixture::fstring(format));
        b.extend(0i32.to_le_bytes());
        b.extend(i32::try_from(mips.len()).unwrap().to_le_bytes());
        for &(flags, payload, offset, w, h) in mips {
            b.extend(1i32.to_le_bytes());
            b.extend(flags.to_le_bytes());
            let n = i32::try_from(payload.len()).unwrap();
            b.extend(n.to_le_bytes());
            b.extend(n.to_le_bytes());
            b.extend(offset.to_le_bytes());
            if flags & (BULK_AT_END_OF_FILE | BULK_SEPARATE_FILE | BULK_UNUSED) == 0 {
                b.extend(payload);
            }
            b.extend(w.to_le_bytes());
            b.extend(h.to_le_bytes());
        }
        fx.none(&mut b);
        b.extend(trailer);
        fx.export(class_idx, "uT_Test", b.len());
        b.extend(crate::package::PACKAGE_MAGIC.to_le_bytes());
        (fx.uasset(), b)
    }

    const BGRA_2X1: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    #[test]
    fn decodes_inline_bgra_top_mip() {
        let (ua, ux) = texture(
            "Texture2D",
            "PF_B8G8R8A8",
            &[(0x48, &BGRA_2X1, 0, 2, 1)],
            &[],
        );
        let t = read_texture2d(&ua, &ux, None).unwrap();
        assert_eq!((t.width, t.height, t.format), (2, 1, PixelFormat::B8G8R8A8));
        assert_eq!(t.rgba8, [3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn decodes_g8_and_dxt1() {
        let (ua, ux) = texture("Texture2D", "PF_G8", &[(0x48, &[9, 200], 0, 1, 2)], &[]);
        let t = read_texture2d(&ua, &ux, None).unwrap();
        assert_eq!(t.rgba8, [9, 9, 9, 255, 200, 200, 200, 255]);

        let block = [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0]; // solid red
        let (ua, ux) = texture("Texture2D", "PF_DXT1", &[(0x48, &block, 0, 2, 2)], &[]);
        let t = read_texture2d(&ua, &ux, None).unwrap();
        assert_eq!((t.width, t.format), (2, PixelFormat::Dxt1));
        assert_eq!(t.rgba8, [255, 0, 0, 255].repeat(4));
    }

    #[test]
    fn skips_unused_mips_and_reads_ubulk() {
        // Mip 0 stripped, mip 1 in the .ubulk at position 2.
        let mips = [
            (BULK_UNUSED, &[][..], 0, 4, 2),
            (0x501, &BGRA_2X1[..], 0, 2, 1),
        ];
        let (ua, ux) = texture("Texture2D", "PF_B8G8R8A8", &mips, &[]);
        let base = i64::try_from(ua.len() + ux.len() - 4).unwrap();
        let mips = [
            (BULK_UNUSED, &[][..], 0, 4, 2),
            (0x501, &BGRA_2X1[..], 2 - base, 2, 1),
        ];
        let (ua, ux) = texture("Texture2D", "PF_B8G8R8A8", &mips, &[]);
        let ubulk = [[0, 0].as_slice(), &BGRA_2X1].concat();
        let t = read_texture2d(&ua, &ux, Some(&ubulk)).unwrap();
        assert_eq!((t.width, t.rgba8[0]), (2, 3));
        assert!(matches!(
            read_texture2d(&ua, &ux, None),
            Err(Ue4Error::MissingBulkFile)
        ));
        assert!(matches!(
            read_texture2d(&ua, &ux, Some(&ubulk[..5])),
            Err(Ue4Error::Eof { .. })
        ));
    }

    #[test]
    fn reads_payload_at_end_of_uexp() {
        let mips = [(BULK_AT_END_OF_FILE, &BGRA_2X1[..], 0, 2, 1)];
        let (ua, ux) = texture("Texture2D", "PF_B8G8R8A8", &mips, &BGRA_2X1);
        let at = i64::try_from(ua.len() + ux.len() - 4 - BGRA_2X1.len()).unwrap();
        let mips = [(BULK_AT_END_OF_FILE, &BGRA_2X1[..], at, 2, 1)];
        let (ua, ux) = texture("Texture2D", "PF_B8G8R8A8", &mips, &BGRA_2X1);
        assert_eq!(
            read_texture2d(&ua, &ux, None).unwrap().rgba8[..4],
            [3, 2, 1, 4]
        );
    }

    #[test]
    fn reports_unsupported_format_and_non_textures() {
        let (ua, ux) = texture("Texture2D", "PF_BC7", &[(0x48, &[0; 16], 0, 4, 4)], &[]);
        assert!(matches!(
            read_texture2d(&ua, &ux, None),
            Err(Ue4Error::UnsupportedPixelFormat(f)) if f == "PF_BC7"
        ));
        let (ua, ux) = texture("Material", "PF_G8", &[(0x48, &[0], 0, 1, 1)], &[]);
        assert!(matches!(
            read_texture2d(&ua, &ux, None),
            Err(Ue4Error::NotATexture)
        ));
    }

    #[test]
    fn short_payload_is_an_error() {
        let (ua, ux) = texture(
            "Texture2D",
            "PF_B8G8R8A8",
            &[(0x48, &BGRA_2X1, 0, 2, 2)],
            &[],
        );
        assert!(matches!(
            read_texture2d(&ua, &ux, None),
            Err(Ue4Error::MipTooSmall { need: 16, len: 8 })
        ));
    }

    #[test]
    fn data_sizes_round_blocks_up() {
        assert_eq!(PixelFormat::Dxt1.data_size(5, 1), 16);
        assert_eq!(PixelFormat::Dxt5.data_size(4, 4), 16);
        assert_eq!(PixelFormat::B8G8R8A8.data_size(usize::MAX, 2), usize::MAX);
        assert_eq!(
            PixelFormat::from_name("PF_DXT5").map(PixelFormat::name),
            Some("PF_DXT5")
        );
    }
}
