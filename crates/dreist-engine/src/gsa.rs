//! `.gsa` slice-analysis sidecars ([re:3135eb],
//! [re:6f2d5b], [re:4a07b5]
//! [re:5cdff0]): `<root><SliceAnalysis name="Master" version="3000"><Slices>
//! <SlicePoint …/>…</Slices><AudioFileMetaData name="Meta" …/></SliceAnalysis>
//! <AudioFileMarkers name="StartEndPoints" …/></root>`, with an optional
//! second [re:cd08a0] (legacy `Orig` / `Curr`). The
//! "active" analysis everywhere ([re:fb32a7] User, else
//! Master) is the one these fields hold; the other is kept verbatim.

use std::path::Path;

use dreist_dsp::slice::SlicePoint;
use dreist_formats::Node;

#[derive(Debug, Clone, PartialEq)]
pub struct SliceFile {
    pub slices: Vec<SlicePoint>,
    pub bpm: f32,
    pub bars: f32,
    pub beats: f32,
    pub pitch: f32,
    /// [re:ed1f12] Name / Author / Desc (≤ 63 characters).
    pub name: String,
    pub author: String,
    pub desc: String,
    /// The fields are the `User` analysis; `master` then holds the Master
    /// one as read.
    pub user: bool,
    pub master: Option<Node>,
    /// `StartEndPoints` (sample region).
    pub start: u32,
    pub end: u32,
}

impl SliceFile {
    pub fn new(frames: u32) -> SliceFile {
        SliceFile {
            slices: Vec::new(),
            bpm: 120.0,
            bars: 1.0,
            beats: 0.0,
            pitch: 440.0,
            name: String::new(),
            author: String::new(),
            desc: String::new(),
            user: false,
            master: None,
            start: 0,
            end: frames,
        }
    }

    pub fn from_node(root: &Node) -> SliceFile {
        let mut f = SliceFile::new(0);
        let analyses: Vec<&Node> = root.children.iter().filter(|c| c.class == "SliceAnalysis").collect();
        let is = |n: &Node, names: [&str; 2]| n.name.as_deref().is_some_and(|x| names.contains(&x));
        let master = analyses.iter().find(|n| is(n, ["Master", "Orig"])).or(analyses.first()).copied();
        let user = analyses.iter().find(|n| is(n, ["User", "Curr"])).copied();
        if let (Some(m), Some(_)) = (master, user) {
            f.user = true;
            f.master = Some(m.clone());
        }
        if let Some(a) = user.or(master) {
            f.read_analysis(a);
        }
        if let Some(m) = root.children.iter().find(|c| c.class == "AudioFileMarkers" && c.name.as_deref() == Some("StartEndPoints")) {
            f.start = m.u32("StartSample").unwrap_or(0);
            f.end = m.u32("EndSample").unwrap_or(0);
        }
        f
    }

    fn read_analysis(&mut self, a: &Node) {
        fn walk(n: &Node, f: &mut SliceFile) {
            match n.class.as_str() {
                "SlicePoint" => {
                    let start = n.u32("Start").unwrap_or(0);
                    f.slices.push(SlicePoint {
                        start,
                        pos: n.attr("Pos").and_then(|v| v.parse().ok()).unwrap_or(start as i32),
                        locked: n.bool("Locked").unwrap_or(false),
                        deleted: n.bool("Deleted").unwrap_or(false),
                        strength: n.f32("Strength").unwrap_or(-50.0),
                        dyn_sense: n.f32("DynSense").unwrap_or(-50.0),
                        mean: n.f32("Mean").unwrap_or(-50.0),
                        kind: 0,
                    });
                }
                "AudioFileMetaData" => {
                    f.bpm = n.f32("BPM").unwrap_or(f.bpm);
                    f.bars = n.f32("Bars").unwrap_or(f.bars);
                    f.beats = n.f32("Beats").unwrap_or(f.beats);
                    f.pitch = n.f32("Pitch").unwrap_or(f.pitch);
                    let s = |k: &str| n.children.iter().find(|c| c.name.as_deref() == Some(k)).and_then(|c| c.attr("string")).unwrap_or("").to_string();
                    (f.name, f.author, f.desc) = (s("Name"), s("Author"), s("Desc"));
                    return;
                }
                _ => {}
            }
            n.children.iter().for_each(|c| walk(c, f));
        }
        walk(a, self);
        self.slices.sort_by_key(|s| s.start);
    }

    pub fn load(path: &Path) -> Option<SliceFile> {
        Node::parse(&std::fs::read_to_string(path).ok()?).ok().map(|n| SliceFile::from_node(&n))
    }

    pub fn to_node(&self) -> Node {
        let mut root = Node::new("root", None);
        if let (true, Some(m)) = (self.user, &self.master) {
            root.children.push(m.clone());
        }
        let mut sa = Node::new("SliceAnalysis", Some(if self.user { "User" } else { "Master" }));
        sa.version = Some(3000);
        let mut slices = Node::new("Slices", Some("Slices"));
        for (i, s) in self.slices.iter().enumerate() {
            let mut p = Node::new("SlicePoint", Some(&format!("SlicePoint{i}")));
            p.version = Some(3);
            if s.locked {
                p.set("Locked", "true");
            }
            if s.deleted {
                p.set("Deleted", "true");
            }
            p.set("DynSense", s.dyn_sense).set("Mean", s.mean).set("Pos", s.pos).set("Start", s.start).set("Strength", s.strength);
            slices.children.push(p);
        }
        sa.children.push(slices);
        let mut meta = Node::new("AudioFileMetaData", Some("Meta"));
        meta.version = Some(2100);
        meta.set("BPM", self.bpm).set("Bars", self.bars).set("Beats", self.beats).set("Pitch", self.pitch);
        for (n, v) in [("Name", &self.name), ("Author", &self.author), ("Desc", &self.desc)] {
            let mut c = Node::new("VFixedString_64", Some(n));
            c.set("string", v.chars().take(63).collect::<String>());
            meta.children.push(c);
        }
        sa.children.push(meta);
        root.children.push(sa);
        let mut m = Node::new("AudioFileMarkers", Some("StartEndPoints"));
        m.version = Some(2);
        m.set("EndSample", self.end).set("SampleLength", self.end - self.start).set("StartSample", self.start);
        root.children.push(m);
        root
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        dreist_formats::file::write_atomic(path, self.to_node().to_xml())
    }

    /// Analyze a sample ([re:c5ff4b]) into a new sidecar.
    pub fn analyze(left: &[f32], right: Option<&[f32]>) -> SliceFile {
        let mut f = SliceFile::new(left.len() as u32);
        f.slices = dreist_dsp::slice::analyze(left, right);
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_installed_sidecars() {
        let root = std::path::PathBuf::from(dreist_formats::legacy::content_dir().to_string_lossy().into_owned());
        let p = root.join(dreist_formats::legacy::DEMO_FACTORY).join("Kits/Synth Melody.samples/Synth/Drop Lead 1 D.gsa");
        let Some(a) = SliceFile::load(&p) else { return };
        assert_eq!(a.slices.len(), 7);
        let b = SliceFile::from_node(&Node::parse(&a.to_node().to_xml()).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn user_analysis_is_active_and_master_kept() {
        let xml = r#"<root><SliceAnalysis name="Master" version="3000"><Slices><SlicePoint name="SlicePoint0" Start="10"/></Slices><AudioFileMetaData name="Meta" BPM="100"/></SliceAnalysis><SliceAnalysis name="User" version="3000"><Slices><SlicePoint name="SlicePoint0" Start="20"/><SlicePoint name="SlicePoint1" Start="30"/></Slices><AudioFileMetaData name="Meta" BPM="140"><VFixedString_64 name="Author" string="me"/></AudioFileMetaData></SliceAnalysis><AudioFileMarkers name="StartEndPoints" StartSample="0" EndSample="99"/></root>"#;
        let f = SliceFile::from_node(&Node::parse(xml).unwrap());
        assert!(f.user);
        assert_eq!((f.bpm, f.slices.len(), f.author.as_str(), f.end), (140.0, 2, "me", 99));
        let back = SliceFile::from_node(&Node::parse(&f.to_node().to_xml()).unwrap());
        assert_eq!(back, f);
        assert_eq!(back.master.as_ref().and_then(|m| m.child("AudioFileMetaData", "Meta")).and_then(|m| m.f32("BPM")), Some(100.0));
    }
}
