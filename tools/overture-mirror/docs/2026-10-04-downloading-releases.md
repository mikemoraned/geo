# Downloading a release over a home connection

*Assessment of 2026-10-04, written from published sources before any measurement of the
connection a mirror is filled over. It records the reasoning behind a choice of download
strategy, not a measurement. Whatever survives a first set of trials belongs in the README. The
rest goes stale.*

How a whole Overture release is best copied from the public bucket to a mirror, when the
connection between them is a domestic line of modest capacity rather than a data-centre link.

A release is about 577 GiB in about 1,300 files, most of them hundreds of MiB. The bucket and
its only alternative both sit in the US West region, so a mirror outside North America reaches
them over a long, high-latency path.

## Parallel downloads

Several files are copied at once. Whether that helps depends on which of two limits binds first.

**The line itself.** Once the streams together fill the line, a further stream adds no
throughput. The streams divide the same capacity, each runs slower, and the cost rises:

- An interruption leaves more partly copied files to resume.
- Other traffic on the same line slows.
- A slow stream looks more like a stalled one.

**A single stream.** One TCP stream carries at most the data it can have unacknowledged per
round trip, the bandwidth-delay product. After a lost packet it halves its rate and recovers by
small steps, one per round trip. A round trip from Europe to US West typically takes 140–150 ms.
Over such a path that recovery is slow, and one stream can fall well short of even a modest line.
Parallel streams compensate: while one recovers, the others keep the line full. This is the
established result for parallel TCP over long, lossy paths.

The number of streams worth running is therefore about the line's capacity divided by what one
stream achieves. On a domestic line that is typically two to four. A fixed eight is likely more
than the line needs.

The figure can be measured before anything is built. The run log records each copied file's
rate. The sum of the rates of the files in flight, set against a speed test of the line, shows
which limit binds:

- One stream near the line's capacity means one or two streams suffice.
- Many slow streams that together match the line mean the line is the limit, and fewer streams
  would do as well.

## Adaptive concurrency

The number of streams can instead be found while a run proceeds, by additive increase and
multiplicative decrease, the scheme TCP applies to its own rate:

- Start with one or two copies in flight.
- Every 30–60 seconds, compare the aggregate rate with the previous interval.
- Add a copy where the rate rose by more than about 10%, and remove one where it did not.
- Halve the copies in flight on a retry or a stall.

Measurements are noisy, from file boundaries and from the bucket's own variation, so the steps
are coarse: 1, 2, 4, 8. TCP already tunes each connection, so the outer loop needs no finer
control. A semaphore whose permits are added and withdrawn takes the place of a fixed limit on
copies in flight.

## Bulk-download tools

- **The AWS Common Runtime S3 client**, behind the AWS CLI v2's optional fast transfer mode,
  boto3, and the C++ SDK. It divides each object into ranged requests spread across several
  bucket servers, and adjusts toward a target throughput. It is the only adaptive client found.
  Its default target is 10 Gbps, set for hosts inside AWS.
- **s5cmd and rclone**, which run a fixed, high number of parallel transfers: 256 workers by
  default in s5cmd. Both far outpace `aws s3 sync` on data-centre links. On a domestic line the
  advantage ends once the line is full.
- **Ranged requests within one object.** These raise throughput on a fast link. On a slow one,
  parallel transfer of separate files gives the same benefit with less machinery.

None of these refuses a superseded release, verifies by signature, or syncs one mirror from
another, which a mirror here depends on.

## Overture's distribution

| Source | Holds | Notes |
| --- | --- | --- |
| Amazon S3, `s3://overturemaps-us-west-2/release/` | whole releases | anonymous access |
| Azure Blob Storage, `overturemapswestus2` | whole releases | copied with `azcopy`, in the same region by a different network route |
| `overturemaps` Python CLI, DuckDB, QGIS GeoParquet plugin | a bounding box or a query | read the cloud copies directly |
| BigQuery, Databricks, and Snowflake (CARTO), and Wherobots | the data as tables | query platforms, not files |
| Fused, on Source Cooperative | a geo-partitioned copy | not in a release's original layout |

No torrent exists, official or community. The licences permit redistribution, but a monthly
release of this size needs sustained seeding, and the cloud copies cost a downloader nothing.

Azure is the one alternative for a whole release. It sits in the same region, so it is no
nearer, but its route differs, which matters where a provider's path to AWS is congested.

## Proposed next steps

1. A `--concurrency` setting with a default of four, and short trials at 1, 2, 4, and 8 streams,
   read from the run log against a speed test.
2. Adaptive concurrency, only where the trials show the best number varying from run to run.
3. Azure Blob Storage as a second bucket source, only where the trials point at the route to AWS.

## Not covered

- Measured figures for any real connection. The round-trip time above is a typical value, not a
  measurement.
- Cost. Both official sources carry no charge to an anonymous downloader.
- Uploading a mirror to a bucket.

## References

- [Accessing the Overture catalog](https://docs.overturemaps.org/getting-data/cloud-sources/)
  and [Overture data mirrors](https://docs.overturemaps.org/getting-data/data-mirrors/) — the
  official sources and the mirrors
- [Fused-partitioned Overture on Source Cooperative](https://source.coop/fused/overture)
- [overturemaps-py](https://github.com/OvertureMaps/overturemaps-py) and the
  [QGIS GeoParquet Downloader](https://plugins.qgis.org/plugins/qgis_plugin_gpq_downloader/) —
  partial downloads
- [OvertureMaps discussion #3](https://github.com/orgs/OvertureMaps/discussions/3) — downloading,
  as discussed by the project
- [Improving Amazon S3 throughput with the AWS Common Runtime](https://aws.amazon.com/blogs/storage/improving-amazon-s3-throughput-for-the-aws-cli-and-boto3-with-the-aws-common-runtime/)
  and [Improving S3 throughput with the AWS SDK for C++](https://github.com/aws/aws-sdk-cpp/wiki/Improving-S3-Throughput-with-AWS-SDK-for-CPP-v1.9)
  — ranged requests and target throughput
- [s5cmd against the AWS CLI](https://www.doit.com/blog/save-time-and-money-on-s3-data-transfers-surpass-aws-cli-performance-by-up-to-80x)
  — fixed high concurrency on data-centre links
- [Bandwidth-delay product](https://speedtesthq.com/guides/fundamentals/bandwidth-delay-product),
  [Microsoft Research TR-2005-130](https://microsoft.com/en-us/research/wp-content/uploads/2016/02/tr-2005-130.pdf),
  and [Oracle A-Team on parallel streams](https://www.ateam-oracle.com/?p=9673) — single-stream
  limits and parallel TCP over long paths
- [Adaptive concurrency limiting](https://www.techinterview.org/post/3233473210/lld-adaptive-concurrency-limiting/?format=md)
  — additive increase, multiplicative decrease applied to concurrency
