use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, BufWriter, Cursor},
    path::PathBuf,
};

use clap::{command, Parser};
use conversion::projection::Projection;
use geo::{coord, Area, BoundingRect, Coord, Geometry, GeometryCollection, LineString, Within};
use geozero::{
    geo_types::GeoWriter,
    geojson::{GeoJsonReader, GeoJsonWriter},
    GeozeroDatasource, GeozeroGeometry,
};
use image::{GrayImage, ImageReader, Luma, Rgba, RgbaImage};
use imageproc::{
    definitions::Image,
    region_labelling::{connected_components, Connectivity},
};
use rand::Rng;
use regions::contours::find_contours_in_luma;
use thiserror::Error;
use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// find regions in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input GeoJSON `.geojson` file(s) representing what to be drawn as the backgrounds
    #[arg(long)]
    background: Vec<PathBuf>,

    /// input GeoJSON `.geojson` file(s) representing what to be drawn as the borders
    #[arg(long)]
    borders: Vec<PathBuf>,

    /// whether to exclude regions that are on the border
    #[arg(long, default_value_t = false)]
    exclude_border: bool,

    /// only allow regions whos proportions of width, height, or area are less than this value
    #[arg(long, default_value_t = 0.25)]
    exclude_by_proportion: f32,

    /// template file name for the stages; must contain STAGE_NAME
    #[arg(long)]
    stage_template: PathBuf,

    /// output GeoJSON `.geojson` file representing the regions found
    #[arg(long)]
    regions: PathBuf,
}

#[derive(Error, Debug)]
pub enum RegionsError {
    #[error("Could not read geometry from file")]
    CannotReadGeometry,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let background_geometry = read_geometry(&args.background)?;
    let border_geometry = read_geometry(&args.borders)?;

    let (draw_image, projection) = draw_routes(&background_geometry, &border_geometry)?;

    let draw_stage_png = args
        .stage_template
        .to_str()
        .unwrap()
        .replace("STAGE_NAME", "draw");
    draw_image.save(draw_stage_png)?;

    let background_color = Luma([255u8]);
    let labelled_image: Image<Luma<u32>> =
        connected_components(&draw_image, Connectivity::Four, background_color);

    let labelled_colored_image: RgbaImage = assign_random_colors(&labelled_image);

    let labelled_stage_png = args
        .stage_template
        .to_str()
        .unwrap()
        .replace("STAGE_NAME", "labelled");
    labelled_colored_image.save(labelled_stage_png)?;

    let contours = find_contours_in_luma(Luma([0u32; 1]), &labelled_image);
    println!("Found {} contours", contours.len());
    let contours_image = draw_contours(&contours, labelled_image.width(), labelled_image.height())?;
    let contours_stage_png = args
        .stage_template
        .to_str()
        .unwrap()
        .replace("STAGE_NAME", "contours");
    contours_image.save(contours_stage_png)?;

    let mut contour_collection = GeometryCollection::from(
        contours
            .iter()
            .map(|contour| {
                let coords: Vec<Coord> = contour
                    .iter()
                    .map(|point| {
                        let (x, y) = projection.invert(point.x as f64, point.y as f64);
                        coord!(x: x, y: y)
                    })
                    .collect();
                let exterior = geo::LineString::new(coords);
                let poly = geo::Polygon::new(exterior, vec![]);
                Geometry::Polygon(poly)
            })
            .collect::<Vec<Geometry>>(),
    );

    if args.exclude_border {
        contour_collection = exclude_border(&contour_collection);
    }

    contour_collection = exclude_by_proportion(&contour_collection, args.exclude_by_proportion);

    let regions_file = BufWriter::new(File::create(args.regions)?);
    let mut regions_writer = GeoJsonWriter::new(regions_file);
    Geometry::GeometryCollection(contour_collection).process_geom(&mut regions_writer)?;

    Ok(())
}

fn read_geometry(paths: &Vec<PathBuf>) -> Result<Geometry, Box<dyn std::error::Error>> {
    let mut geometries_per_path = vec![];
    for path in paths {
        let mut file = BufReader::new(File::open(path)?);
        let mut reader = GeoJsonReader(&mut file);
        let mut writer = GeoWriter::new();
        reader.process_geom(&mut writer)?;

        if let Some(geometry) = writer.take_geometry() {
            geometries_per_path.push(geometry);
        } else {
            return Err(Box::new(RegionsError::CannotReadGeometry));
        }
    }

    let collection = GeometryCollection::new_from(geometries_per_path);
    Ok(Geometry::GeometryCollection(collection))
}

fn exclude_by_proportion(collection: &GeometryCollection, proportion: f32) -> GeometryCollection {
    let bounds = collection.bounding_rect().unwrap();
    let max_width = bounds.width() as f32 * proportion;
    let max_height = bounds.height() as f32 * proportion;
    let max_area = (collection.signed_area()) as f32 * proportion;
    let filtered: Vec<Geometry> = collection
        .clone()
        .into_iter()
        .filter(|geom| {
            if let Geometry::Polygon(poly) = geom {
                let poly_bounds = poly.bounding_rect().unwrap();
                let width = poly_bounds.width() as f32;
                let height = poly_bounds.height() as f32;
                let area = poly.signed_area() as f32;
                width < max_width && height < max_height && area < max_area
            } else {
                false
            }
        })
        .collect();
    GeometryCollection::from(filtered)
}

fn exclude_border(collection: &GeometryCollection) -> GeometryCollection {
    let current_bounds = collection.bounding_rect().unwrap();

    // exclude regions that sit on the border by only including those that are fully contained within a slightly smaller bounding box
    let shrink_factor = 0.01;
    let shrink_amount_x = shrink_factor * current_bounds.width();
    let shrink_amount_y = shrink_factor * current_bounds.height();
    let min = coord!(x: current_bounds.min().x + shrink_amount_x, y: current_bounds.min().y + shrink_amount_y);
    let max = coord!(x: current_bounds.max().x - shrink_amount_x, y: current_bounds.max().y - shrink_amount_y);
    let smaller_bounds = geo::Rect::new(min, max);
    let filtered: Vec<Geometry> = collection
        .clone()
        .into_iter()
        .filter(|geom| {
            if let Geometry::Polygon(poly) = geom {
                let bounds = poly.bounding_rect().unwrap();
                bounds.is_within(&smaller_bounds)
            } else {
                false
            }
        })
        .collect();
    GeometryCollection::from(filtered)
}

fn assign_random_colors(labelled_image: &Image<Luma<u32>>) -> RgbaImage {
    let unique_ids = labelled_image
        .pixels()
        .map(|p| p[0])
        .collect::<HashSet<u32>>();
    let mut color_map = HashMap::new();
    for id in unique_ids {
        let color = Rgba([rand::random(), rand::random(), rand::random(), 255]);
        color_map.insert(id, color);
    }
    let image = RgbaImage::from_fn(labelled_image.width(), labelled_image.height(), |x, y| {
        let label = labelled_image.get_pixel(x, y)[0];
        *color_map.get(&label).unwrap()
    });
    image
}

fn draw_routes(
    background: &Geometry,
    borders: &Geometry,
) -> Result<(GrayImage, Projection), Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let bounds = background.bounding_rect().unwrap();
    println!("Bounding rect: {:?}", bounds);

    let scale = 10000.0;
    let projection = Projection::from_geo_bounding_box_to_scaled_space(bounds, scale);

    let width = bounds.width() as f32;
    let height = bounds.height() as f32;

    let width_px = (width * scale).ceil() as u32;
    let height_px = (height * scale).ceil() as u32;

    println!("Width: {} Height: {}", width, height);
    println!("Width px: {} Height px: {}", width_px, height_px);
    let mut pixmap = Pixmap::new(width_px, height_px).ok_or("Failed to create pixmap")?;

    let transform = projection.as_transform();

    let mut black = Paint::default();
    black.set_color(Color::BLACK);
    black.anti_alias = true;

    let mut white = Paint::default();
    white.set_color(Color::WHITE);
    white.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 0.0005 * width.min(height);

    // anything that is white is a border and anything that is black is a candidate region,

    // so, first make everything white by default
    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, width_px as f32, height_px as f32)
            .ok_or("Failed to create rect")?,
        &white,
        Transform::identity(),
        None,
    );

    // then draw backgrounds for regions
    let count_background_geometries =
        draw_geometry(&mut pixmap, background, &black, &stroke, transform)?;
    println!("Drew {} black backgrounds", count_background_geometries);

    // then draw candidate borders
    let count_borders_geometries = draw_geometry(&mut pixmap, borders, &white, &stroke, transform)?;
    println!("Drew {} white borders", count_borders_geometries);

    // apply threshold to get a binary image, where black is candidate regions and white is ignorable border
    let image = GrayImage::from_fn(width_px, height_px, |x, y| {
        let pixmap_color = pixmap.pixel(x, y).unwrap();
        if pixmap_color == Color::BLACK.to_color_u8().premultiply() {
            Luma([0u8])
        } else {
            Luma([255u8])
        }
    });

    Ok((image, projection))
}

fn draw_geometry(
    pixmap: &mut Pixmap,
    geometry: &Geometry,
    paint: &Paint,
    stroke: &Stroke,
    transform: Transform,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut transparent = Paint::default();
    transparent.set_color(Color::TRANSPARENT);
    transparent.anti_alias = true;

    let mut red = Paint::default();
    red.set_color(Color::from_rgba(1.0, 0.0, 0.0, 1.0).unwrap());
    red.anti_alias = true;

    let mut black = Paint::default();
    black.set_color(Color::BLACK);
    black.anti_alias = true;

    match geometry {
        Geometry::Polygon(poly) => {
            let path = path_from_line(poly.exterior())?;
            pixmap.fill_path(&path, paint, FillRule::EvenOdd, transform, None);
            for interior in poly.interiors() {
                let interior_path = path_from_line(interior)?;
                pixmap.fill_path(&interior_path, &black, FillRule::EvenOdd, transform, None);
            }
            Ok(1)
        }
        Geometry::LineString(line) => {
            let path = path_from_line(line)?;
            pixmap.stroke_path(&path, paint, &stroke, transform, None);
            Ok(1)
        }
        Geometry::MultiPolygon(multi_poly) => {
            let mut count = 0usize;
            for poly in multi_poly.iter() {
                count += draw_geometry(
                    pixmap,
                    &Geometry::Polygon(poly.clone()),
                    paint,
                    stroke,
                    transform,
                )?;
            }
            Ok(count)
        }
        Geometry::GeometryCollection(collection) => {
            let mut count = 0usize;
            for geom in collection.iter() {
                count += draw_geometry(pixmap, geom, paint, stroke, transform)?;
            }
            Ok(count)
        }
        _ => Ok(0),
    }
}

fn path_from_line(line: &LineString) -> Result<Path, Box<dyn std::error::Error>> {
    let mut pb = PathBuilder::new();
    for (i, point) in line.points().enumerate() {
        if i == 0 {
            pb.move_to(point.x() as f32, point.y() as f32);
        } else {
            pb.line_to(point.x() as f32, point.y() as f32);
        }
    }
    Ok(pb.finish().ok_or("Failed to finish path")?)
}

fn draw_contours(
    contours: &Vec<Vec<regions::contours::Point<u32>>>,
    width_px: u32,
    height_px: u32,
) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let mut pixmap = Pixmap::new(width_px, height_px).ok_or("Failed to create pixmap")?;

    let mut black = Paint::default();
    black.set_color(Color::BLACK);
    black.anti_alias = true;

    let mut white = Paint::default();
    white.set_color(Color::WHITE);
    white.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 1.0;

    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, width_px as f32, height_px as f32)
            .ok_or("Failed to create rect")?,
        &black,
        Transform::identity(),
        None,
    );

    let mut rng = rand::thread_rng();
    for contour in contours {
        if contour.len() < 2 {
            continue;
        }
        let mut pb = PathBuilder::new();
        for (i, point) in contour.iter().enumerate() {
            if i == 0 {
                pb.move_to(point.x as f32, point.y as f32);
            } else {
                pb.line_to(point.x as f32, point.y as f32);
            }
        }
        let path = pb.finish().ok_or("Failed to finish path")?;
        pixmap.stroke_path(&path, &white, &stroke, Transform::identity(), None);
        let mut paint = Paint::default();
        paint.set_color_rgba8(
            rng.gen_range(0..255),
            rng.gen_range(0..255),
            rng.gen_range(0..255),
            255,
        );
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::EvenOdd,
            Transform::identity(),
            None,
        );
    }

    let png_bytes = pixmap.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_rgba8())
}
