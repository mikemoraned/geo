use geo::{Bearing, Coord, Haversine, Point};
use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

#[wasm_bindgen]
pub struct TestCard {
    coord: Coord,
    center_point: Point,
    northern_point: Point,
    east_point: Point,
}

#[wasm_bindgen]
impl TestCard {
    #[wasm_bindgen(getter)]
    pub fn x(&self) -> f64 {
        self.coord.x
    }

    #[wasm_bindgen(getter)]
    pub fn y(&self) -> f64 {
        self.coord.y
    }

    #[wasm_bindgen(getter)]
    pub fn coord(&self) -> JsValue {
        return JsValue::from_serde(&[self.coord.x, self.coord.y]).unwrap();
    }

    #[wasm_bindgen(getter)]
    pub fn bearing_north_degrees(&self) -> f64 {
        Haversine::bearing(self.center_point, self.northern_point)
    }

    #[wasm_bindgen(getter)]
    pub fn bearing_east_degrees(&self) -> f64 {
        Haversine::bearing(self.center_point, self.east_point)
    }
}

impl TestCard {
    pub fn new(coord: Coord) -> TestCard {
        let offset = 0.01;
        let center_point: Point = coord.into();
        let northern_point = Point::new(coord.x, coord.y + offset);
        let east_point = Point::new(coord.x + offset, coord.y);
        TestCard {
            coord,
            center_point,
            northern_point,
            east_point,
        }
    }
}
