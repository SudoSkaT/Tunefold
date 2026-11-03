//! Suavizado de ganancia compartido por los procesadores.

pub(crate) fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

pub(crate) struct GainSmoother {
    current: f32,
    target: f32,
    coeff: f32,
    initialized: bool,
}

impl GainSmoother {
    pub(crate) fn new() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
            coeff: 0.0,
            initialized: false,
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: u32, time_ms: f32) {
        let samples = (sample_rate as f32 * time_ms / 1_000.0).max(1.0);
        self.coeff = (-1.0 / samples).exp();
        self.reset();
    }

    pub(crate) fn set_target(&mut self, target: f32) {
        self.target = target;
        if !self.initialized {
            self.current = target;
            self.initialized = true;
        }
    }

    pub(crate) fn next(&mut self) -> f32 {
        self.current = self.target + self.coeff * (self.current - self.target);
        self.current
    }

    pub(crate) fn reset(&mut self) {
        self.current = 1.0;
        self.target = 1.0;
        self.initialized = false;
    }
}
