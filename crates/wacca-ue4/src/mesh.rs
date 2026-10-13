//! Cooked `UStaticMesh` LOD0 geometry (positions, UV0, indices, sections). Spec:
//! `docs/research/11-shaders-and-meshes.md` §1.4 (port of `tools/research/ue4_mesh.py`).
//!
//! Cooked `FStaticMeshLODResources` layout (UE 4.19, verified on WACCA's `Vfx` meshes):
//! sections `i32 n, n × 7 i32 {MaterialIndex, FirstIndex, NumTriangles, MinVertexIndex, MaxVertexIndex,
//! bEnableCollision, bCastShadow}, i32`; `FPositionVertexBuffer` `{Stride 12, NumVertices, bulk {12,
//! n, float3[]}}`; `FStaticMeshVertexBuffer` `{u16 strip flags, NumTexCoords, NumVertices,
//! bFullPrecisionUVs, bHighPrecisionTangents, bulk tangents, bulk UVs (interleaved per vertex)}`;
//! `FColorVertexBuffer` `{u16 strip flags, Stride, NumVertices, [bulk BGRA8]}`;
//! `FRawStaticIndexBuffer` `{b32Bit, bulk {1, bytes, u16|u32[]}}`. The position buffer is found by
//! its header `(12, N, 12, N)`; the sections are read backwards from it.

use crate::Ue4Error;
use crate::reader::Reader;

/// One material section: triangles `first_index / 3 .. + triangles` use material slot `material`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshSection {
    pub material: u32,
    pub first_index: u32,
    pub triangles: u32,
}

/// LOD0 of a static mesh in raw UE coordinates (centimetres; X forward, Y right, Z up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticMesh {
    pub positions: Vec<[f32; 3]>,
    /// First UV channel as stored (UE convention: v down).
    pub uv0: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub sections: Vec<MeshSection>,
}

/// Most sections a mesh may have (the note meshes have up to 2 × 60).
const MAX_SECTIONS: usize = 256;

/// Parses LOD0 from a static mesh's `.uexp`.
pub fn read_static_mesh(uexp: &[u8]) -> Result<StaticMesh, Ue4Error> {
    let (pos, count) = find_position_buffer(uexp).ok_or(Ue4Error::NotAStaticMesh)?;
    let sections = read_sections(uexp, pos).ok_or(Ue4Error::NotAStaticMesh)?;
    let mut r = Reader::at(uexp, pos + 16)?;
    let positions = (0..count)
        .map(|_| Ok([r.f32()?, r.f32()?, r.f32()?]))
        .collect::<Result<Vec<_>, Ue4Error>>()?;
    r.u16()?; // strip flags
    let (tex_coords, count2, full_uv, _high_tangents) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
    if count2 as usize != count || tex_coords == 0 {
        return Err(Ue4Error::NotAStaticMesh);
    }
    skip_bulk(&mut r)?; // tangents
    let (elem, n) = (r.u32()? as usize, r.u32()? as usize);
    let uv_bytes = elem.checked_mul(n).ok_or(Ue4Error::NotAStaticMesh)?;
    let uvs = r.bytes(uv_bytes)?;
    let stride = elem * tex_coords as usize;
    if n != count * tex_coords as usize || elem != if full_uv != 0 { 8 } else { 4 } {
        return Err(Ue4Error::NotAStaticMesh);
    }
    let uv0 = uvs
        .chunks_exact(stride)
        .map(|v| {
            if full_uv != 0 {
                [f32_at(v, 0), f32_at(v, 4)]
            } else {
                [half_at(v, 0), half_at(v, 2)]
            }
        })
        .collect();
    r.u16()?; // colour strip flags
    let (_stride, colours) = (r.u32()?, r.u32()?);
    if colours != 0 {
        skip_bulk(&mut r)?;
    }
    let wide = r.u32()? != 0;
    let (_elem, bytes) = (r.u32()?, r.u32()? as usize);
    let raw = r.bytes(bytes)?;
    let indices: Vec<u32> = if wide {
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|&b| u32::from_le_bytes(b))
            .collect()
    } else {
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|&b| u32::from(u16::from_le_bytes(b)))
            .collect()
    };
    if indices.iter().any(|&i| i as usize >= count) {
        return Err(Ue4Error::NotAStaticMesh);
    }
    Ok(StaticMesh {
        positions,
        uv0,
        indices,
        sections,
    })
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(d: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

fn half_at(d: &[u8], at: usize) -> f32 {
    half_to_f32(u16::from_le_bytes([d[at], d[at + 1]]))
}

/// IEEE 754 binary16 → f32.
fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 if frac == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

/// Offset and vertex count of the first `(12, N, 12, N)` header.
fn find_position_buffer(d: &[u8]) -> Option<(usize, usize)> {
    (0..d.len().saturating_sub(15)).find_map(|p| {
        let n = u32_at(d, p + 4)?;
        (u32_at(d, p)? == 12 && u32_at(d, p + 8)? == 12 && u32_at(d, p + 12)? == n && n > 0)
            .then_some((p, n as usize))
    })
}

/// Section records ending 4 bytes before the position buffer at `pos`: the first `n` for which
/// the count matches and the records are consecutive (material = index, first index = running sum).
fn read_sections(d: &[u8], pos: usize) -> Option<Vec<MeshSection>> {
    (1..MAX_SECTIONS).find_map(|n| {
        let start = pos.checked_sub(8 + n * 28)?;
        if u32_at(d, start)? as usize != n {
            return None;
        }
        let mut first = 0;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let at = start + 4 + i * 28;
            let (material, first_index, triangles) =
                (u32_at(d, at)?, u32_at(d, at + 4)?, u32_at(d, at + 8)?);
            if material as usize != i || first_index != first {
                return None;
            }
            first += 3 * triangles;
            out.push(MeshSection {
                material,
                first_index,
                triangles,
            });
        }
        Some(out)
    })
}

/// Skips a bulk array `{u32 element size, u32 count, data}`.
fn skip_bulk(r: &mut Reader) -> Result<(), Ue4Error> {
    let (elem, n) = (r.u32()? as usize, r.u32()? as usize);
    r.bytes(elem.checked_mul(n).ok_or(Ue4Error::NotAStaticMesh)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(out: &mut Vec<u8>, words: &[u32]) {
        for w in words {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }

    /// A one-triangle mesh with two half-precision UV channels, laid out like a cooked .uexp.
    fn fixture(wide: bool) -> Vec<u8> {
        let mut d = vec![0xaa; 7]; // unrelated leading bytes
        put(&mut d, &[1, 0, 0, 1, 0, 2, 1, 1, 0]); // 1 section: mat 0, first 0, 1 tri, ..., trailer
        put(&mut d, &[12, 3, 12, 3]);
        for p in [[1.0f32, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]] {
            for c in p {
                d.extend_from_slice(&c.to_le_bytes());
            }
        }
        d.extend_from_slice(&[0, 0]);
        put(&mut d, &[2, 3, 0, 0]); // 2 UV sets, 3 vertices, half UVs
        put(&mut d, &[8, 3]);
        d.extend_from_slice(&[0; 24]); // tangents
        put(&mut d, &[4, 6]);
        // half: 0.0 = 0x0000, 0.5 = 0x3800, 1.0 = 0x3c00, -1.0 = 0xbc00; UV1 = 0x3c00.
        for (u, v) in [(0x0000u16, 0x3800u16), (0x3c00, 0x0000), (0xbc00, 0x3c00)] {
            for h in [u, v, 0x3c00, 0x3c00] {
                d.extend_from_slice(&h.to_le_bytes());
            }
        }
        d.extend_from_slice(&[0, 0]);
        put(&mut d, &[4, 0]); // no colours
        if wide {
            put(&mut d, &[1, 1, 12, 0, 2, 1]);
        } else {
            put(&mut d, &[0, 1, 6]);
            for i in [0u16, 2, 1] {
                d.extend_from_slice(&i.to_le_bytes());
            }
        }
        d
    }

    #[test]
    fn reads_positions_uv0_indices_and_sections() {
        for wide in [false, true] {
            let m = read_static_mesh(&fixture(wide)).unwrap();
            assert_eq!(m.positions[2], [7.0, 8.0, 9.0]);
            assert_eq!(m.uv0, [[0.0, 0.5], [1.0, 0.0], [-1.0, 1.0]]);
            assert_eq!(m.indices, [0, 2, 1]);
            assert_eq!(
                m.sections,
                [MeshSection {
                    material: 0,
                    first_index: 0,
                    triangles: 1
                }]
            );
        }
    }

    #[test]
    fn rejects_other_data() {
        assert!(read_static_mesh(&[0; 64]).is_err());
        let mut bad = fixture(false);
        let n = bad.len();
        bad[n - 2] = 9; // index out of range
        assert!(read_static_mesh(&bad).is_err());
    }

    #[test]
    fn half_floats() {
        assert_eq!(half_to_f32(0x3555), 0.333_251_95);
        assert_eq!(half_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(half_to_f32(0xc000), -2.0);
    }
}
