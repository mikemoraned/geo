# Current Slice: visualise transport geo data for regions

### Target

To get a sense of what kinds of overlaps we may see with real transport data, fetch data from "transport" overturemaps data, where it overlaps with device data. The intent is to see where we can correspond data with transport segments.

We'll do this visualisation in rerun.

### Straw man implementation / preferences

I suggest:
1. grouping data by id and by day
2. getting gps coords for each group
3. finding the bounding box
4. finding all connectors and segments in overturemaps that intersect those bounding boxes
5. unioning and dedupe those together into a single dataset and save

The bounding boxes are probably small enough that a live fetch of overturemaps data from S3 will be fast enough, but if needed we have a recent local copy on disk. We should do this as a new `enrich` cli in a new `geo` crate, and we should save in sqlite as a new "transport" table, using whatever direct geo support it has. There are perhaps better geo db but let's stick with sqlite for now unless it makes it really hard.

The visualisation should be done by extending the current cli to add a new track corresponding to the segments. Later on we might want to restrict the segments visualised to be those within some distance of a sample, but we can leave that out for now if it's not trivial to do.
