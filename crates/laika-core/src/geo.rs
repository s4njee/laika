//! Map support: GPS parsing, Web Mercator projection, tile coverage,
//! clustering, fit-to-bounds, and coarse place groups. Pure and UI-free.

use std::collections::HashMap;

/// Web Mercator's latitude limit (the world is a square).
pub const MAX_LAT: f64 = 85.051_128_78;
pub const TILE: f64 = 256.;
pub const MIN_ZOOM: f64 = 1.;
pub const MAX_ZOOM: f64 = 19.;

/// Parse a stored GPS string into decimal (lat, lon). Accepts the EXIF
/// display form (`33 deg 3 min 47.3 secN, 96 deg 48 min 38.5 secW`),
/// rational triples (`33/1, 2/1, 458/100N, 96/1, 53/1, 3024/100W`), and
/// decimal pairs (`33.063, -96.810`). (0, 0) and out-of-range values read
/// as no location.
pub fn parse_gps(s: &str) -> Option<(f64, f64)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (lat, lon) = if let Some(i) = s.find(['N', 'S']) {
        let lat = dms(&s[..i])? * if &s[i..i + 1] == "S" { -1. } else { 1. };
        let rest = &s[i + 1..];
        let j = rest.find(['E', 'W'])?;
        let lon = dms(&rest[..j])? * if &rest[j..j + 1] == "W" { -1. } else { 1. };
        (lat, lon)
    } else {
        let mut parts = s.split(',').map(|p| p.trim().parse::<f64>());
        let lat = parts.next()?.ok()?;
        let lon = parts.next()?.ok()?;
        if parts.next().is_some() {
            return None;
        }
        (lat, lon)
    };
    let valid = lat.is_finite()
        && lon.is_finite()
        && (-90. ..=90.).contains(&lat)
        && (-180. ..=180.).contains(&lon)
        && !(lat.abs() < 1e-9 && lon.abs() < 1e-9);
    valid.then_some((lat, lon))
}

/// Degrees from up to three numbers (degrees, minutes, seconds), each a
/// decimal or an `a/b` rational.
fn dms(part: &str) -> Option<f64> {
    let mut vals = Vec::new();
    for tok in part.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '/' || c == '-')) {
        if tok.is_empty() || tok == "-" {
            continue;
        }
        let v = match tok.split_once('/') {
            Some((a, b)) => {
                let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
                if b == 0. {
                    return None;
                }
                a / b
            }
            None => tok.parse::<f64>().ok()?,
        };
        vals.push(v);
    }
    if vals.is_empty() || vals.len() > 3 {
        return None;
    }
    let sign = if vals[0] < 0. { -1. } else { 1. };
    let deg = vals[0].abs()
        + vals.get(1).copied().unwrap_or(0.) / 60.
        + vals.get(2).copied().unwrap_or(0.) / 3600.;
    Some(sign * deg)
}

/// Stored form for locations Laika sets: a decimal pair.
pub fn format_gps(lat: f64, lon: f64) -> String {
    format!("{lat:.6}, {lon:.6}")
}

/// Human form: `33.0631° N, 96.8107° W`.
pub fn display_gps(lat: f64, lon: f64) -> String {
    format!(
        "{:.4}° {}, {:.4}° {}",
        lat.abs(),
        if lat < 0. { "S" } else { "N" },
        lon.abs(),
        if lon < 0. { "W" } else { "E" }
    )
}

/// XMP `exif:GPSLatitude` form (`33,3.788452N`).
pub fn xmp_coord(value: f64, positive: char, negative: char) -> String {
    let hemi = if value < 0. { negative } else { positive };
    let v = value.abs();
    let deg = v.trunc();
    let min = (v - deg) * 60.;
    format!("{},{:.6}{hemi}", deg as i64, min)
}

/// Parse `exif:GPSLatitude`/`GPSLongitude` (`33,3.788452N` or
/// `33,3,47.3N`) to signed degrees.
pub fn parse_xmp_coord(s: &str) -> Option<f64> {
    let s = s.trim();
    let hemi = s.chars().last()?;
    let sign = match hemi {
        'N' | 'E' => 1.,
        'S' | 'W' => -1.,
        _ => return None,
    };
    let body = &s[..s.len() - 1];
    let nums: Vec<f64> = body
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let deg = match nums.as_slice() {
        [d] => *d,
        [d, m] => d + m / 60.,
        [d, m, s] => d + m / 60. + s / 3600.,
        _ => return None,
    };
    Some(sign * deg)
}

/// World pixel of a coordinate at a (fractional) zoom.
pub fn project(lat: f64, lon: f64, zoom: f64) -> (f64, f64) {
    let size = TILE * 2f64.powf(zoom);
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.) / 360. * size;
    let y = (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2. * size;
    (x, y)
}

pub fn unproject(x: f64, y: f64, zoom: f64) -> (f64, f64) {
    let size = TILE * 2f64.powf(zoom);
    let lon = x / size * 360. - 180.;
    let n = std::f64::consts::PI * (1. - 2. * y / size);
    let lat = n.sinh().atan().to_degrees();
    (lat.clamp(-MAX_LAT, MAX_LAT), wrap_lon(lon))
}

pub fn wrap_lon(lon: f64) -> f64 {
    let mut l = (lon + 180.) % 360.;
    if l < 0. {
        l += 360.;
    }
    l - 180.
}

/// A map view: center coordinate and zoom over a viewport in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
    pub width: f64,
    pub height: f64,
}

impl View {
    /// Viewport pixel of a coordinate (may be outside the viewport).
    pub fn to_screen(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (cx, cy) = project(self.lat, self.lon, self.zoom);
        let (x, y) = project(lat, lon, self.zoom);
        let world = TILE * 2f64.powf(self.zoom);
        // Nearest copy horizontally (the world wraps).
        let mut dx = x - cx;
        if dx > world / 2. {
            dx -= world;
        } else if dx < -world / 2. {
            dx += world;
        }
        (self.width / 2. + dx, self.height / 2. + (y - cy))
    }

    pub fn to_coord(&self, sx: f64, sy: f64) -> (f64, f64) {
        let (cx, cy) = project(self.lat, self.lon, self.zoom);
        unproject(
            cx + sx - self.width / 2.,
            cy + sy - self.height / 2.,
            self.zoom,
        )
    }

    /// Move the view by a screen delta (drag).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (cx, cy) = project(self.lat, self.lon, self.zoom);
        let (lat, lon) = unproject(cx - dx, cy - dy, self.zoom);
        self.lat = lat;
        self.lon = lon;
    }

    /// Zoom keeping the coordinate under a screen point fixed.
    pub fn zoom_at(&mut self, new_zoom: f64, sx: f64, sy: f64) {
        let new_zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let (lat, lon) = self.to_coord(sx, sy);
        self.zoom = new_zoom;
        let (px, py) = project(lat, lon, new_zoom);
        let (clat, clon) = unproject(
            px - (sx - self.width / 2.),
            py - (sy - self.height / 2.),
            new_zoom,
        );
        self.lat = clat;
        self.lon = clon;
    }

    /// Tiles covering the viewport at the nearest integer level plus
    /// `detail` (1 = half-size tiles, sharp on a 2× display):
    /// (z, x, y, screen left, screen top, drawn size).
    pub fn tiles(&self, detail: f64) -> Vec<(u8, u32, u32, f64, f64, f64)> {
        let z = (self.zoom + detail).round().clamp(0., MAX_ZOOM) as u8;
        let scale = 2f64.powf(self.zoom - z as f64);
        let size = TILE * scale;
        let n = 1i64 << z;
        let (cx, cy) = project(self.lat, self.lon, self.zoom);
        let left = cx - self.width / 2.;
        let top = cy - self.height / 2.;
        let x0 = (left / size).floor() as i64;
        let x1 = ((left + self.width) / size).floor() as i64;
        let y0 = (top / size).floor().max(0.) as i64;
        let y1 = (((top + self.height) / size).floor() as i64).min(n - 1);
        let mut out = Vec::new();
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let wrapped = tx.rem_euclid(n) as u32;
                out.push((
                    z,
                    wrapped,
                    ty as u32,
                    tx as f64 * size - left,
                    ty as f64 * size - top,
                    size,
                ));
            }
        }
        out
    }

    /// Geographic bounds of the viewport: (south, west, north, east).
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let (n, w) = self.to_coord(0., 0.);
        let (s, e) = self.to_coord(self.width, self.height);
        (s, w, n, e)
    }
}

/// A group of nearby photos drawn as one circle.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub ids: Vec<i64>,
    pub lat: f64,
    pub lon: f64,
    /// Screen position of the cluster's centroid.
    pub x: f64,
    pub y: f64,
}

/// Screen-space grid clustering of located photos (`cell` px cells),
/// dropping clusters well outside the viewport. Sorted largest first.
pub fn cluster(points: &[(i64, f64, f64)], view: &View, cell: f64) -> Vec<Cluster> {
    let cell = cell.max(8.);
    let mut buckets: HashMap<(i64, i64), (Vec<i64>, f64, f64, f64, f64)> = HashMap::new();
    for &(id, lat, lon) in points {
        let (x, y) = view.to_screen(lat, lon);
        if x < -cell || y < -cell || x > view.width + cell || y > view.height + cell {
            continue;
        }
        let key = ((x / cell).floor() as i64, (y / cell).floor() as i64);
        let b = buckets
            .entry(key)
            .or_insert_with(|| (Vec::new(), 0., 0., 0., 0.));
        b.0.push(id);
        b.1 += lat;
        b.2 += lon;
        b.3 += x;
        b.4 += y;
    }
    let mut out: Vec<Cluster> = buckets
        .into_values()
        .map(|(ids, lat, lon, x, y)| {
            let n = ids.len() as f64;
            Cluster {
                lat: lat / n,
                lon: lon / n,
                x: x / n,
                y: y / n,
                ids,
            }
        })
        .collect();
    out.sort_by(|a, b| b.ids.len().cmp(&a.ids.len()).then(a.ids[0].cmp(&b.ids[0])));
    out
}

/// Center and zoom that fit coordinates into a viewport with padding.
pub fn fit(points: &[(f64, f64)], width: f64, height: f64, pad: f64) -> Option<(f64, f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(lat, lon) in points {
        let (x, y) = project(lat, lon, 0.);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (w, h) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
    let zx = ((width - 2. * pad).max(1.) / w).log2();
    let zy = ((height - 2. * pad).max(1.) / h).log2();
    let zoom = zx.min(zy).clamp(MIN_ZOOM, 15.);
    let (lat, lon) = unproject((x0 + x1) / 2., (y0 + y1) / 2., 0.);
    Some((lat, lon, zoom))
}

/// A coarse place group for the Places list.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub label: String,
    pub lat: f64,
    pub lon: f64,
    pub count: usize,
}

/// Group located photos into ~`step`° cells, largest first. Labels are
/// coordinates (Laika has no offline place names).
pub fn places(points: &[(i64, f64, f64)], step: f64, limit: usize) -> Vec<Place> {
    let mut cells: HashMap<(i64, i64), (f64, f64, usize)> = HashMap::new();
    for &(_, lat, lon) in points {
        let key = ((lat / step).floor() as i64, (lon / step).floor() as i64);
        let c = cells.entry(key).or_insert((0., 0., 0));
        c.0 += lat;
        c.1 += lon;
        c.2 += 1;
    }
    let mut out: Vec<Place> = cells
        .into_values()
        .map(|(lat, lon, n)| {
            let (lat, lon) = (lat / n as f64, lon / n as f64);
            Place {
                label: format!(
                    "{:.2}° {} · {:.2}° {}",
                    lat.abs(),
                    if lat < 0. { "S" } else { "N" },
                    lon.abs(),
                    if lon < 0. { "W" } else { "E" }
                ),
                lat,
                lon,
                count: n,
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn parses_every_stored_form() {
        let (lat, lon) =
            parse_gps("33 deg 3 min 47.3071 secN, 96 deg 48 min 38.4906 secW").unwrap();
        assert!(near(lat, 33.063141) && near(lon, -96.810692), "{lat} {lon}");
        let (lat, lon) = parse_gps("33/1, 2/1, 458/100N, 96/1, 53/1, 3024/100W").unwrap();
        assert!(
            near(lat, 33. + 2. / 60. + 4.58 / 3600.)
                && near(lon, -(96. + 53. / 60. + 30.24 / 3600.))
        );
        let (lat, lon) = parse_gps("-33.8688, 151.2093").unwrap();
        assert!(near(lat, -33.8688) && near(lon, 151.2093));
        let (lat, lon) = parse_gps("51 deg 30 min 0 secS, 0 deg 7 min 39 secE").unwrap();
        assert!(near(lat, -51.5) && near(lon, 7. / 60. + 39. / 3600.));
        for bad in [
            "",
            "0, 0",
            "0 deg 0 min 0 secN, 0 deg 0 min 0 secE",
            "95, 10",
            "abc",
            "1/0N, 2E",
        ] {
            assert_eq!(parse_gps(bad), None, "{bad}");
        }
        let (lat, lon) = parse_gps(&format_gps(48.858370, 2.294481)).unwrap();
        assert!(near(lat, 48.85837) && near(lon, 2.294481));
        assert_eq!(display_gps(-33.8688, 151.2093), "33.8688° S, 151.2093° E");
        let x = xmp_coord(-96.810692, 'E', 'W');
        assert!(
            x.ends_with('W') && near(parse_xmp_coord(&x).unwrap(), -96.810692),
            "{x}"
        );
        assert!(near(parse_xmp_coord("33,3,47.3071N").unwrap(), 33.063141));
    }

    #[test]
    fn projection_round_trips_and_views_pan_and_zoom() {
        for &(lat, lon) in &[
            (0., 0.),
            (48.8584, 2.2945),
            (-33.8688, 151.2093),
            (64.1466, -21.9426),
        ] {
            for z in [1., 5.5, 12.] {
                let (x, y) = project(lat, lon, z);
                let (la, lo) = unproject(x, y, z);
                assert!(near(la, lat) && near(lo, lon));
            }
        }
        let mut v = View {
            lat: 48.8584,
            lon: 2.2945,
            zoom: 12.,
            width: 800.,
            height: 600.,
        };
        let (sx, sy) = v.to_screen(48.8584, 2.2945);
        assert!(near(sx, 400.) && near(sy, 300.));
        // Zooming keeps the point under the cursor fixed.
        let target = v.to_coord(100., 150.);
        v.zoom_at(14.25, 100., 150.);
        let (sx, sy) = v.to_screen(target.0, target.1);
        assert!((sx - 100.).abs() < 1e-6 && (sy - 150.).abs() < 1e-6);
        // Panning moves content with the pointer.
        let before = v.to_screen(48.86, 2.3);
        v.pan(40., -25.);
        let after = v.to_screen(48.86, 2.3);
        assert!((after.0 - before.0 - 40.).abs() < 1e-6 && (after.1 - before.1 + 25.).abs() < 1e-6);
        // Tiles cover the viewport without gaps.
        let tiles = v.tiles(0.);
        assert!(!tiles.is_empty());
        let min_x = tiles.iter().map(|t| t.3).fold(f64::MAX, f64::min);
        let max_x = tiles.iter().map(|t| t.3 + t.5).fold(f64::MIN, f64::max);
        assert!(min_x <= 0. && max_x >= v.width);
        // Across the antimeridian the tile x wraps.
        let dateline = View {
            lat: 0.,
            lon: 179.9,
            zoom: 3.,
            width: 800.,
            height: 400.,
        };
        assert!(dateline.tiles(0.).iter().all(|t| t.1 < 8));
        assert!(dateline.to_screen(0., -179.9).0 > 400.);
    }

    #[test]
    fn clusters_fit_and_places() {
        let pts: Vec<(i64, f64, f64)> = vec![
            (1, 48.8584, 2.2945),
            (2, 48.8585, 2.2946),
            (3, 48.8606, 2.3376),
            (4, 40.6892, -74.0445),
        ];
        let world = View {
            lat: 45.,
            lon: -30.,
            zoom: 2.,
            width: 900.,
            height: 600.,
        };
        let c = cluster(&pts, &world, 48.);
        assert_eq!(c[0].ids.len(), 3, "Paris together at world zoom");
        assert_eq!(c.iter().map(|c| c.ids.len()).sum::<usize>(), 4);
        let street = View {
            lat: 48.8595,
            lon: 2.316,
            zoom: 13.,
            width: 900.,
            height: 600.,
        };
        let c = cluster(&pts, &street, 48.);
        assert_eq!(
            c.len(),
            2,
            "Eiffel pair and Louvre apart, New York off screen"
        );
        let (lat, lon, zoom) = fit(
            &pts.iter().map(|p| (p.1, p.2)).collect::<Vec<_>>(),
            900.,
            600.,
            40.,
        )
        .unwrap();
        let v = View {
            lat,
            lon,
            zoom,
            width: 900.,
            height: 600.,
        };
        for p in &pts {
            let (x, y) = v.to_screen(p.1, p.2);
            assert!(
                x >= 39. && x <= 861. && y >= 39. && y <= 561.,
                "{p:?} at {x},{y}"
            );
        }
        let single = fit(&[(48.8584, 2.2945)], 900., 600., 40.).unwrap();
        assert!(single.2 <= 15.);
        let pl = places(&pts, 0.5, 10);
        assert_eq!((pl[0].count, pl.len()), (3, 2));
        assert!(pl[0].label.contains("48.8") && pl[0].label.contains("E"));
    }
}
