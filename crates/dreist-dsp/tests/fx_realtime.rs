//! Effects on the audio thread: once built (`Effect::new`, which prepares
//! the node for the rate), processing must not touch the heap from the very
//! first block, including parameter sweeps (mode switches), large host
//! blocks and panics (`Effect::panic`).

use dreist_dsp::fx::{Ctx, Effect, nodes};
use dreist_testkit::CountingAlloc;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn effects_do_not_touch_the_heap_after_construction() {
    let mut failures = Vec::new();
    for kind in (1..=47).filter(|&k| nodes::is_ported(k)) {
        for sr in [44_100.0f32, 96_000.0] {
            let mut fx = Effect::new(kind, sr);
            fx.mix = 1.0;
            let ctx = Ctx::new(sr);
            let n = 4096;
            let inp: Vec<f32> = (0..n).map(|i| ((i * 7919) % 1000) as f32 / 1000.0 - 0.5).collect();
            let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
            let ((), calls) = CountingAlloc::count(|| {
                for step in 0..48 {
                    // Sweep every parameter through its range (enum
                    // parameters switch modes), a panic every 16 blocks.
                    for i in 0..12 {
                        fx.push_parameter(i, ((step * (i + 3)) % 17) as f32 / 16.0);
                    }
                    if step % 16 == 15 {
                        fx.panic();
                    }
                    let len = [32usize, 7, 4096, 512][step % 4];
                    fx.process(&ctx, [&inp[..len], &inp[..len]], [&mut l[..len], &mut r[..len]]);
                }
            });
            if calls > 0 {
                failures.push(format!("kind {kind} @{sr}: {calls} allocator calls"));
            }
        }
    }
    let sites: Vec<String> = CountingAlloc::sites().iter().take(30).map(|(s, n)| format!("{n:6}  {s}")).collect();
    assert!(failures.is_empty(), "{}\n(ALLOC_TRACE=1 for call sites)\n{}", failures.join("\n"), sites.join("\n"));
}
