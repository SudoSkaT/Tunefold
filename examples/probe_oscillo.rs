use std::error::Error;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tunefold::analysis::WaveformEnvelope;
use tunefold::visualization::engine::{SceneState, VisualState, WaveformView};
use tunefold::visualization::palette::VisualPalette;
use tunefold::visualization::render::render;
use tunefold::visualization::VISUAL_BARS;

fn punchy_view() -> WaveformView {
    let mut samples = Vec::with_capacity(2048);
    for i in 0..2048 {
        let seg = (i / 256) as usize;
        let amp = match seg {
            0 | 4 => 0.85,
            1 => 0.3,
            2 | 6 => 0.65,
            _ => 0.15,
        };
        let v = amp * ((i as f32 / 55.0).sin() + 0.4 * (i as f32 / 11.0).sin());
        samples.push(v);
    }
    let env = WaveformEnvelope::from_window(&samples);
    WaveformView {
        min: env.min,
        max: env.max,
        gain: 1.0,
    }
}

fn state(palette: VisualPalette) -> VisualState {
    let mut bars = [0.0f32; VISUAL_BARS];
    for (i, b) in bars.iter_mut().enumerate() {
        *b = 0.85 * (1.0 - i as f32 / VISUAL_BARS as f32).max(0.2);
    }
    VisualState {
        bars,
        level: 0.8,
        intensity: 0.7,
        pulse: 0.4,
        phase: 0.33,
        active: true,
        scene: SceneState {
            waveform: punchy_view(),
            energy: 0.7,
            brightness: 0.4,
            palette,
            active: true,
        },
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let palette = VisualPalette::fallback();
    for (w, h) in [(120, 40), (100, 30), (80, 24), (70, 20), (60, 15)] {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render(f, f.area(), &state(palette), 42.0)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        std::fs::write(format!("/tmp/oscillo_{w}x{h}.txt"), &out)?;
    }
    println!("dumped /tmp/oscillo_*");
    Ok(())
}