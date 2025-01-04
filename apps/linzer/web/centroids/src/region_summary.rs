use geo::Point;

#[derive(Clone)]
pub struct RegionSummary {
    pub id: usize,
    pub centroid: Point<f64>,
    pub bucket_width: f64,
    pub normalised: Vec<f64>,
    pub dominant: (usize, f64)
}

impl RegionSummary {

    pub fn new(id: usize, centroid: Point<f64>, bucket_width: f64, normalised: Vec<f64>) -> RegionSummary {
        let dominant = dominant(&normalised);
        RegionSummary { id, centroid, bucket_width, normalised, dominant }
    }

    pub fn distance_from(&self, other: &RegionSummary) -> f64 {
        let (offset, _) = self.dominant;
        let (other_offset, _) = other.dominant;
        let mut total_diff = 0.0;
        for i in 0..360 {
            let degree = (offset + i) % 360;
            let other_degree = (other_offset + i) % 360;
            let diff = (self.normalised[degree] - other.normalised[other_degree]).abs();
            total_diff += diff;
        }
        let avg_diff = total_diff / 360.0;
        return avg_diff;
    }
}

fn dominant(normalised: &Vec<f64>) -> (usize, f64) {
    let mut max = None;
    for (degree, length) in normalised.iter().enumerate() {
        let total = *length 
        + normalised[(degree + 90) % 360] 
        + normalised[(degree + 180) % 360] 
        + normalised[(degree + 270) % 360];
        if let Some((_, max_length, max_total)) = max {
            if total > max_total {
                max = Some((degree, length, total));
            }
            else if total == max_total && length > max_length {
                max = Some((degree, length, total));
            }
        }
        else {
            max = Some((degree, length, total));
        }
    }
    max.map(|(degree, length, _)| (degree, *length)).unwrap()
}