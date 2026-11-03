//! Engine validation harness. The demo presets (skipped when the factory
//! content is absent; `DREIST_CONTENT` overrides its location) are rendered
//! as they are and as stress scenarios that rewrite every layer and step
//! (filters at high resonance, loops, pitch and time-stretch extremes,
//! random step lanes, extreme envelopes, tiny sample regions). Every render
//! must stay finite and below a blow-up level, and the 44.1 kHz renders must
//! match the stored fingerprints (`tests/golden/engine.txt`;
//! `UPDATE_GOLDEN=1` rewrites them) so refactors are checked bit for bit.

use dreist_engine::harness::{self as common, doc_for, name, presets, render, sane, scenarios};

#[test]
fn scenarios_render_sane_and_match_fingerprints() {
    dreist_engine::harness::check_scenarios("engine");
}

#[test]
fn high_sample_rates_stay_sane() {
    let mut failures = Vec::new();
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        for (sname, f) in scenarios() {
            let doc = doc_for(&p, f);
            for sr in [48_000.0f32, 96_000.0, 192_000.0] {
                let (l, r) = render(&doc, &samples, sr, 512, 3.0);
                failures.extend(sane(&format!("{}-{sname}@{sr}", name(&p)), &l, &r));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn host_block_sizes_stay_sane_and_close() {
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        // Without step randomisation: the random draws interleave with
        // control-rate work differently per block size.
        let doc = doc_for(&p, |d| {
            common::walk(d, &mut |n| {
                if n.class == "Track" {
                    n.attrs.retain(|(k, _)| !k.ends_with("Rand"));
                }
            })
        });
        let (a, _) = render(&doc, &samples, 44_100.0, 512, 3.0);
        let energy = a.iter().map(|x| x * x).sum::<f32>();
        for block in [1usize, 7, 32, 64, 333, 4096] {
            let (l, r) = render(&doc, &samples, 44_100.0, block, 3.0);
            if let Some(e) = sane(&format!("{} block {block}", name(&p)), &l, &r) {
                panic!("{e}");
            }
            // The engine runs ≤ 32-frame sub-blocks cut from the host block
            // start, like the original ([re:142820]): host blocks
            // that are multiples of 32 give the same voices (song cues still
            // land per host block); other sizes shift control-rate updates.
            if block % 32 == 0 {
                let err = a.iter().zip(&l).map(|(x, y)| (x - y) * (x - y)).sum::<f32>();
                let rel = (err / energy).sqrt();
                assert!(rel < 0.01, "{} block {block}: relative difference {rel}", name(&p));
            }
        }
    }
}

#[test]
fn renders_are_reproducible() {
    let mut failures = Vec::new();
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        for (sname, f) in scenarios() {
            let doc = doc_for(&p, f);
            let a = render(&doc, &samples, 44_100.0, 512, 3.0);
            let b = render(&doc, &samples, 44_100.0, 512, 3.0);
            if let Some(i) = a.0.iter().zip(&b.0).position(|(x, y)| x.to_bits() != y.to_bits()) {
                failures.push(format!("{}-{sname}: renders differ from sample {i}", name(&p)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An editor rebuild swaps in a new engine mid-play ([re:57f537]): with
/// the same document (effects removed: their tails are not carried) the
/// audio continues exactly as if nothing happened.
#[test]
fn rebuild_mid_play_continues_seamlessly() {
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        let doc = doc_for(&p, |d| common::walk(d, &mut |n| n.children.retain(|c| c.class != "EffectChain")));
        let (block, sr) = (512, 44_100.0);
        let blocks = (sr as usize * 4) / block;
        let run = |rebuild_at: Option<usize>| {
            let mut g = common::engine(&doc, &samples, sr);
            g.start();
            let mut out = Vec::new();
            let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
            for b in 0..blocks {
                if Some(b) == rebuild_at {
                    let mut n = common::engine(&doc, &samples, sr);
                    n.adopt(&mut g);
                    g = n;
                }
                g.process(&mut l, &mut r);
                out.extend_from_slice(&l);
            }
            out
        };
        let (a, b) = (run(None), run(Some(blocks / 2)));
        if let Some(i) = a.iter().zip(&b).position(|(x, y)| x.to_bits() != y.to_bits()) {
            panic!("{}: the rebuilt engine differs from sample {i} (rebuilt at {})", name(&p), blocks / 2 * block);
        }
    }
}

/// A rebuild to a document with fewer pads and layers (Reset, a smaller
/// kit) while voices play: the voices whose strips are gone stop, the
/// engine keeps running.
#[test]
fn rebuild_to_a_smaller_document_mid_play() {
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        let doc = doc_for(&p, |_| {});
        let mut g = common::engine(&doc, &samples, 44_100.0);
        g.start();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for pad in 0..16 {
            g.pad_hit(0, pad, 100, 0);
        }
        for _ in 0..20 {
            g.process(&mut l, &mut r);
        }
        let empty = dreist_formats::Node::parse(r#"<root version="3000"><Engine name="Engine0" /></root>"#).unwrap();
        for new_doc in [empty, dreist_formats::Node::parse(r#"<root version="3000" />"#).unwrap()] {
            let mut n = common::engine(&new_doc, &samples, 44_100.0);
            n.adopt(&mut g);
            for _ in 0..20 {
                n.process(&mut l, &mut r);
                assert!(l.iter().chain(&r).all(|v| v.is_finite()), "{}", name(&p));
            }
            g = n;
        }
    }
}
