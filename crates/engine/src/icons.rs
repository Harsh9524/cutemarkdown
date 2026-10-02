//! Small vector icons drawn with the painter (paths from Lucide, ISC license,
//! <https://lucide.dev>), stroked at 1.75 in a 24×24 view box.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Info,
    Lightbulb,
    MessageSquareWarning,
    TriangleAlert,
    OctagonAlert,
    Copy,
    Check,
    ArrowUpRight,
    ImageOff,
}

fn svg(icon: Icon) -> &'static [&'static str] {
    match icon {
        Icon::Info => &[
            "M22 12a10 10 0 1 1-20 0a10 10 0 1 1 20 0z",
            "M12 16v-4",
            "M12 8h.01",
        ],
        Icon::Lightbulb => &[
            "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5",
            "M9 18h6",
            "M10 22h4",
        ],
        Icon::MessageSquareWarning => &[
            "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
            "M12 7v2",
            "M12 13h.01",
        ],
        Icon::TriangleAlert => &[
            "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
            "M12 9v4",
            "M12 17h.01",
        ],
        Icon::OctagonAlert => &[
            "M12 16h.01",
            "M12 8v4",
            "M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z",
        ],
        Icon::Copy => &[
            "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-10a2 2 0 0 1-2-2v-10a2 2 0 0 1 2-2z",
            "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
        ],
        Icon::Check => &["M20 6 9 17l-5-5"],
        Icon::ArrowUpRight => &["M7 7h10v10", "M7 17 17 7"],
        Icon::ImageOff => &[
            "M2 2 22 22",
            "M10.41 10.41a2 2 0 1 1-2.83-2.83",
            "M13.5 13.5 6 21",
            "M18 12 21 15",
            "M3.59 3.59A1.99 1.99 0 0 0 3 5v14a2 2 0 0 0 2 2h14c.55 0 1.052-.22 1.41-.59",
            "M21 15V5a2 2 0 0 0-2-2H9",
        ],
    }
}

/// A flattened sub-path in view-box units.
#[derive(Clone, Debug)]
struct Poly {
    points: Vec<Pos2>,
    closed: bool,
}

fn polys(icon: Icon) -> &'static [Poly] {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<Icon, &'static [Poly]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(icon).or_insert_with(|| {
        let v: Vec<Poly> = svg(icon).iter().flat_map(|d| flatten(d)).collect();
        Box::leak(v.into_boxed_slice())
    })
}

/// Paint `icon` into `rect` (the 24×24 view box is scaled to fit) with a 1.75-unit stroke
/// scaled like the icon (`stroke_px` overrides the screen width).
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, color: Color32, stroke_px: Option<f32>) {
    let s = rect.width().min(rect.height()) / 24.0;
    let origin = rect.center() - vec2(12.0 * s, 12.0 * s);
    // Lucide's 1.75 is in view-box units; keep small icons legible.
    let width = stroke_px.unwrap_or((1.75 * s).max(1.3));
    let stroke = Stroke::new(width, color);
    for p in polys(icon) {
        let pts: Vec<Pos2> = p.points.iter().map(|q| origin + q.to_vec2() * s).collect();
        if pts.len() < 2 {
            continue;
        }
        let len: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        if len < width * 0.5 {
            // Lucide "dots" (h.01): a round cap.
            painter.circle_filled(pts[0], width * 0.62, color);
            continue;
        }
        if p.closed {
            painter.add(Shape::closed_line(pts, stroke));
        } else {
            let (a, b) = (pts[0], *pts.last().unwrap_or(&pts[0]));
            painter.add(Shape::line(pts, stroke));
            // Round caps.
            painter.circle_filled(a, width * 0.5, color);
            painter.circle_filled(b, width * 0.5, color);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Minimal SVG path flattener (M L H V C S Q A Z, absolute and relative).

fn flatten(d: &str) -> Vec<Poly> {
    let toks = tokenize(d);
    let mut out = Vec::new();
    let mut cur = pos2(0.0, 0.0);
    let mut start = cur;
    let mut pts: Vec<Pos2> = Vec::new();
    let mut last_ctrl: Option<Pos2> = None;
    let mut i = 0;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> f32 {
        match toks.get(*i) {
            Some(Tok::Num(n)) => {
                *i += 1;
                *n
            }
            _ => {
                *i += 1;
                0.0
            }
        }
    };
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                if !pts.is_empty() {
                    out.push(Poly {
                        points: std::mem::take(&mut pts),
                        closed: true,
                    });
                }
                cur = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur.to_vec2() } else { vec2(0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                if pts.len() > 1 {
                    out.push(Poly {
                        points: std::mem::take(&mut pts),
                        closed: false,
                    });
                }
                pts.clear();
                let p = pos2(num(&mut i), num(&mut i)) + base;
                cur = p;
                start = p;
                pts.push(p);
                // Subsequent pairs are implicit line-tos.
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let p = pos2(num(&mut i), num(&mut i)) + base;
                pts.push(p);
                cur = p;
                last_ctrl = None;
            }
            'H' => {
                let x = num(&mut i) + base.x;
                cur = pos2(x, cur.y);
                pts.push(cur);
                last_ctrl = None;
            }
            'V' => {
                let y = num(&mut i) + base.y;
                cur = pos2(cur.x, y);
                pts.push(cur);
                last_ctrl = None;
            }
            'C' | 'S' => {
                let c1 = if cmd.eq_ignore_ascii_case(&'C') {
                    pos2(num(&mut i), num(&mut i)) + base
                } else {
                    last_ctrl.map(|c| cur + (cur - c)).unwrap_or(cur)
                };
                let c2 = pos2(num(&mut i), num(&mut i)) + base;
                let p = pos2(num(&mut i), num(&mut i)) + base;
                for k in 1..=12 {
                    let t = k as f32 / 12.0;
                    let mt = 1.0 - t;
                    let q = cur.to_vec2() * mt * mt * mt
                        + c1.to_vec2() * 3.0 * mt * mt * t
                        + c2.to_vec2() * 3.0 * mt * t * t
                        + p.to_vec2() * t * t * t;
                    pts.push(q.to_pos2());
                }
                last_ctrl = Some(c2);
                cur = p;
            }
            'Q' => {
                let c = pos2(num(&mut i), num(&mut i)) + base;
                let p = pos2(num(&mut i), num(&mut i)) + base;
                for k in 1..=10 {
                    let t = k as f32 / 10.0;
                    let mt = 1.0 - t;
                    let q =
                        cur.to_vec2() * mt * mt + c.to_vec2() * 2.0 * mt * t + p.to_vec2() * t * t;
                    pts.push(q.to_pos2());
                }
                last_ctrl = None;
                cur = p;
            }
            'A' => {
                let rx = num(&mut i);
                let ry = num(&mut i);
                let rot = num(&mut i);
                let large = num(&mut i) != 0.0;
                let sweep = num(&mut i) != 0.0;
                let p = pos2(num(&mut i), num(&mut i)) + base;
                arc(&mut pts, cur, p, rx, ry, rot, large, sweep);
                cur = p;
                last_ctrl = None;
            }
            _ => {
                i += 1;
            }
        }
    }
    if pts.len() > 1 {
        out.push(Poly {
            points: pts,
            closed: false,
        });
    }
    out
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Cmd(char),
    Num(f32),
}

fn tokenize(d: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    // Arc flags may be written without separators ("0 1 1-2.83"), but numbers are fine.
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '.' || c == '+' {
            let s = i;
            i += 1;
            let mut seen_dot = c == '.';
            while i < b.len() {
                let d = b[i] as char;
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if d == 'e' && i + 1 < b.len() {
                    i += 2;
                } else {
                    break;
                }
            }
            out.push(Tok::Num(d[s..i].parse().unwrap_or(0.0)));
        } else {
            i += 1;
        }
    }
    out
}

/// Endpoint-parameterized elliptical arc → polyline (SVG implementation notes F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc(
    pts: &mut Vec<Pos2>,
    p0: Pos2,
    p1: Pos2,
    rx: f32,
    ry: f32,
    rot_deg: f32,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 {
        pts.push(p1);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = cos_p * dx + sin_p * dy;
    let y1 = -sin_p * dx + cos_p * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cos_p * cx1 - sin_p * cy1 + (p0.x + p1.x) / 2.0;
    let cy = sin_p * cx1 + cos_p * cy1 + (p0.y + p1.y) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let steps = ((dt.abs() / std::f32::consts::TAU) * 32.0).ceil().max(2.0) as usize;
    for k in 1..=steps {
        let t = t1 + dt * k as f32 / steps as f32;
        let (st, ct) = t.sin_cos();
        let x = cos_p * rx * ct - sin_p * ry * st + cx;
        let y = sin_p * rx * ct + cos_p * ry * st + cy;
        pts.push(pos2(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_flatten() {
        for icon in [
            Icon::Info,
            Icon::Lightbulb,
            Icon::MessageSquareWarning,
            Icon::TriangleAlert,
            Icon::OctagonAlert,
            Icon::Copy,
            Icon::Check,
            Icon::ArrowUpRight,
            Icon::ImageOff,
        ] {
            let p = polys(icon);
            assert!(!p.is_empty(), "{icon:?}");
            for poly in p {
                for q in &poly.points {
                    assert!(
                        (-1.0..=25.0).contains(&q.x) && (-1.0..=25.0).contains(&q.y),
                        "{icon:?} {q:?}"
                    );
                }
            }
        }
    }
}
