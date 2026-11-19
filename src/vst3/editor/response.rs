use crate::parameters::{
    self,
    layout::{edge_ids, Layout},
    BandParam as P, Values, BANDS,
};

const LOG_FREQUENCY_SPAN: f64 = 6.907_755_278_982_137;

#[derive(Clone, Copy)]
pub struct BandShape {
    pub edges: [f64; 2],
    slope: [f64; 2],
}

impl BandShape {
    pub fn new(values: &Values, band: usize, edges: [f64; 2]) -> Self {
        let mut slope = [P::LowSlope, P::HighSlope].map(|field| {
            let id = field.id(band);
            2.0 * (parameters::spec(id).plain(values[id]) + 1.0)
        });
        if edge_ids(values, band)[0] != P::Low.id(band) {
            slope.swap(0, 1);
        }
        Self { edges, slope }
    }

    pub fn center(self) -> f64 {
        (self.edges[0] + self.edges[1]) * 0.5
    }

    pub fn contour_weight(self, frequency: f64) -> f32 {
        let center = self.center();
        let side = usize::from(frequency >= center);
        let edge = self.edges[side];
        if edge == side as f64 {
            return 1.0;
        }
        let direction = if side == 0 { 1.0 } else { -1.0 };
        let slope = direction * self.slope[side] * LOG_FREQUENCY_SPAN;
        // Ease both flanks into the band node and toward zero beyond the crossover.
        let weight =
            (1.0 + ((edge - center) * slope).exp()) / (1.0 + ((edge - frequency) * slope).exp());
        (weight * weight * (3.0 - 2.0 * weight)) as f32
    }

    fn ratios(self, frequency: f64) -> [f64; 2] {
        std::array::from_fn(|edge| {
            ((self.edges[edge] - frequency) * LOG_FREQUENCY_SPAN * self.slope[edge]).exp()
        })
    }
}

#[derive(Clone, Copy)]
struct Band {
    shape: BandShape,
    gain: f64,
}

pub struct Response {
    bands: [Option<Band>; BANDS],
    linear: bool,
}

impl Response {
    pub fn new(values: &Values, activity: [f32; BANDS]) -> Self {
        let layout = Layout::new(values);
        let bands = std::array::from_fn(|band| {
            let enabled = values[P::Enabled.id(band)] > 0.5 && values[parameters::BYPASS] < 0.5;
            if !enabled {
                return None;
            }
            let makeup = P::Makeup.id(band);
            let muted = values[P::Mute.id(band)] > 0.5 && values[P::Solo.id(band)] < 0.5;
            let db = if muted {
                -96.0
            } else {
                parameters::spec(makeup).plain(values[makeup]) + f64::from(activity[band])
            };
            Some(Band {
                shape: BandShape::new(values, band, layout.edges[band]),
                gain: 10.0_f64.powf(db * 0.05),
            })
        });
        Self {
            bands,
            linear: values[parameters::PHASE] > 0.5,
        }
    }

    pub fn db(&self, frequency: f64) -> f32 {
        if self.linear {
            let mut weight = 0.0_f64;
            let mut change = 0.0;
            for band in self.bands.iter().flatten() {
                let [hp, lp] = band.shape.ratios(frequency);
                let magnitude = 1.0 / ((1.0 + hp) * (1.0 + lp.recip())).sqrt();
                weight += magnitude;
                change += magnitude * (band.gain - 1.0);
            }
            (20.0 * (1.0 + change / weight.max(1.0)).max(1e-5).log10()) as f32
        } else {
            let mut sum = 0.0;
            for band in self.bands.iter().flatten() {
                let [low, high] = band.shape.ratios(frequency);
                let shelf = |ratio| band.gain * (band.gain + ratio) / (1.0 + ratio * band.gain);
                sum += 10.0 * (shelf(low) / shelf(high)).log10();
            }
            sum as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contours_follow_slope_and_keep_full_span_ends_open() {
        let mut values = parameters::defaults();
        let layout = Layout::new(&values);
        let edges = layout.edges[1];
        let gentle = BandShape::new(&values, 1, edges);
        values[P::LowSlope.id(1)] = 1.0;
        let steep = BandShape::new(&values, 1, edges);
        assert_eq!(steep.contour_weight(steep.center()), 1.0);
        assert!(steep.contour_weight(edges[0] - 0.06) < gentle.contour_weight(edges[0] - 0.06));
        assert_eq!(
            steep.contour_weight(edges[1]),
            gentle.contour_weight(edges[1])
        );
        assert_eq!(
            BandShape::new(&values, 0, layout.edges[0]).contour_weight(0.0),
            1.0
        );
        assert_eq!(
            BandShape::new(&values, 2, layout.edges[2]).contour_weight(1.0),
            1.0
        );
    }

    #[test]
    fn response_is_flat_at_default_and_preserves_the_direction_of_gain() {
        let mut values = parameters::defaults();
        let probe = 0.55;
        let neutral = Response::new(&values, [0.0; BANDS]);
        assert_eq!(neutral.db(probe), 0.0);
        for mode in [0.0, 1.0] {
            values[parameters::PHASE] = mode;
            let mut activity = [0.0; BANDS];
            activity[1] = -6.0;
            let response = Response::new(&values, activity);
            assert!(response.db(probe) < -4.0);
            assert!(response.db(0.02).abs() < 0.25);
        }
    }
}
