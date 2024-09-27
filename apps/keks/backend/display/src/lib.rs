use tiny_skia::*;

trait RegionStage {

}

trait LayoutStage {

}

pub struct PixmapLayout {
    pixmap: Pixmap
}

impl PixmapLayout {
    pub fn new(width: u32, height: u32) -> Result<PixmapLayout, Box<dyn std::error::Error>> {
        let pixmap = Pixmap::new(width, height).ok_or("Failed to create pixmap")?;

        Ok(PixmapLayout {
            pixmap
        })
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        Ok(self.pixmap.encode_png()?)
    }
}

impl RegionStage for PixmapLayout {

}

impl LayoutStage for PixmapLayout {

}