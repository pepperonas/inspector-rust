//! QR exports. STL coordinates are millimetres: 1 mm modules, four-module
//! quiet zone, 2 mm base and 0.6 mm relief. Slightly inset raised modules
//! avoid non-manifold diagonal contacts; all faces share matching edges.
use std::io::Write;

type Point = [f32; 3];

fn quad(out: &mut Vec<u8>, p: [Point; 4]) {
    for [a, b, c] in [[p[0], p[1], p[2]], [p[0], p[2], p[3]]] {
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
}

pub fn stl(matrix: &[Vec<bool>]) -> Result<Vec<u8>, String> {
    let n = matrix.len();
    if !(21..=177).contains(&n)
        || !(n - 17).is_multiple_of(4)
        || matrix.iter().any(|r| r.len() != n)
    {
        return Err("Invalid QR matrix".into());
    }
    let size = n + 8;
    let mut out = vec![0; 84];
    for y in 0..size {
        for x in 0..size {
            // Flip QR rows so the top view is readable, never mirrored.
            let dark = x >= 4 && x < n + 4 && y >= 4 && y < n + 4 && matrix[n + 3 - y][x - 4];
            let (x, y) = (x as f32, y as f32);
            let bottom = [
                [x, y, 0.],
                [x + 1., y, 0.],
                [x + 1., y + 1., 0.],
                [x, y + 1., 0.],
            ];
            let top = bottom.map(|[x, y, _]| [x, y, 2.]);
            quad(&mut out, [bottom[3], bottom[2], bottom[1], bottom[0]]);
            if dark {
                let inner = [
                    [x + 0.04, y + 0.04, 2.],
                    [x + 0.96, y + 0.04, 2.],
                    [x + 0.96, y + 0.96, 2.],
                    [x + 0.04, y + 0.96, 2.],
                ];
                let raised = inner.map(|[x, y, _]| [x, y, 2.6]);
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
                assert!(p[0] >= 0. && p[0] <= 29. && p[1] >= 0. && p[1] <= 29.);
                assert!([0., 2., 2.6].contains(&p[2]));
                if p[2] > 2. {
                    raised.push(*p);
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
        assert!((volume - (29. * 29. * 2. + 3. * 0.92 * 0.92 * 0.6)).abs() < 0.001);
        assert!(raised
            .iter()
            .all(|p| p[0] >= 4. && p[0] < 7. && p[1] > 23. && p[1] < 25.));
    }
}
