use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::region_summary::RegionSummary;



#[wasm_bindgen]
#[derive(Clone)]
pub struct RegionSummaryJS {
    summary: RegionSummary
}

impl RegionSummaryJS {
    pub fn new(summary: RegionSummary) -> RegionSummaryJS {
        RegionSummaryJS { summary }
    }
}

#[wasm_bindgen]
impl RegionSummaryJS {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.summary.id.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn group_name(&self) -> JsValue {
        JsValue::from_serde(&self.summary.group_name).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn centroid(&self) -> JsValue {
        JsValue::from_serde(&self.summary.centroid).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn bucket_width(&self) -> f64 {
        self.summary.bucket_width
    }
    #[wasm_bindgen(getter)]
    pub fn lengths(&self) -> JsValue {
        JsValue::from_serde(&self.summary.lengths).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_degree(&self) -> JsValue {
        JsValue::from_serde(&self.summary.dominant.0).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_length(&self) -> JsValue {
        JsValue::from_serde(&self.summary.dominant.1).unwrap()
    }
    pub fn as_data_uri_image(&self, side_length: u32) -> Result<String, JsValue> {
        use tiny_skia::*;
        use base64::prelude::*;

        let mut pixmap = Pixmap::new(side_length, side_length).ok_or(format!("Failed to create Pixmap"))?;

        let mut green = Paint::default();
        green.set_color_rgba8(0, 255, 0, 255);
        green.anti_alias = true;

        let mut gray = Paint::default();
        gray.set_color_rgba8(150, 150, 150, 255);
        gray.anti_alias = true;

        let mut stroke = Stroke::default();
        stroke.width = 2.0;

        let radius = (side_length as f32) / 2.0;
        let circle = PathBuilder::from_circle(radius, radius, radius).ok_or(format!("Failed to create circle"))?;
        pixmap.fill_path(&circle, &gray, FillRule::EvenOdd, Transform::identity(), None);

        let mut x_y_pairs = vec![];
        for (degree, length) in self.summary.arrange_lengths_by_dominant_degree().iter().enumerate() {
            let radians = (degree as f32).to_radians();
            let length = *length as f32;
            let x = radians.cos() * length * radius;
            let y = radians.sin() * length * radius;
            x_y_pairs.push((x, y));
        }
        let mut pb = PathBuilder::new();
        let (x, y) = x_y_pairs[0];
        pb.move_to(radius, radius);
        pb.line_to(radius + x, radius + y);
        for i in 1..x_y_pairs.len() {
            let (x, y) = x_y_pairs[i];
            pb.line_to(radius + x, radius + y);
        }
        pb.move_to(radius, radius);
        pb.close();
        let path = pb.finish().ok_or(format!("Failed to finish path"))?;
        pixmap.stroke_path(&path, &green, &stroke, Transform::identity(), None);

        let png_bytes = pixmap.encode_png().map_err(|e| format!("Failed to create encode PNG: {e:?}"))?;
        let encoded = BASE64_STANDARD.encode(&png_bytes);
        let data_uri = format!("data:image/png;base64,{}", encoded);
        
        Ok(data_uri)

        // JsValue::from_serde(&self.summary.dominant.1).unwrap()
    }
}

