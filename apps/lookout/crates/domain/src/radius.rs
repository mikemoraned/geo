#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Radius(f64);

impl Radius {
    pub const fn new(metres: f64) -> Self {
        Self(metres)
    }

    pub const fn as_metres(self) -> f64 {
        self.0
    }
}
