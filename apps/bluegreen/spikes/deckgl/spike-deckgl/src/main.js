import {MapboxOverlay as DeckOverlay} from '@deck.gl/mapbox';
import {GeoJsonLayer, ArcLayer} from '@deck.gl/layers';
import mapboxgl from 'mapbox-gl';
import 'mapbox-gl/dist/mapbox-gl.css';

const MAPBOX_TOKEN = process.env.MapboxAccessToken; // eslint-disable-line

const map = new mapboxgl.Map({
  container: 'map',
  style: 'mapbox://styles/mapbox/light-v9',
  accessToken: MAPBOX_TOKEN,
  center: [4.897070, 52.377956],
  zoom: 12,
  bearing: 0,
  pitch: 30
});

const AIR_PORTS =
  'https://d2ad6b4ur7yvpq.cloudfront.net/naturalearth-3.3.0/ne_10m_airports.geojson';

const WATER_AREA = '/amsterdam/water.geojson';

const deckOverlay = new DeckOverlay({
  // interleaved: true,
  layers: [
    new GeoJsonLayer({
      id: 'airports',
      data: WATER_AREA,
      // Styles
      filled: true,
      pointRadiusMinPixels: 2,
      pointRadiusScale: 2000,
      getPointRadius: f => 11 - f.properties.scalerank,
      getFillColor: [200, 0, 80, 180],
      // Interactive props
      pickable: true,
      autoHighlight: true,
      onClick: info =>
        // eslint-disable-next-line
        info.object && alert(`${info.object.properties.name} (${info.object.properties.abbrev})`)
      // beforeId: 'waterway-label' // In interleaved mode render the layer under map labels
    }),
  ]
});

map.addControl(deckOverlay);
map.addControl(new mapboxgl.NavigationControl());
