<script>

	import mapbox from 'mapbox-gl';
	import 'mapbox-gl/dist/mapbox-gl.css';
	const { Map } = mapbox;
	import { onMount, onDestroy } from 'svelte';

	import { PUBLIC_MAPBOX_TOKEN } from '$env/static/public';

	let map;

	const edinburgh = [-3.188267, 55.953251];
	const starting_position = {
		center: edinburgh,
		zoom: 12
	};

	onMount(async () => {
		map = new Map({
			container: "map-container",
			accessToken: PUBLIC_MAPBOX_TOKEN,
			style: `mapbox://styles/mapbox/outdoors-v11`,
			...starting_position
		});
	});

	onDestroy(() => {
		if (map) {
			map.remove();
		}
	});
</script>

<head>
	<title>geoarrow.info</title>
</head>

<div id="drop_zone">Drag and drop files here</div>
<div id="map-container"></div>

<style>
    #drop_zone {
        width: 300px;
        height: 200px;
        border: 2px dashed #007BFF;
        border-radius: 5px;
        text-align: center;
        line-height: 200px;
        color: #007BFF;
        font-family: Arial, sans-serif;
    }
    #drop_zone.highlight {
        background-color: #e9f5ff;
    }
    #map-container {
        width: 600px;
        height: 400px;
    }
</style>