//! Dattorro plate: the port of MultiGrainLive's `ShapeKind::DattorroPlate` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "dattorro-plate";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Dattorro plate", &[
        ("predelayMs", "Predelay", 0.0, 250.0, 0.0, false, "ms"),
        ("bandwidth", "Bandwidth", 0.0, 1.0, 0.9995, false, ""),
        ("inputDiffusion1", "Input diffusion 1", 0.0, 0.999, 0.75, false, ""),
        ("inputDiffusion2", "Input diffusion 2", 0.0, 0.999, 0.625, false, ""),
        ("decay", "Decay", 0.0, 0.9999, 0.5, false, ""),
        ("decayDiffusion1", "Decay diffusion 1", 0.0, 0.999, 0.7, false, ""),
        ("decayDiffusion2", "Decay diffusion 2", 0.0, 0.999, 0.5, false, ""),
        ("damping", "Damping", 0.0, 1.0, 0.0005, false, ""),
        ("modRateHz", "Mod rate", 0.01, 5.0, 0.3, true, "Hz"),
        ("modDepth", "Mod depth", 0.0, 32.0, 8.0, false, "smp"),
        ("wet", "Wet", 0.0, 1.0, 0.35, false, ""),
        ("dry", "Dry", 0.0, 1.0, 1.0, false, ""),
        ("crossfeed", "Width", 0.0, 1.0, 0.0, false, ""),
    ]);
}
