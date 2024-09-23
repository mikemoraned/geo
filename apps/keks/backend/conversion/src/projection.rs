pub struct Projection {
    pub scale_x: f64,
    pub scale_y: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Projection {
    pub fn from_geo_bounding_box_to_scaled_space(bounds: geo::Rect, scale: f32) -> Projection {
        let min_x = bounds.min().x as f32;
        let min_y = bounds.min().y as f32;
    
        let scale_x = scale;
        let scale_y = scale;
    
        let offset_x = -1.0 * min_x;
        let offset_y = -1.0 * min_y;
    
        Projection {
            scale_x: scale_x as f64,
            scale_y: scale_y as f64,
            offset_x: offset_x as f64,
            offset_y: offset_y as f64,
        }    
    }

    pub fn as_transform(&self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_translate(self.offset_x as f32, self.offset_y as f32).post_scale(self.scale_x as f32, self.scale_y as f32)
    }

    pub fn invert(&self, x: f64, y: f64) -> (f64, f64) {
        let x = x / self.scale_x - self.offset_x;
        let y = y / self.scale_y - self.offset_y;
        (x, y)
    }
}
