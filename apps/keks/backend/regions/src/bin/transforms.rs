use tiny_skia::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut black = Paint::default();
    black.set_color_rgba8(0, 0, 0, 255);

    let mut orig = Paint::default();
    orig.set_color_rgba8(0, 127, 0, 200);
    orig.anti_alias = true;

    let mut after = Paint::default();
    after.set_color_rgba8(127, 0, 0, 200);
    after.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 6.0;

    let width_px = 500u32;
    let height_px = 500u32;
    let mut pixmap = Pixmap::new(width_px, height_px).unwrap();
    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, width_px as f32, height_px as f32).unwrap(),
        &black,
        Transform::identity(), 
        None);

    // Bounding rect: Rect { min: Coord { x: -3.530138, y: 55.854295 }, max: Coord { x: -3.001677, y: 55.995439 } }

    let min_x = -3.530138;
    let min_y = 55.854295;
    let max_x = -3.001677;
    let max_y = 55.995439;

    // let min_x = -50.0;
    // let min_y = -50.0;
    // let max_x = 200.0;
    // let max_y = 100.0;

    let scale_x = width_px as f32 / (max_x - min_x);
    let scale_y = height_px as f32 / (max_y - min_y);

    let offset_x = -1.0 * min_x;
    let offset_y = -1.0 * min_y;

    let transform = Transform::from_translate(offset_x, offset_y).post_scale(scale_x, scale_y);
    println!("transform: {:?}", transform);

    let path = {
        let mut pb = PathBuilder::new();
        pb.move_to(min_x, min_y);
        pb.line_to(max_x, min_y);
        pb.line_to(max_x, max_y);
        pb.line_to(min_x, max_y);
        pb.close();
        pb.finish().ok_or("Failed to finish path")?
    };
    println!("Path: {:?}", path);
    pixmap.stroke_path(&path, &orig, &stroke, Transform::identity(), None);
    pixmap.stroke_path(&path, &after, &stroke, transform, None);

    pixmap.save_png("transform.png").unwrap();

    Ok(())
}