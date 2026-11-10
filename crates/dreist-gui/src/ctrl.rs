//! Control tree loaded from `.skin` files
//! ([re:3cfb1f]) and edge anchoring ([re:00273a]).

use std::collections::HashMap;

use dreist_formats::Node;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    /// [re:6b4e94]: "x, y, w, h".
    pub fn parse(s: &str) -> Rect {
        let mut v = s.split(',').map(|t| t.trim().parse::<i32>().unwrap_or(0));
        Rect { x: v.next().unwrap_or(0), y: v.next().unwrap_or(0), w: v.next().unwrap_or(0), h: v.next().unwrap_or(0) }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// The two share at least one pixel.
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

/// Axis resize mode ([re:f8e582]); the skins
/// use the short forms FNear/FFar/LNear/LFar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    #[default]
    None,
    FixedNear,
    FixedFar,
    LockedNear,
    LockedFar,
    Relative,
    Center,
    Opposite,
}

impl Anchor {
    fn parse(s: &str) -> (Anchor, f32) {
        let mut it = s.split_whitespace();
        let mode = match it.next().unwrap_or("") {
            "FixedNear" | "FNear" => Anchor::FixedNear,
            "FixedFar" | "FFar" => Anchor::FixedFar,
            "LockedNear" | "LNear" => Anchor::LockedNear,
            "LockedFar" | "LFar" => Anchor::LockedFar,
            "Relative" => Anchor::Relative,
            "Center" => Anchor::Center,
            "Opposite" => Anchor::Opposite,
            _ => Anchor::None,
        };
        (mode, it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0))
    }
}

/// Anchors that keep a control filling its parent (`FNear 0` on all edges).
pub fn anchors_fill() -> [(Anchor, f32); 4] {
    [(Anchor::FixedNear, 0.0); 4]
}

/// Rounding with a {0.5, −0.5} table: half away from zero.
fn rnd(v: f32) -> i32 {
    (v + if v < 0.0 { -0.5 } else { 0.5 }) as i32
}

#[derive(Clone, Debug, Default)]
pub struct Ctrl {
    /// Element class ([re:713b29], [re:371681], ...).
    pub class: String,
    pub name: String,
    pub overload: String,
    pub display_name: String,
    pub style: String,
    pub text: String,
    pub id: i32,
    pub rect: Rect,
    pub zorder: i16,
    pub visible: bool,
    pub enabled: bool,
    /// Ignored by hit testing (display-only children of custom controls).
    pub passive: bool,
    /// Left, top, right, bottom anchors.
    pub anchors: [(Anchor, f32); 4],
    pub attrs: Attrs,
    pub children: Vec<Ctrl>,
    // Runtime state.
    pub value: f32,
    pub on: bool,
    pub hover: bool,
    /// [re:d45123] channels (see `meter.rs`).
    pub meters: Vec<crate::meter::MeterInfo>,
    /// Where the anchors last placed this control from (see
    /// [`Ctrl::auto_resize_children`]).
    pub anchored: Option<Anchored>,
}

/// An anchored control's layout basis: the rect it had with its parent at
/// `parent` (its skin rect, or where code last put it) and the rect the
/// anchors made of it since.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchored {
    pub base: Rect,
    pub parent: (i32, i32),
    pub placed: Rect,
}

/// A control's skin attributes. Reads go through `Deref`; any mutable
/// access forgets the cached [`Attrs::hash`].
#[derive(Clone, Debug, Default)]
pub struct Attrs {
    map: HashMap<String, String>,
    hash: std::cell::Cell<Option<u64>>,
}

impl Attrs {
    /// Order-independent hash of every key and value (cached).
    pub fn hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        if let Some(h) = self.hash.get() {
            return h;
        }
        let h = self.map.iter().fold(0u64, |acc, kv| {
            let mut e = FxHasher::default();
            kv.hash(&mut e);
            acc.wrapping_add(e.finish())
        });
        self.hash.set(Some(h));
        h
    }
}

impl std::ops::Deref for Attrs {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl std::ops::DerefMut for Attrs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.hash.set(None);
        &mut self.map
    }
}

impl FromIterator<(String, String)> for Attrs {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(it: I) -> Attrs {
        Attrs { map: it.into_iter().collect(), hash: Default::default() }
    }
}

/// A cheap word-at-a-time hasher (FxHash mixing) for [`Ctrl::draw_hash`]:
/// short fields, run over the whole tree on every sync.
pub(crate) struct FxHasher(u64);

impl Default for FxHasher {
    fn default() -> FxHasher {
        FxHasher(0xcbf2_9ce4_8422_2325)
    }
}

impl FxHasher {
    pub(crate) fn mix(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.mix(u64::from_le_bytes(*w));
        }
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.mix(u64::from_le_bytes(w));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.mix(i as u64);
    }

    fn write_u16(&mut self, i: u16) {
        self.mix(i as u64);
    }

    fn write_u32(&mut self, i: u32) {
        self.mix(i as u64);
    }

    fn write_u64(&mut self, i: u64) {
        self.mix(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.mix(i as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl Ctrl {
    /// A hash of what changes how this control itself draws (children
    /// excluded but counted; attributes in any order). Fixed keys: equal
    /// states hash equally across runs.
    pub fn draw_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = FxHasher::default();
        (&self.class, &self.name, &self.overload, &self.style, &self.text, self.id).hash(&mut h);
        (self.rect.x, self.rect.y, self.rect.w, self.rect.h, self.zorder, self.visible, self.enabled, self.on, self.hover, self.value.to_bits(), self.children.len()).hash(&mut h);
        self.attrs.hash().hash(&mut h);
        for m in &self.meters {
            (m.value.to_bits(), m.level.to_bits(), m.peak.to_bits(), m.clip, m.hold).hash(&mut h);
        }
        h.finish()
    }

    pub fn from_node(n: &Node) -> Ctrl {
        let a = |k: &str| n.attr(k).unwrap_or("").to_string();
        let mut c = Ctrl {
            class: n.class.clone(),
            name: n.name.clone().unwrap_or_default(),
            overload: a("overloadClass"),
            display_name: a("displayName"),
            style: a("style"),
            text: a("text"),
            id: n.attr("id").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            rect: n.attr("rect").map(Rect::parse).unwrap_or_default(),
            zorder: n.attr("zorder").and_then(|v| v.trim().parse().ok()).unwrap_or(1),
            visible: true,
            enabled: true,
            ..Ctrl::default()
        };
        for (i, k) in ["ARLeft", "ARTop", "ARRight", "ARBottom"].iter().enumerate() {
            c.anchors[i] = n.attr(k).map(Anchor::parse).unwrap_or((Anchor::None, -1.0));
        }
        c.attrs = n.attrs.iter().cloned().collect();
        c.children = n.children.iter().filter(|ch| !ch.class.ends_with("Style") && ch.class.starts_with("Ctrl")).map(Ctrl::from_node).collect();
        c
    }

    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }

    /// Set attribute `k` (untouched when it already holds `v`, so the
    /// cached attribute hash survives per-sync updates).
    pub fn set_attr(&mut self, k: &str, v: String) {
        if self.attrs.get(k) != Some(&v) {
            self.attrs.insert(k.into(), v);
        }
    }

    /// Remove attribute `k` (untouched when absent).
    pub fn remove_attr(&mut self, k: &str) {
        if self.attrs.contains_key(k) {
            self.attrs.remove(k);
        }
    }

    pub fn attr_f32(&self, k: &str, default: f32) -> f32 {
        self.attr(k).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    pub fn attr_i32(&self, k: &str, default: i32) -> i32 {
        self.attr(k).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    pub fn attr_bool(&self, k: &str, default: bool) -> bool {
        self.attr(k).map(|v| v.trim() == "true" || v.trim() == "1").unwrap_or(default)
    }

    /// Depth-first search by `name`.
    pub fn find(&self, name: &str) -> Option<&Ctrl> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Ctrl> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(name))
    }

    /// Path (child indices from `self`) of the first descendant named
    /// `name`, depth first.
    pub fn path_of(&self, name: &str) -> Option<Vec<usize>> {
        fn rec(c: &Ctrl, name: &str, path: &mut Vec<usize>) -> bool {
            for (i, ch) in c.children.iter().enumerate() {
                path.push(i);
                if ch.name == name || rec(ch, name, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut p = Vec::new();
        rec(self, name, &mut p).then_some(p)
    }

    /// Visit every descendant (not `self`), parents before their children.
    pub fn walk_mut(&mut self, f: &mut dyn FnMut(&mut Ctrl)) {
        for k in &mut self.children {
            f(k);
            k.walk_mut(f);
        }
    }

    /// Depth-first search by `overloadClass`.
    pub fn find_overload_mut(&mut self, class: &str) -> Option<&mut Ctrl> {
        if self.overload == class {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_overload_mut(class))
    }

    /// Set the rect, then auto-resize the children (old, new).
    pub fn set_rect(&mut self, r: Rect) {
        let old = self.rect;
        self.rect = r;
        if self.class == "CtrlSplitterPanel" {
            // [re:1edcc1]: a resize keeps the far panel (or the bar), and
            // the bar is clamped to the panels' minimum / maximum sizes
            // whenever there is room (no room at all: left).
            let len = if crate::splitter::vertical(self) { r.h } else { r.w };
            if old.w > 0 && old.h > 0 && (old.w, old.h) != (r.w, r.h) && len > crate::splitter::dim(self) {
                crate::splitter::follow_resize(self, old);
            } else if len > crate::splitter::dim(self) {
                let p = crate::splitter::clamp(self, crate::splitter::position(self));
                if p != crate::splitter::position(self) {
                    self.set_attr("intSplitPosition", p.to_string());
                }
            }
            self.layout_splitter();
        } else if (old.w, old.h) != (r.w, r.h) {
            self.auto_resize_children(old, r);
        }
    }

    /// [re:e58107]: panels `SplitterPanel0/1` either side of the
    /// splitter at `intSplitPosition` (`optIsVertical`, default true,
    /// stacks them top/bottom), `intSplitterDim` apart.
    pub fn layout_splitter(&mut self) {
        let pos = self.attr_i32("intSplitPosition", 0);
        let dim = self.attr_i32("intSplitterDim", 5);
        let vertical = self.attr_bool("optIsVertical", true);
        let (w, h) = (self.rect.w, self.rect.h);
        for c in &mut self.children {
            let r = match (c.display_name.as_str(), vertical) {
                ("SplitterPanel0", true) => Rect::new(0, 0, w, pos),
                ("SplitterPanel1", true) => Rect::new(0, pos + dim, w, (h - pos - dim).max(0)),
                ("SplitterPanel0", false) => Rect::new(0, 0, pos, h),
                ("SplitterPanel1", false) => Rect::new(pos + dim, 0, (w - pos - dim).max(0), h),
                _ => continue,
            };
            c.set_rect(r);
        }
    }

    /// Edge anchoring of the children [re:00273a].
    ///
    /// Each child is placed from its basis (its rect at a known parent
    /// size: the skin's, or wherever code last put it), not from its
    /// current rect, so a resize gives the same layout whatever sizes the
    /// parent went through before: a parent squeezed below its children's
    /// sizes (whose rects then clamp to nothing) or shrunk and grown back
    /// step by step comes back exactly as one resized there at once.
    pub fn auto_resize_children(&mut self, old: Rect, new: Rect) {
        for c in &mut self.children {
            let [(lm, lv), (tm, tv), (rm, rv), (bm, bv)] = c.anchors;
            if [lm, tm, rm, bm].iter().all(|m| *m == Anchor::None) {
                continue;
            }
            // Moved by code since the anchors last placed it (or never
            // placed): that rect, at the parent's old size, is the basis.
            let basis = match c.anchored {
                Some(a) if a.placed == c.rect => a,
                _ if old.w == 0 && old.h == 0 => continue,
                _ => Anchored { base: c.rect, parent: (old.w, old.h), placed: c.rect },
            };
            let b = basis.base;
            let (y, h) = anchor_axis(b.y, b.h, (tm, tv), (bm, bv), basis.parent.1, new.h);
            let (x, w) = anchor_axis(b.x, b.w, (lm, lv), (rm, rv), basis.parent.0, new.w);
            let r = Rect { x, y, w, h };
            c.anchored = Some(Anchored { placed: r, ..basis });
            c.set_rect(r);
        }
    }
}

/// One axis of the children's auto-resize: `near` is left/top, `far` right/bottom;
/// `parent_old`/`parent_new` the parent extent before and after.
fn anchor_axis(pos: i32, size: i32, near: (Anchor, f32), far: (Anchor, f32), parent_old: i32, parent_new: i32) -> (i32, i32) {
    let (nm, nv) = near;
    let (fm, fv) = far;
    let mut p = pos;
    let mut s = size;
    match nm {
        Anchor::FixedNear => p = rnd(nv),
        Anchor::FixedFar => p = parent_new - rnd(nv),
        Anchor::LockedFar => p = size + pos + parent_new - parent_old,
        Anchor::Relative => p = rnd(parent_new as f32 * nv),
        Anchor::Center => p = parent_new / 2 - size / 2,
        _ => {}
    }
    if fm == Anchor::Opposite {
        s = (fv as i32).max(0);
    }
    if fm == Anchor::Center {
        p = parent_new / 2 - size / 2;
    } else if fm != Anchor::Opposite {
        let mut end = s + p;
        match fm {
            Anchor::FixedNear => end = parent_new - rnd(fv),
            Anchor::FixedFar => end = rnd(fv) + p,
            Anchor::LockedNear => end = end - parent_old + parent_new,
            Anchor::LockedFar => end = p - parent_old + parent_new,
            Anchor::Relative => end = rnd(parent_new as f32 * fv),
            _ => {}
        }
        s = (end - p).max(0);
    }
    if nm == Anchor::Opposite {
        p = s + p - nv as i32;
    }
    (p, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cached attribute hash follows every change made through the
    /// map, in any order, and survives writes of the same value.
    #[test]
    fn draw_hash_follows_attributes() {
        let mut c = Ctrl::default();
        let h0 = c.draw_hash();
        c.set_attr("a", "1".into());
        let h1 = c.draw_hash();
        assert_ne!(h0, h1);
        c.set_attr("a", "1".into());
        assert!(c.attrs.hash.get().is_some(), "an unchanged value keeps the cache");
        c.attrs.insert("a".into(), "2".into());
        assert_ne!(c.draw_hash(), h1);
        c.remove_attr("a");
        assert_eq!(c.draw_hash(), h0);
        let mut d = Ctrl::default();
        d.set_attr("x", "1".into());
        d.set_attr("y", "2".into());
        let mut e = Ctrl::default();
        e.set_attr("y", "2".into());
        e.set_attr("x", "1".into());
        assert_eq!(d.draw_hash(), e.draw_hash());
    }

    #[test]
    fn anchoring_keeps_margins() {
        // ARLeft/ARRight FNear 5 in a parent growing from 100 to 200 wide.
        assert_eq!(anchor_axis(5, 90, (Anchor::FixedNear, 5.0), (Anchor::FixedNear, 5.0), 100, 200), (5, 190));
        // LNear on the far edge: the far edge follows the parent's (stretch).
        assert_eq!(anchor_axis(60, 30, (Anchor::None, -1.0), (Anchor::LockedNear, 0.0), 100, 200), (60, 130));
        assert_eq!(anchor_axis(10, 20, (Anchor::Center, 0.0), (Anchor::None, -1.0), 100, 200), (90, 20));
    }

    /// A panel with a fixed-width left column, a stretching body, a
    /// centred toolbar and a nested stretching child.
    fn panel() -> Ctrl {
        let a = |l, t, r, b| [l, t, r, b];
        let kid = |name: &str, rect, anchors, children| Ctrl { name: name.into(), rect, anchors, children, visible: true, ..Ctrl::default() };
        let inner = kid("inner", Rect::new(4, 4, 312, 192), a((Anchor::FixedNear, 4.0), (Anchor::FixedNear, 4.0), (Anchor::FixedNear, 4.0), (Anchor::FixedNear, 4.0)), vec![]);
        kid(
            "root",
            Rect::new(0, 0, 400, 300),
            Default::default(),
            vec![
                kid("column", Rect::new(0, 30, 80, 270), a((Anchor::FixedNear, 0.0), (Anchor::FixedNear, 30.0), (Anchor::FixedFar, 80.0), (Anchor::FixedNear, 0.0)), vec![]),
                kid("body", Rect::new(80, 30, 320, 200), a((Anchor::FixedNear, 80.0), (Anchor::FixedNear, 30.0), (Anchor::FixedNear, 0.0), (Anchor::LockedNear, 0.0)), vec![inner]),
                kid("tools", Rect::new(150, 5, 100, 20), a((Anchor::Center, 0.0), (Anchor::None, -1.0), (Anchor::None, -1.0), (Anchor::None, -1.0)), vec![]),
                kid("corner", Rect::new(380, 280, 15, 15), a((Anchor::FixedFar, 20.0), (Anchor::FixedFar, 20.0), (Anchor::FixedFar, 15.0), (Anchor::FixedFar, 15.0)), vec![]),
            ],
        )
    }

    fn rects(c: &Ctrl, out: &mut Vec<(String, Rect)>) {
        for k in &c.children {
            out.push((k.name.clone(), k.rect));
            rects(k, out);
        }
    }

    /// Resized through any sizes (squeezed to nothing, shrunk and grown
    /// step by step), the layout equals one resized there at once.
    #[test]
    fn anchoring_does_not_depend_on_the_resize_path() {
        for (w, h) in [(400, 300), (520, 410), (300, 220), (1200, 900)] {
            let mut direct = panel();
            direct.set_rect(Rect::new(0, 0, w, h));
            let mut want = Vec::new();
            rects(&direct, &mut want);
            for path in [vec![(0, 0)], vec![(1, 1), (90, 40)], (0..40).map(|k| (400 - k * 7, 300 - k * 5)).collect::<Vec<_>>(), vec![(2000, 1500), (60, 50), (700, 500)]] {
                let mut c = panel();
                for (pw, ph) in path.iter().copied().chain([(w, h)]) {
                    c.set_rect(Rect::new(0, 0, pw, ph));
                }
                let mut got = Vec::new();
                rects(&c, &mut got);
                assert_eq!(got, want, "{w}x{h} after {path:?}");
            }
        }
    }

    /// A control code moved is anchored from where it was put.
    #[test]
    fn code_placed_controls_anchor_from_their_new_rect() {
        let mut c = panel();
        c.set_rect(Rect::new(0, 0, 500, 300));
        c.children[2].rect = Rect::new(200, 12, 100, 24);
        c.set_rect(Rect::new(0, 0, 600, 400));
        assert_eq!(c.children[2].rect, Rect::new(250, 12, 100, 24));
    }
}
