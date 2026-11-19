# test-countries

Writes an Overture extract, with a country's areas, into a store for a test. Tests that read a
country's areas take this crate as a dev-dependency, so they share one way of writing an extract.

`record_extract` records the extract in the manifest and writes its division areas.
`division_area` builds those areas as a batch, and `square_from` builds a square area.
