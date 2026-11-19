/// Audio and sidechain history share one cursor so both detectors refer to
/// the same input timestamp, including when lookahead is automated.
pub struct History {
    frames: Box<[[f64; 4]]>,
    cursor: usize,
}

impl History {
    pub fn new(max_delay: usize) -> Self {
        Self {
            frames: vec![[0.0; 4]; max_delay + 2].into_boxed_slice(),
            cursor: 0,
        }
    }

    pub fn reset(&mut self) {
        self.frames.fill([0.0; 4]);
        self.cursor = 0;
    }

    pub fn push(&mut self, [left, right]: [f64; 2], [side_l, side_r]: [f64; 2]) {
        self.cursor += 1;
        if self.cursor == self.frames.len() {
            self.cursor = 0;
        }
        self.frames[self.cursor] = [left, right, side_l, side_r];
    }

    pub fn at(&self, samples: usize) -> [f64; 4] {
        let index = if samples <= self.cursor {
            self.cursor - samples
        } else {
            self.frames.len() + self.cursor - samples
        };
        self.frames[index]
    }

    pub fn pair(&self, samples: f64, external: bool) -> [f64; 2] {
        let offset = usize::from(external) * 2;
        let exact = samples as usize;
        let current = self.at(exact);
        let fraction = samples - exact as f64;
        if fraction == 0.0 {
            return [current[offset], current[offset + 1]];
        }
        let prior = self.at(exact + 1);
        std::array::from_fn(|i| {
            let i = offset + i;
            current[i] + fraction * (prior[i] - current[i])
        })
    }
}
