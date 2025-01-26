use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use domain::signature::region_signature::RegionSignature;

#[wasm_bindgen]
#[derive(Clone)]
pub struct RegionSignatureJS {
    signature: RegionSignature,
}

impl RegionSignatureJS {
    pub fn new(signature: RegionSignature) -> RegionSignatureJS {
        RegionSignatureJS { signature }
    }
}

#[wasm_bindgen]
impl RegionSignatureJS {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.signature.id.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn group_name(&self) -> JsValue {
        JsValue::from_serde(&self.signature.group_name).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn centroid(&self) -> JsValue {
        JsValue::from_serde(&self.signature.centroid).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn bucket_width(&self) -> f64 {
        self.signature.bucket_width
    }
    #[wasm_bindgen(getter)]
    pub fn lengths(&self) -> JsValue {
        JsValue::from_serde(&self.signature.lengths).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_degree(&self) -> JsValue {
        JsValue::from_serde(&self.signature.dominant.0).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_length(&self) -> JsValue {
        JsValue::from_serde(&self.signature.dominant.1).unwrap()
    }
    pub fn as_data_uri_image(&self, side_length: u32) -> Result<String, JsValue> {
        use base64::prelude::*;
        use tiny_skia::*;

        let mut pixmap =
            Pixmap::new(side_length, side_length).ok_or("Failed to create Pixmap".to_string())?;

        let mut green = Paint::default();
        green.set_color_rgba8(0, 255, 0, 255);
        green.anti_alias = true;

        let mut gray = Paint::default();
        gray.set_color_rgba8(150, 150, 150, 255);
        gray.anti_alias = true;

        let stroke = tiny_skia::Stroke {
            width: 2.0,
            ..Default::default()
        };

        let radius = (side_length as f32) / 2.0;
        let circle = PathBuilder::from_circle(radius, radius, radius)
            .ok_or("Failed to create circle".to_string())?;
        pixmap.fill_path(
            &circle,
            &gray,
            FillRule::EvenOdd,
            Transform::identity(),
            None,
        );

        let mut x_y_pairs = vec![];
        for (degree, length) in self
            .signature
            .arrange_lengths_by_dominant_degree()
            .iter()
            .enumerate()
        {
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
        for (x, y) in x_y_pairs.iter().skip(1) {
            pb.line_to(radius + x, radius + y);
        }
        pb.move_to(radius, radius);
        pb.close();
        let path = pb.finish().ok_or("Failed to finish path".to_string())?;
        pixmap.stroke_path(&path, &green, &stroke, Transform::identity(), None);

        let png_bytes = pixmap
            .encode_png()
            .map_err(|e| format!("Failed to create encode PNG: {e:?}"))?;
        let encoded = BASE64_STANDARD.encode(&png_bytes);
        let data_uri = format!("data:image/png;base64,{}", encoded);

        Ok(data_uri)
    }
}
