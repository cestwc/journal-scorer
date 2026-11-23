use mupdf::{Quad, Rect};
use scorer_core::BBox;

pub fn quad_bounds(q: &Quad) -> BBox {
    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
    BBox {
        x0: xs.iter().copied().fold(f32::INFINITY, f32::min),
        y0: ys.iter().copied().fold(f32::INFINITY, f32::min),
        x1: xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        y1: ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    }
}

pub fn union(a: BBox, b: BBox) -> BBox {
    BBox { x0: a.x0.min(b.x0), y0: a.y0.min(b.y0), x1: a.x1.max(b.x1), y1: a.y1.max(b.y1) }
}

pub fn to_rect(b: BBox) -> Rect {
    Rect { x0: b.x0, y0: b.y0, x1: b.x1, y1: b.y1 }
}

pub fn from_rect(r: Rect) -> BBox {
    BBox { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 }
}

pub fn to_quad(b: BBox) -> Quad {
    use mupdf::Point;
    Quad {
        ul: Point { x: b.x0, y: b.y0 },
        ur: Point { x: b.x1, y: b.y0 },
        ll: Point { x: b.x0, y: b.y1 },
        lr: Point { x: b.x1, y: b.y1 },
    }
}

pub fn intersect(a: BBox, b: BBox) -> Option<BBox> {
    let r = BBox { x0: a.x0.max(b.x0), y0: a.y0.max(b.y0), x1: a.x1.min(b.x1), y1: a.y1.min(b.y1) };
    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
}
