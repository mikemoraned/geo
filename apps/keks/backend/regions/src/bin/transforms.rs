use tiny_skia::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 127, 0, 200);
    paint.anti_alias = true;

    let min_x = 0.0;
    let min_y = 0.0;
    let max_x = 200.0;
    let max_y = 100.0;

    let path = {
        let mut pb = PathBuilder::new();
        pb.move_to(min_x, min_y);
        pb.line_to(max_x, min_y);
        pb.line_to(max_x, max_y);
        pb.line_to(min_x, max_y);
        pb.close();
        pb.finish().ok_or("Failed to finish path")?
    };

    let mut stroke = Stroke::default();
    stroke.width = 6.0;

    let mut pixmap = Pixmap::new(500, 500).unwrap();
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    pixmap.save_png("transform.png").unwrap();

    Ok(())
}