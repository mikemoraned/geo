# Capturing Great Britain's trains from a local Motis server

*Assessment of 2026-10-23. It fetched no feed for Great Britain and imported no timetable, so no
figure here comes from data for Great Britain. The evidence is Transitous's repository at commit
`f68d1781` and the Motis source at v2.11.3. It also takes in the feeds' documentation, two
projects built on them, and the store. Whatever survives a first import belongs in
[motis.md](motis.md), and the rest goes stale.*

How a local Motis server can serve Great Britain's trains with realtime arrivals, and what a
week-long capture from it needs. It is for whoever sets up that server and runs the capture.

The UK's national railways form two networks. National Rail runs in Great Britain: England,
Scotland, and Wales. NI Railways runs in Northern Ireland. This doc concerns National Rail. A
timetable is a static GTFS feed, and a realtime feed is GTFS-RT.

## National Rail publishes neither GTFS nor GTFS-RT

National Rail's timetable comes as CIF, the industry's own schedule format, or a JSON form of it.
Its realtime data comes from Darwin, National Rail Enquiries' system that forecasts and records each
train's times at each stop. The Rail Data Marketplace streams Darwin's messages over Kafka. Each
source needs an account.

Motis v2.11.3 reads realtime data in four protocols: GTFS-RT, SIRI, SIRI as JSON, and VDV AUS.
Darwin's messages fit none of them. A local server therefore needs a timetable and a realtime feed
that someone has converted from National Rail's data. Realtime resolves against the timetable's trip
ids, so the two must share them. [motis.md](motis.md) records the German case: against another
producer's timetable, about 99.9% of trip updates fail to resolve.

## Transitous takes both feeds from Aubin

Since August 2025, Transitous has taken its Great Britain timetable and realtime feed from Aubin
MaaS Limited, a London company behind a travel app. `feeds/gb.json` pairs the two in one dataset:

| Feed | URL | Content |
| --- | --- | --- |
| Timetable | `https://beta.aubin.app/gtfs/great_britain_gtfs.zip` | National Rail, converted from National Rail Enquiries' timetable, and buses from the Bus Open Data Service |
| Realtime | `https://realtime.aubin.app/great_britain_gtfsrt.binpb` | Updates built from National Rail Enquiries' data |

Transitous prepares the timetable before import:

- It drops the timetable's shapes, which run in straight lines between stations. Motis computes new
  ones during import, as for DELFI.
- It loads `gb-great-britain.lua` as the dataset's script. The timetable holds one route per
  operator, keyed by the operator's two-letter code, and the script sets each one's route type. The
  script is under the MIT licence, which permits copying it into the tree, unlike the AGPL-3.0
  `de-DELFI.lua`.
- It drops FlixBus, which it takes from FlixBus's own feed instead.

Transitous fetches the realtime feed through its feed proxy, which caches each response and limits
the rate of requests upstream. Transitous's repository stores each feed's secrets, encrypted, in the
feed's own file. The entry for Aubin's realtime feed holds no secret.

Aubin publishes no terms of its own, and Transitous credits the data to its sources:

- National Rail Enquiries, for the rail timetable and the realtime feed. Its open data terms build
  on the Open Government Licence v2.0, and ask for attribution to National Rail Enquiries.
- The Department for Transport's Bus Open Data Service, for the buses.
- OpenStreetMap contributors, under the Open Database Licence, for location data.

Both feeds come from one small provider, the timetable from a host named `beta`.

## TravelWhiz publishes a second timetable, with no realtime feed

TravelWhiz, maker of the Momego app, builds a rail-only timetable for Great Britain from Darwin's
data each night: `https://storage.travelwhiz.app/generated-gtfs/gb-nationalrail.gtfs.zip`. Its own
additions are under CC BY 4.0, and its sources keep their licences. It differs from Aubin's
timetable in four ways:

- It holds rail alone.
- It keys stops by NaPTAN ATCO code, and groups routes into operator corridors, such as
  `SR:ALO-GLQ`, ScotRail's Alloa to Glasgow Queen Street.
- It computes shapes from OpenStreetMap.
- Each trip carries `realtime_trip_id`: the train's schedule UID and run date, such as
  `U10021:20260401`.

TravelWhiz publishes no realtime feed. `realtime_trip_id` serves a bridge that turns Darwin's
messages into GTFS-RT, since Darwin identifies each train by the same UID and date.

## What the local server needs

A server for Great Britain runs from its own directory beside the German server in
`tools/motis-server`, with its own config. That config differs from the German server's in six
settings:

- **OSM extract.** Geofabrik's `great-britain-latest.osm.pbf` covers England, Scotland, and Wales,
  in under 2 GB, against 4.9 GB for Germany.
- **Dataset name.** `motis config` names each dataset after its file, without `_` or `.`, so
  `great_britain_gtfs.zip` becomes the dataset `greatbritaingtfs`.
- **Script.** A dataset takes one script. A script sets route types, as Transitous's does. When its
  `process_route` returns `false`, Motis drops the route and its trips. Dropping every route that is
  not rail keeps Aubin's buses out of the import.
- **Tiles and geocoding.** With no `tiles` block, and `geocoding` and `reverse_geocoding` set to
  `false`, the import skips both. The capture uses neither, though the server's web map draws its
  base map from the tiles. Together they take 17.5 GiB of the German server's 26 GiB data directory.
- **Port.** Motis listens on port 8080 by default, as the German server does. A second server
  running alongside it sets another in `server.port`, such as 8081.
- **Realtime.** After the import, Aubin's realtime URL goes into the dataset's `rt` list, as
  `enable_rt` adds DELFI's. An `rt` entry takes `headers`, so its requests can carry a `User-Agent`
  naming lookout. The server fetches each realtime feed every 60 seconds by default.

Two settings stay as the German server has them:

- **Shapes.** The `route_shapes` block applies unchanged: mode `all`, with every class the capture
  leaves out turned off. Mode `all` recomputes rail shapes whatever the timetable ships, so Aubin's
  straight lines need no dropping first.
- **Street routing.** `route_shapes` routes over the street graph, and the import builds that graph
  only with `street_routing` on.

Before the import, the config reads in part:

```yaml
osm: ../geo_data/great-britain.osm.pbf
timetable:
  datasets:
    greatbritaingtfs:
      path: ../geo_data/great_britain_gtfs.zip
      script: scripts/gb-rail.lua
  route_shapes:
    mode: all
    clasz: { BUS: false, COACH: false, … } # the German server's list
street_routing: true
geocoding: false
reverse_geocoding: false
server:
  port: "8081"
```

`scripts/gb-rail.lua` stands for a script holding Transitous's route types and the filter that drops
every route that is not rail.

## The capture

The poller captures Great Britain with no change to its code. The store holds an Overture extract
for country code GB, release 2026-09-23.1, with the regions GB-ENG, GB-SCT, GB-WLS, and GB-NIR.
`--country GB` captures the whole country, and `--region GB-SCT` narrows it to Scotland. A week's
capture from a server on port 8081 runs:

```sh
motis_track_area --motis-url http://127.0.0.1:8081 --feed local --label aubin-<build>.route-shapes \
  --country GB --poll-interval 10m --poll-duration 7d
```

The label names the timetable build and the setup, as the German labels do.

Five properties of the data differ from Germany's.

### Modes

Motis maps each route type to a mode. Under Transitous's script, the operators fall into four:

| Mode | Operators |
| --- | --- |
| Long-distance | Avanti West Coast, CrossCountry, Grand Central, Great Western Railway, Hull Trains, LNER, and Lumo |
| Night | Caledonian Sleeper |
| Suburban | Thameslink |
| Regional | 17 others, such as Northern, Southern, and TransPennine Express |

The capture keeps every rail mode but suburban, to leave out S-Bahn trains. In Great Britain that
leaves out Thameslink, which runs from Bedford to Brighton through London. The script leaves
ScotRail unset, since ScotRail runs both intercity and regional trains. ScotRail, and any operator
the script omits, such as London Overground or Merseyrail, keeps the route type Aubin gives it.

### Train numbers

Trains in Great Britain carry no public number. The railway knows a train by a four-character
headcode, such as `1A23`, and ticket retailers by a retail service id, such as `VT719000`. When a
trip's `trip_short_name` parses as a positive integer, the capture records it as the train number.
Neither form parses, so a capture of Great Britain records no train numbers unless Aubin's
`trip_short_name` holds a number. The German check that no train is counted twice, by agency and
train number, then has no counterpart.

### Northern Ireland

NI Railways is not part of National Rail, so neither timetable covers it. Transitous takes Northern
Ireland's timetable from busmaps, with no realtime feed. Country code GB covers Northern Ireland, so
a capture with `--country GB` polls it too, and finds no legs there.

### Timetable horizon

How far ahead Aubin's timetable runs, and how often Aubin rebuilds it, is unknown. If it runs only
days ahead, a week's capture needs fresh imports during the week:

- A failed poll logs an error and the poller carries on, so a server restart costs at most one poll.
- A run keeps the label it started with, so a fresh build named in the label needs a new run.

### Volume

National Rail runs over 20,000 passenger services a day. The German capture saw 25,996 trips on 20
October and 26,101 on 21 October, in about 400,000 rows a day. A capture of Great Britain therefore
runs to the same order of rows as Germany's.

## Disk and git

On 2026-10-23, the machine holding the German server had 13 GiB of disk free.

- **Disk.** The German server's data directory takes 26 GiB: tiles 15 GiB, the street graph
  4.5 GiB, geocoding 2.5 GiB, and the rest about 4 GiB. A server for Great Britain without tiles or
  geocoding keeps the street graph, timetable, and shapes. The street graph grows with the OSM
  extract, which for Great Britain is about two-fifths the size of Germany's.
- **Git.** A poller writing straight to bronze leaves one file per poll for git. A capture started
  once polls drain through landing leaves none.

## Recommended order

1. Ask Aubin whether a local server may poll its realtime feed every minute for a week.
2. Download Aubin's timetable, and read the dates its calendar covers.
3. Set up the server, and record the import's time, peak memory, routes matched, and segments
   beelined.
4. Capture Scotland for a few hours, and read realtime resolution from the server's log.
5. Capture Scotland from the local server and Transitous together, and compare shapes and arrivals.
6. Capture all of Great Britain for a week, once polls drain through landing.

## Approaches considered and rejected

**Polling Transitous across Great Britain.** Transitous serves Aubin's feeds with computed shapes
already. Its usage policy asks for light use, so polls of Transitous stay at one region every 10
minutes, as for Thuringia.

**Importing Transitous's processed timetable.** `api.transitous.org/gtfs/gb_great-britain.gtfs.zip`
serves it, with the shapes dropped. Mode `all` achieves the same from Aubin's own timetable, without
depending on Transitous's pipeline.

**TravelWhiz's timetable with Aubin's realtime feed.** Nothing shows the two producers' trip ids
agree. If they differ, realtime fails to resolve, as gtfs.de's realtime does against DELFI's
timetable.

## The fallback: a bridge from Darwin

If Aubin's feeds prove unusable, a bridge of lookout's own can turn Darwin's messages into GTFS-RT
for TravelWhiz's timetable. It needs:

- An account at the Rail Data Marketplace, with a subscription to Darwin Real Time Train
  Information. Both are free. The marketplace approves each account by hand, within hours to days,
  and each subscription renews yearly.
- One running consumer per set of credentials. A Kafka consumer group shares its messages out among
  its consumers, so a second consumer takes part of the feed from the first.
- New code: a Kafka consumer, a parser for Darwin's messages, and a GTFS-RT server keyed by
  `realtime_trip_id`.

The trackside project, AGPL-3.0 and begun in October 2026, consumes Darwin through the Rail Data
Marketplace. It serves its own API rather than GTFS-RT, and documents the accounts, products, and
Kafka topics. A bridge is new code, against config alone for Aubin's feeds.

## Not covered

- Vehicle positions. Darwin forecasts and records times at stops, so a train's position stays
  interpolated along its leg, as in Germany.
- Realtime for Northern Ireland.
- Buses, which the capture leaves out.

## Open questions for a first import

- Whether Aubin's realtime feed answers without a key, and whether Aubin agrees to a local server
  polling it.
- How far ahead Aubin's timetable runs, and how often Aubin rebuilds it.
- The import's time and peak memory, the data directory's size, and the routes matched and segments
  beelined.
- Realtime resolution, against the 99.96% that [motis.md](motis.md) records for Germany.
- The route type Aubin gives ScotRail and the operators the script omits, and what Aubin's
  `trip_short_name` holds.
- Which trains in Great Britain the capture leaves out as suburban, Thameslink among them.

## References

- [Transitous `feeds/gb.json`](https://github.com/public-transport/transitous/blob/f68d1781d0fd42c6da7bfcadb5624d5edda0639f/feeds/gb.json)
  — Aubin's two feeds, and how Transitous prepares the timetable
- [Transitous `scripts/gb-great-britain.lua`](https://github.com/public-transport/transitous/blob/f68d1781d0fd42c6da7bfcadb5624d5edda0639f/scripts/gb-great-britain.lua)
  — each operator's route type
- [Transitous's sources page for Great Britain](https://github.com/public-transport/transitous/blob/f68d1781d0fd42c6da7bfcadb5624d5edda0639f/website/content/sources-great-britain.md)
  — the sources and licences behind Aubin's feeds
- [Transitous feed proxy config](https://github.com/public-transport/transitous/blob/f68d1781d0fd42c6da7bfcadb5624d5edda0639f/ansible/roles/feed-proxy/templates/feed-proxy.conf.j2)
  and [its whitelist](https://github.com/public-transport/transitous/blob/f68d1781d0fd42c6da7bfcadb5624d5edda0639f/ansible/roles/feed-proxy/vars/feed-whitelist.yml)
  — caching, rate limits, and where feeds keep their secrets
- Transitous [PR #1458](https://github.com/public-transport/transitous/pull/1458),
  [PR #1974](https://github.com/public-transport/transitous/pull/1974), and
  [PR #2476](https://github.com/public-transport/transitous/pull/2476) — the move to Aubin, the
  route types, and dropping the shapes
- [TravelWhiz GB-Bus-Train-Metro-GTFS](https://github.com/travelwhiz-ltd/GB-Bus-Train-Metro-GTFS/blob/466bd95d45ad4695d7c0adf5fdad457f9badc0d0/README.md)
  — the rail timetable and `realtime_trip_id`
- [trackside, Getting the data](https://github.com/carbonarok/trackside/blob/64d8979f9ea186559429f3a094ef22ae68adfcfc/docs/data-feeds.md)
  — Rail Data Marketplace accounts, products, and Kafka topics
- [Motis `include/motis/config.h` at v2.11.3](https://github.com/motis-project/motis/blob/v2.11.3/include/motis/config.h)
  — realtime protocols, `headers`, `update_interval`, and the server's port
- [Motis `src/config.cc` at v2.11.3](https://github.com/motis-project/motis/blob/v2.11.3/src/config.cc)
  — dataset names from file names
- [Motis `src/import.cc` at v2.11.3](https://github.com/motis-project/motis/blob/v2.11.3/src/import.cc)
  — the street graph, tiles, and geocoding steps, and what `route_shapes` depends on
- [Motis scripting at v2.11.3](https://github.com/motis-project/motis/blob/v2.11.3/docs/scripting.md)
  — setting route types, and dropping routes
- [nigiri `src/loader/gtfs/route.cc`](https://github.com/motis-project/nigiri/blob/a8cdaf498336cef99dbe75cba6bdf14029371e78/src/loader/gtfs/route.cc),
  at the commit v2.11.3 pins — each route type's class
- [Geofabrik, Great Britain](https://download.geofabrik.de/europe/great-britain.html) — the OSM
  extracts
- [Facts about the National Rail network](https://britrail.com/be-inspired/national-rail-facts/) —
  passenger services a day
- [Open Rail Data wiki, National Rail feeds](https://wiki.openraildata.com/index.php/AtocIndustryData)
  — the National Rail Enquiries licence
