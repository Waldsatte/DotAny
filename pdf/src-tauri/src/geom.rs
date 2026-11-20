//! PDF's matrices: [a b c d e f], a point as a row times the matrix, the way the format writes them.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat(pub [f32; 6]);

impl Mat {
    pub const ID: Mat = Mat([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    pub fn translate(x: f32, y: f32) -> Mat {
        Mat([1.0, 0.0, 0.0, 1.0, x, y])
    }
    /// self, then other: self × other
    pub fn then(&self, o: &Mat) -> Mat {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = o.0;
        Mat([a * g + b * i, a * h + b * j, c * g + d * i, c * h + d * j, e * g + f * i + k, e * h + f * j + l])
    }
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (x * a + y * c + e, x * b + y * d + f)
    }
    pub fn invert(&self) -> Mat {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return Mat::ID;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Mat([ia, ib, ic, id, -(e * ia + f * ic), -(e * ib + f * id)])
    }
    pub fn from(v: &[f32]) -> Mat {
        if v.len() == 6 { Mat([v[0], v[1], v[2], v[3], v[4], v[5]]) } else { Mat::ID }
    }
    /// The smallest upright box around a rectangle once it's moved: [x0, y0, x1, y1]
    pub fn bbox(&self, x: f32, y: f32, w: f32, h: f32) -> [f32; 4] {
        let pts = [self.apply(x, y), self.apply(x + w, y), self.apply(x, y + h), self.apply(x + w, y + h)];
        let xs = pts.iter().map(|p| p.0);
        let ys = pts.iter().map(|p| p.1);
        [
            xs.clone().fold(f32::MAX, f32::min),
            ys.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
            ys.fold(f32::MIN, f32::max),
        ]
    }
    /// How much lengths grow, on average
    pub fn scale(&self) -> f32 {
        let [a, b, c, d, _, _] = self.0;
        ((a * d - b * c).abs()).sqrt()
    }
}

pub fn fmt(v: f32) -> String {
    if (v - v.round()).abs() < 0.0005 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// "#RRGGBB" as three numbers from 0 to 1
pub fn rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32 / 255.0;
    [p(0), p(2), p(4)]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrices_compose_and_invert() {
        let m = Mat([0.0, 1.0, -1.0, 0.0, 10.0, 20.0]);
        let back = m.invert();
        let (x, y) = m.apply(3.0, 4.0);
        let (a, b) = back.apply(x, y);
        assert!((a - 3.0).abs() < 1e-4 && (b - 4.0).abs() < 1e-4);
        let t = Mat::translate(5.0, 0.0).then(&Mat([2.0, 0.0, 0.0, 2.0, 1.0, 1.0]));
        assert_eq!(t.apply(0.0, 0.0), (11.0, 1.0));
        assert_eq!(fmt(1.5), "1.5");
        assert_eq!(fmt(2.0), "2");
    }
}
