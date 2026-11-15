# Current Slice: Spike on Maps using Kiosk display as an example

### Target

We want ultimately to show some Map context within the App itself. This will involve being able to access Map data live and/or embedded in App (if running on M5). As a way to flush out what kinds of options we have, we'll instead using the Kiosk as an example of Map usage.

So, we'll aim to set it up so that:
* When https://lookout-hom.fly.dev/kiosk/ is shown on a bigger screen (e.g. at least iPad size) then it shows a Map with the current route being replayed in it, zoomed in, so that route fills area, and with a dot which moves showing current position being replayed
  * The App itself should appear on top of this Map, perhaps in top-right, so that we can see the route and the App itself, as it would appear
* When showing on a smaller screen than iPad, it should display as it is now

### Straw Man / Spike goal

Ideally, we'd use something like https://protomaps.com (https://docs.protomaps.com/pmtiles/) as our data format and used as the basis for display. So, we'd produce a PMTile file as part of our Gold stage which both contains the data needed for replay (i.e. positions and timings) as well as anything needed for display of the map. This means that we'd be refactoring the Kiosk to get it's data from a PMTiles file.

For display of the Map itself on the page, we'd use https://maplibre.org which should be able to use the same input format. We'd use it also to do the "zoom to fit" behaviour we need.

