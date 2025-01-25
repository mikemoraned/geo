use geo::Point;

#[derive(Clone)]
pub struct RegionSummary {
    pub id: String,
    pub group_name: String,
    pub centroid: Point<f64>,
    pub bucket_width: f64,
    pub lengths: Vec<f64>,
    pub dominant: (usize, f64)
}

impl RegionSummary {

    pub fn new(id: String, group_name: String, centroid: Point<f64>, bucket_width: f64, lengths: Vec<f64>) -> RegionSummary {
        let dominant = dominant(&lengths);
        RegionSummary { id, group_name, centroid, bucket_width, lengths, dominant }
    }

    pub fn arrange_lengths_by_dominant_degree(&self) -> Vec<f64> {
        let (offset, _  ) = self.dominant;
        let mut arranged = vec![0.0; 360];
        for i in 0..360 {
            let degree = (offset + i) % 360;
            arranged[i] = self.lengths[degree];
        }
        arranged
    }

    pub fn distance_from(&self, other: &RegionSummary) -> f64 {
        let lengths = self.arrange_lengths_by_dominant_degree();
        let other_lengths = other.arrange_lengths_by_dominant_degree();
        let mut total_diff = 0.0;
        for i in 0..360 {
            let diff = (lengths[i] - other_lengths[i]).abs();
            total_diff += diff;
        }
        let avg_diff = total_diff / 360.0;
        return avg_diff;
    }
}

fn dominant(lengths: &Vec<f64>) -> (usize, f64) {
    let mut max = None;
    for (degree, length) in lengths.iter().enumerate() {
        let total = *length 
        + lengths[(degree + 90) % 360] 
        + lengths[(degree + 180) % 360] 
        + lengths[(degree + 270) % 360];
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