//! QR exports. STL coordinates are millimetres: 1 mm modules, a two-module
//! quiet zone (`QUIET`, mirrored by `QR_QUIET_MODULES` in lib/qr.ts), 2 mm base
//! and 0.6 mm relief. Slightly inset raised modules avoid non-manifold diagonal
//! contacts; all faces share matching edges.
//!
//! The base plate has ROUNDED CORNERS (2026-09-26) with radius = the quiet
//! zone, so rounding only ever touches light border cells. Each corner block
//! (`QUIET`×`QUIET` cells) is replaced by a quarter disk centred on its inner
//! corner. Its straight edges carry the same integer vertices the neighbouring
//! cells use (no T-junctions), and it is triangulated as a fan from the arc
//! midpoint — the quarter disk is convex and no three arc points are
//! collinear, so no fan triangle is degenerate.
use std::io::Write;

type Point = [f32; 3];

/// Quiet zone in modules (= millimetres in the STL). Also the corner radius.
const QUIET: usize = 2;
/// Segments per rounded corner (a quarter circle).
const CORNER_SEGMENTS: usize = 16;
const BASE_Z: f32 = 2.;
const RELIEF_Z: f32 = 2.6;

fn facet(out: &mut Vec<u8>, [a, b, c]: [Point; 3]) {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = normal.iter().map(|x| x * x).sum::<f32>().sqrt();
    for value in normal
        .map(|x| x / length)
        .into_iter()
        .chain(a)
        .chain(b)
        .chain(c)
    {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
}

fn quad(out: &mut Vec<u8>, p: [Point; 4]) {
    facet(out, [p[0], p[1], p[2]]);
    facet(out, [p[0], p[2], p[3]]);
}

/// Outline of the bottom-left rounded corner (block `[0,r]²`, quarter disk
/// centred on `(r,r)`), counter-clockwise seen from above. Returns the vertices
/// and the index of the arc midpoint (the fan apex).
fn corner_outline(r: usize) -> (Vec<[f32; 2]>, usize) {
    let rf = r as f32;
    let mut v = Vec::new();
    // Right edge (r,0) → (r,r): the neighbouring cells' left edges.
    for i in 0..=r {
        v.push([rf, i as f32]);
    }
    // Top edge (r-1,r) → (0,r): the neighbouring cells' bottom edges.
    for i in (0..r).rev() {
        v.push([i as f32, rf]);
    }
    // Arc (0,r) → (r,0) through the corner; endpoints are the exact grid
    // points already pushed, only the interior is computed.
    let mut apex = 0;
    for j in 1..CORNER_SEGMENTS {
        let t = std::f32::consts::PI * (1. + 0.5 * j as f32 / CORNER_SEGMENTS as f32);
        if j == CORNER_SEGMENTS / 2 {
            apex = v.len();
        }
        v.push([rf + rf * t.cos(), rf + rf * t.sin()]);
    }
    (v, apex)
}

/// Rotate a plate point by `k` quarter turns about the plate centre. Rotation
/// keeps the winding, so the canonical CCW corner stays CCW at every corner.
fn rotate(p: [f32; 2], size: f32, k: usize) -> [f32; 2] {
    (0..k).fold(p, |[x, y], _| [size - y, x])
}

fn rounded_corner(out: &mut Vec<u8>, size: f32, k: usize) {
    let (outline, apex) = corner_outline(QUIET);
    let v: Vec<[f32; 2]> = outline.iter().map(|&p| rotate(p, size, k)).collect();
    let len = v.len();
    let at = |p: [f32; 2], z: f32| [p[0], p[1], z];
    // Top (z = BASE_Z, facing up) and bottom (z = 0, facing down) fans.
    for i in 1..len - 1 {
        let (a, b) = (v[(apex + i) % len], v[(apex + i + 1) % len]);
        facet(out, [at(v[apex], BASE_Z), at(a, BASE_Z), at(b, BASE_Z)]);
        facet(out, [at(v[apex], 0.), at(b, 0.), at(a, 0.)]);
    }
    // Curved side wall: the arc runs from (0,r) (index 2r) to (r,0) (index 0).
    let arc_start = 2 * QUIET;
    let mut chain: Vec<[f32; 2]> = v[arc_start..].to_vec();
    chain.push(v[0]);
    for w in chain.windows(2) {
        let (p, q) = (w[0], w[1]);
        quad(out, [at(p, 0.), at(q, 0.), at(q, BASE_Z), at(p, BASE_Z)]);
    }
}

pub fn stl(matrix: &[Vec<bool>]) -> Result<Vec<u8>, String> {
    let n = matrix.len();
    if !(21..=177).contains(&n)
        || !(n - 17).is_multiple_of(4)
        || matrix.iter().any(|r| r.len() != n)
    {
        return Err("Invalid QR matrix".into());
    }
    let q = QUIET;
    let size = n + 2 * q;
    let mut out = vec![0; 84];
    for y in 0..size {
        for x in 0..size {
            // Corner blocks are drawn as rounded patches below.
            if (x < q || x >= size - q) && (y < q || y >= size - q) {
                continue;
            }
            // Flip QR rows so the top view is readable, never mirrored.
            let dark = x >= q && x < n + q && y >= q && y < n + q && matrix[n + q - 1 - y][x - q];
            let (x, y) = (x as f32, y as f32);
            let bottom = [
                [x, y, 0.],
                [x + 1., y, 0.],
                [x + 1., y + 1., 0.],
                [x, y + 1., 0.],
            ];
            let top = bottom.map(|[x, y, _]| [x, y, BASE_Z]);
            quad(&mut out, [bottom[3], bottom[2], bottom[1], bottom[0]]);
            if dark {
                let inner = [
                    [x + 0.04, y + 0.04, BASE_Z],
                    [x + 0.96, y + 0.04, BASE_Z],
                    [x + 0.96, y + 0.96, BASE_Z],
                    [x + 0.04, y + 0.96, BASE_Z],
                ];
                let raised = inner.map(|[x, y, _]| [x, y, RELIEF_Z]);
                for i in 0..4 {
                    let j = (i + 1) % 4;
                    quad(&mut out, [top[i], top[j], inner[j], inner[i]]);
                    quad(&mut out, [inner[i], inner[j], raised[j], raised[i]]);
                }
                quad(&mut out, raised);
            } else {
                quad(&mut out, top);
            }
            for (i, boundary) in [
                y == 0.,
                x == (size - 1) as f32,
                y == (size - 1) as f32,
                x == 0.,
            ]
            .into_iter()
            .enumerate()
            {
                if boundary {
                    let j = (i + 1) % 4;
                    quad(&mut out, [bottom[i], bottom[j], top[j], top[i]]);
                }
            }
        }
    }
    for k in 0..4 {
        rounded_corner(&mut out, size as f32, k);
    }
    let count = ((out.len() - 84) / 50) as u32;
    out[80..84].copy_from_slice(&count.to_le_bytes());
    Ok(out)
}

pub fn save(png_b64: Option<String>, matrix: Option<Vec<Vec<bool>>>) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (bytes, extension) = match (png_b64, matrix) {
        (Some(png), None) => {
            let bytes = STANDARD.decode(png).map_err(|e| e.to_string())?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err("Invalid PNG".into());
            }
            (bytes, "png")
        }
        (None, Some(matrix)) => (stl(&matrix)?, "stl"),
        _ => return Err("Provide either PNG or QR matrix".into()),
    };
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or("No Downloads directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("qr-{:032x}.{}", rand::random::<u128>(), extension));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(&bytes) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error.to_string());
    }
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rejects_invalid_matrices() {
        for matrix in [
            vec![],
            vec![vec![false; 20]; 20],
            vec![vec![false; 22]; 21],
            vec![vec![false; 181]; 181],
        ] {
            assert!(stl(&matrix).is_err());
        }
    }

    #[test]
    fn relief_is_closed_outward_oriented_and_not_mirrored() {
        const SIZE: f32 = (21 + 2 * QUIET) as f32;
        const R: f32 = QUIET as f32;
        let mut matrix = vec![vec![false; 21]; 21];
        // Include diagonal and edge-adjacent cells, the hard topology cases.
        matrix[0][0] = true;
        matrix[1][1] = true;
        matrix[1][2] = true;
        let bytes = stl(&matrix).unwrap();
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 84 + count * 50);
        let mut edges = HashMap::<([u32; 3], [u32; 3]), (usize, i32)>::new();
        let mut volume = 0f64;
        let mut raised = Vec::new();
        for face in bytes[84..].as_chunks::<50>().0 {
            let floats: Vec<f32> = face[..48]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            let vertices: Vec<Point> = floats[3..]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect();
            for p in &vertices {
                assert!(p[0] >= 0. && p[0] <= SIZE && p[1] >= 0. && p[1] <= SIZE);
                assert!([0., BASE_Z, RELIEF_Z].contains(&p[2]));
                if p[2] > BASE_Z {
                    raised.push(*p);
                }
                // Rounded corners: nothing may reach into the cut-away corner.
                for (cx, cy) in [(R, R), (SIZE - R, R), (R, SIZE - R), (SIZE - R, SIZE - R)] {
                    let in_block = (p[0] - cx).abs() <= R && (p[1] - cy).abs() <= R
                        && (p[0] < R || p[0] > SIZE - R)
                        && (p[1] < R || p[1] > SIZE - R);
                    if in_block {
                        let d = ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt();
                        assert!(d <= R + 1e-4, "vertex {p:?} outside the rounded corner");
                    }
                }
            }
            let [a, b, c] = [vertices[0], vertices[1], vertices[2]];
            let cross = [
                b[1] as f64 * c[2] as f64 - b[2] as f64 * c[1] as f64,
                b[2] as f64 * c[0] as f64 - b[0] as f64 * c[2] as f64,
                b[0] as f64 * c[1] as f64 - b[1] as f64 * c[0] as f64,
            ];
            volume +=
                (a[0] as f64 * cross[0] + a[1] as f64 * cross[1] + a[2] as f64 * cross[2]) / 6.;
            for i in 0..3 {
                let a = vertices[i].map(f32::to_bits);
                let b = vertices[(i + 1) % 3].map(f32::to_bits);
                let (key, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
                let entry = edges.entry(key).or_default();
                entry.0 += 1;
                entry.1 += sign;
            }
        }
        assert!(edges
            .values()
            .all(|&(count, balance)| count == 2 && balance == 0));
        // Plate = full square minus the four cut-away corner slivers.
        let (outline, _) = corner_outline(QUIET);
        let patch: f64 = outline
            .iter()
            .zip(outline.iter().cycle().skip(1))
            .map(|(a, b)| (a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64) / 2.)
            .sum();
        assert!(patch > 0., "corner outline must be counter-clockwise");
        let plate = (SIZE as f64).powi(2) - 4. * ((R as f64).powi(2) - patch);
        assert!((volume - (plate * 2. + 3. * 0.92 * 0.92 * 0.6)).abs() < 0.01);
        // Corners really are rounded: less area than the sharp square.
        assert!(plate < (SIZE as f64).powi(2) - 0.5);
        // Dark modules at matrix (0,0), (1,1), (1,2) → top-left, not mirrored.
        let q = QUIET as f32;
        assert!(raised
            .iter()
            .all(|p| p[0] >= q && p[0] < q + 3. && p[1] > SIZE - q - 2. && p[1] < SIZE - q));
    }

    /// Visual check: `IR_DUMP_DIR=… cargo test --lib dump_qr_stl -- --ignored`.
    #[test]
    #[ignore]
    fn dump_qr_stl() {
        let Ok(dir) = std::env::var("IR_DUMP_DIR") else { return };
        let mut m = vec![vec![false; 25]; 25];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = (r * 7 + c * 3) % 5 < 2 || r < 7 && c < 7;
            }
        }
        std::fs::write(format!("{dir}/qr.stl"), stl(&m).unwrap()).unwrap();
    }

}
