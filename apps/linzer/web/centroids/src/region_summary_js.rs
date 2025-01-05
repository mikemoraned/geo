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
    pub fn id(&self) -> usize {
        self.summary.id
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
    pub fn as_data_uri_image(&self, width: u32, height: u32) -> Result<String, JsValue> {
        use tiny_skia::*;
        use base64::prelude::*;

        let mut pixmap = Pixmap::new(width as u32, height as u32).ok_or(format!("Failed to create Pixmap"))?;

        let mut red = Paint::default();
        red.set_color_rgba8(255, 0, 0, 255);
        red.anti_alias = true;

        let mut stroke = Stroke::default();
        stroke.width = 1.0;

        let mut x_y_pairs = vec![];
        for (degree, length) in self.summary.arrange_lengths_by_dominant_degree().iter().enumerate() {
            let radians = (degree as f32).to_radians();
            let length = *length as f32;
            let x = radians.cos() * length * (width as f32);
            let y = radians.sin() * length * (height as f32);
            x_y_pairs.push((x, y));
        }
        let mut pb = PathBuilder::new();
        let (x, y) = x_y_pairs[0];
        pb.move_to(x, y);
        for i in 1..x_y_pairs.len() {
            let (x, y) = x_y_pairs[i];
            pb.line_to(x, y);
        }
        pb.close();
        let path = pb.finish().ok_or(format!("Failed to finish path"))?;
        pixmap.stroke_path(&path, &red, &stroke, Transform::identity(), None);

        let png_bytes = pixmap.encode_png().map_err(|e| format!("Failed to create encode PNG: {e:?}"))?;
        let encoded = BASE64_STANDARD.encode(&png_bytes);
        let data_uri = format!("data:image/png;base64,{}", encoded);
        
        Ok(data_uri)

        // JsValue::from_serde(&self.summary.dominant.1).unwrap()
    }
}

