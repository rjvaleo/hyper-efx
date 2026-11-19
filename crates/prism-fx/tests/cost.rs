//! Worst-block benchmark for every effect in `prism_fx::CATALOG`
//! (AUDIO-PLAN §3, stage 1 task 1.1; task 004).
//!
//! Run it alone in a release build:
//!
//!     cargo test --release -p prism-fx --test cost -- --ignored --nocapture
//!
//! Writes JSON to `BENCH_OUT` (default `target/effects-worst-block.json`) in
//! the same schema as MGL's `engine/tests/worst_block.rs`, so
//! `MultiGrainLive/tools/bench-compare.mjs` compares runs unchanged.

use prism_fx::{contract, ParamKind, CATALOG};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SR: f32 = 48_000.0;
const SR_USIZE: usize = 48_000;
const SECONDS: usize = 10;
const TOTAL_FRAMES: usize = SR_USIZE * SECONDS;
const WARMUP_BLOCKS: usize = 200;
const BLOCKS: [usize; 3] = [64, 128, 512];

#[cfg(unix)]
fn cpu_now() -> f64 {
    #[repr(C)]
    struct Timespec {
        tv_sec: isize,
        tv_nsec: isize,
    }
    extern "C" {
        fn clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "macos")]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 16;
    #[cfg(not(target_os = "macos"))]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;

    let mut t = Timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `t` is a valid stack Timespec for the kernel to write.
    unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

#[cfg(not(unix))]
fn cpu_now() -> f64 {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// Stereo noise at -12 dB (peak ~0.2512) with a 100 ms silence every second,
/// so tails decaying toward silence (and denormals) are exercised too.
fn build_signal(frames: usize) -> (Vec<f32>, Vec<f32>) {
    let gain = 10.0f32.powf(-12.0 / 20.0);
    let mut seed = 0x1234_5678u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let u = ((seed >> 8) as f32) / (16_777_216.0 * 0.5) - 1.0;
        u * gain
    };
    let mut l = Vec::with_capacity(frames);
    let mut r = Vec::with_capacity(frames);
    let silent_start = SR_USIZE - SR_USIZE / 10; // last 100 ms of every 1 s
    for i in 0..frames {
        let in_sec = i % SR_USIZE;
        if in_sec >= silent_start {
            l.push(0.0);
            r.push(0.0);
            let _ = next();
            let _ = next();
        } else {
            l.push(next());
            r.push(next());
        }
    }
    (l, r)
}

fn first_continuous_param(desc: &prism_fx::Descriptor) -> Option<(usize, f32, f32, bool)> {
    for (idx, p) in desc.params.iter().enumerate() {
        if !p.inert && p.kind == ParamKind::Continuous && (p.max - p.min).abs() > 1e-6 {
            return Some((idx, p.min, p.max, p.log));
        }
    }
    None
}

fn sweep_value(min: f32, max: f32, log: bool, phase: f64) -> f32 {
    // Triangle wave 0 -> 1 -> 0 once per second.
    let frac = phase.fract();
    let t = if frac < 0.5 { frac * 2.0 } else { (1.0 - frac) * 2.0 } as f32;
    if log && min > 0.0 && max > min {
        min * (max / min).powf(t)
    } else {
        min + (max - min) * t
    }
}

struct Row {
    case: String,
    block: usize,
    median_pct: f64,
    p99_pct: f64,
    worst_pct: f64,
    over_50: usize,
    over_100: usize,
}

fn measure_case(
    id: &str,
    mode: &str,
    block: usize,
    sig_l: &[f32],
    sig_r: &[f32],
    test_sound: &([f32; 0], &[f32], usize, f32),
    moving: bool,
) -> Row {
    let _ = test_sound.0;
    let mut fx = prism_fx::make(id, SR).expect("catalog entry must build");
    let desc = fx.descriptor();
    if desc.sound.is_some() {
        fx.set_sound(test_sound.1, test_sound.2, test_sound.3);
    }
    let sweep = if moving { first_continuous_param(desc) } else { None };

    let mut bl = vec![0.0f32; block];
    let mut br = vec![0.0f32; block];
    let total_blocks = TOTAL_FRAMES / block;
    let slot_s = block as f64 / SR as f64;

    // Warm-up: skip the first 200 blocks.
    let mut pos = 0usize;
    for b in 0..WARMUP_BLOCKS {
        if let Some((idx, min, max, log)) = sweep {
            let sec = (b * block) as f64 / SR as f64;
            fx.set_param(idx, sweep_value(min, max, log, sec));
        }
        for i in 0..block {
            let s = (pos + i) % sig_l.len();
            bl[i] = sig_l[s];
            br[i] = sig_r[s];
        }
        fx.process(&mut bl, &mut br);
        pos = (pos + block) % sig_l.len();
    }

    let mut pcts = Vec::with_capacity(total_blocks);
    let mut over_50 = 0usize;
    let mut over_100 = 0usize;
    for b in 0..total_blocks {
        if let Some((idx, min, max, log)) = sweep {
            let sec = ((WARMUP_BLOCKS + b) * block) as f64 / SR as f64;
            fx.set_param(idx, sweep_value(min, max, log, sec));
        }
        for i in 0..block {
            let s = (pos + i) % sig_l.len();
            bl[i] = sig_l[s];
            br[i] = sig_r[s];
        }
        let t0 = cpu_now();
        fx.process(&mut bl, &mut br);
        let dt = cpu_now() - t0;
        let pct = 100.0 * dt / slot_s;
        if pct > 50.0 {
            over_50 += 1;
        }
        if pct > 100.0 {
            over_100 += 1;
        }
        pcts.push(pct);
        pos = (pos + block) % sig_l.len();
    }

    pcts.sort_by(f64::total_cmp);
    let n = pcts.len();
    let median_pct = pcts[(n - 1) / 2];
    let p99_pct = pcts[((n - 1) as f64 * 0.99) as usize];
    let worst_pct = pcts[n - 1];

    Row {
        case: format!("{id}/{mode}"),
        block,
        median_pct,
        p99_pct,
        worst_pct,
        over_50,
        over_100,
    }
}

fn cmd_out(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| cmd_out("hostname", &[]))
        .unwrap_or_else(|| "unknown".into())
}

fn cpu_name() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(s) = cmd_out(
            "pwsh",
            &["-NoProfile", "-Command", "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name).Trim()"],
        ) {
            return s;
        }
        if let Ok(s) = std::env::var("PROCESSOR_IDENTIFIER") {
            if !s.trim().is_empty() {
                return s;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(s) = cmd_out("sysctl", &["-n", "machdep.cpu.brand_string"]) {
            return s;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = fs::read_to_string("/proc/cpuinfo") {
            if let Some(line) = text.lines().find(|l| l.starts_with("model name")) {
                if let Some((_, v)) = line.split_once(':') {
                    return v.trim().to_string();
                }
            }
        }
    }
    "unknown".into()
}

fn git_commit() -> String {
    cmd_out("git", &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into())
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn write_json(path: &PathBuf, machine: &str, cpu: &str, commit: &str, rows: &[Row]) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).expect("create BENCH_OUT parent dir");
        }
    }
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"machine\": \"{}\",\n", json_escape(machine)));
    s.push_str(&format!("  \"cpu\": \"{}\",\n", json_escape(cpu)));
    s.push_str(&format!("  \"commit\": \"{}\",\n", json_escape(commit)));
    s.push_str("  \"rows\": [\n");
    for (i, r) in rows.iter().enumerate() {
        let comma = if i + 1 < rows.len() { "," } else { "" };
        s.push_str(&format!(
            "    {{\"case\": \"{}\", \"block\": {}, \"median_pct\": {:.4}, \"p99_pct\": {:.4}, \"worst_pct\": {:.4}, \"over_50\": {}, \"over_100\": {}}}{}\n",
            json_escape(&r.case),
            r.block,
            r.median_pct,
            r.p99_pct,
            r.worst_pct,
            r.over_50,
            r.over_100,
            comma
        ));
    }
    s.push_str("  ]\n}\n");
    fs::write(path, s).expect("write BENCH_OUT JSON");
}

#[test]
#[ignore = "a measurement: run it alone, in a release build"]
fn effects_worst_block_bench() {
    let t_start = std::time::Instant::now();
    let (sig_l, sig_r) = build_signal(TOTAL_FRAMES);
    let (sound_samples, sound_ch, sound_sr) = contract::test_sound();
    let test_sound = ([], sound_samples.as_slice(), sound_ch, sound_sr);

    let mut rows = Vec::with_capacity(CATALOG.len() * BLOCKS.len() * 2);
    for entry in CATALOG {
        let id = entry.descriptor.id;
        for &block in &BLOCKS {
            rows.push(measure_case(id, "steady", block, &sig_l, &sig_r, &test_sound, false));
            rows.push(measure_case(id, "moving", block, &sig_l, &sig_r, &test_sound, true));
        }
    }

    let machine = machine_name();
    let cpu = cpu_name();
    let commit = git_commit();
    let out_path = std::env::var("BENCH_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target").join("effects-worst-block.json"));
    write_json(&out_path, &machine, &cpu, &commit, &rows);

    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| b.worst_pct.total_cmp(&a.worst_pct));

    println!(
        "\nEffects worst-block bench: {} effects, {} rows, elapsed {:.1}s ({machine} / {cpu} / {commit})",
        CATALOG.len(),
        rows.len(),
        t_start.elapsed().as_secs_f64()
    );
    println!("JSON written to {}", out_path.display());
    println!(
        "{:<36} {:>5} {:>9} {:>9} {:>9} {:>7} {:>8}",
        "case", "block", "med %", "p99 %", "worst %", ">50%", ">100%"
    );
    for r in &sorted {
        println!(
            "{:<36} {:>5} {:>8.2}% {:>8.2}% {:>8.2}% {:>7} {:>8}",
            r.case, r.block, r.median_pct, r.p99_pct, r.worst_pct, r.over_50, r.over_100
        );
    }
}
