//! Live access to Overture Maps transportation data via SedonaDB, queried in-process
//! against the public S3 bucket. This module currently just opens a SedonaDB context;
//! later slice tasks extend it to register anonymous S3 access and query the
//! `theme=transportation` segments/connectors intersecting the GPS bounding boxes.

use sedona::context::SedonaContext;

/// Open a SedonaDB context with the default geometry functions registered.
pub fn open_context() -> SedonaContext {
    SedonaContext::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SedonaDB context builds and opens with our selected feature set — the
    /// smoke test that the engine is wired in and links.
    #[test]
    fn opens_a_sedona_context() {
        let ctx = open_context();
        // `information_schema` is registered on the underlying DataFusion catalog,
        // so the well-known `datafusion` catalog is present on a healthy context.
        assert!(ctx.ctx.catalog("datafusion").is_some());
    }
}
