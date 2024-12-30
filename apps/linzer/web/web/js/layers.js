export function addLayers(map, sourceDataUrl, annotated) {
    addRegionsLayer(map, sourceDataUrl);
    // addRaysLayer(map, annotated);
    addCentroidsLayer(map, annotated);
}

function bindLayerControl(layerId, map) {
    const buttonId = `${layerId}-layer`;
    const button = document.getElementById(buttonId);
    if (!button) {
        console.error(`Button ${buttonId} not found`);
        return;
    }

    const layer = map.getLayer(layerId);
    if (!layer) {
        button.disabled = true;
        console.error(`Layer ${layerId} not found`);
        return
    }

    function updateStateBasedOnVisibility() {
        const visibility = map.getLayoutProperty(layerId, 'visibility');
        if (visibility === 'visible') {
            button.classList.add('is-success');
        }
        else {
            button.classList.remove('is-success');
        }
    }

    button.addEventListener('click', () => {    
        const visibility = map.getLayoutProperty(layerId, 'visibility');
        if (visibility === 'visible') {
            map.setLayoutProperty(layerId, 'visibility', 'none');
        }
        else {
            map.setLayoutProperty(layerId, 'visibility', 'visible');
        }
        updateStateBasedOnVisibility();
    });

    updateStateBasedOnVisibility();
    button.classList.remove('is-loading');
    button.disabled = false;
}

function addRegionsLayer(map, sourceDataUrl) {
    map.addSource('hamburg', {
        type: 'geojson',
        data: sourceDataUrl
    });

    map.addLayer({
        id: 'regions',
        type: 'line',
        source: 'hamburg',
        layout: {
            visibility: 'visible'
        },
        paint: {
            'line-color': 'black'
        }
    });

    bindLayerControl('regions', map);
}

function addCentroidsLayer(map, annotated) {
    let centroids = annotated.centroids();

    let geojsonCentroids = {
        type: 'FeatureCollection',
        features: centroids.map(({ x, y }) => ({
            type: 'Feature',
            geometry: {
                type: 'Point',
                coordinates: [ x, y ]
            }
        }))
    };

    map.addSource('centroids', {
        type: 'geojson',
        data: geojsonCentroids
    });

    map.addLayer({
        id: 'centroids',
        type: 'circle',
        source: 'centroids',
        layout: {
            visibility: 'visible'
        },
        paint: {
            'circle-color': 'blue',
            'circle-radius': 2.5
        }
    });

    bindLayerControl('centroids', map);
}

function addRaysLayer(map, annotated) {
    let rays = annotated.rays();

    let geojsonRays = {
        type: 'FeatureCollection',
        features: rays.map((polygon_rays) => ({
            type: 'Feature',
            properties: {},
            geometry: {
                type: 'MultiLineString',
                coordinates: polygon_rays.map((ray) => 
                    ray.map(({ x, y }) => [ x, y ])
                )
            }
        }))
    };
    
    map.addSource('rays', {
        type: 'geojson',
        data: geojsonRays
    });

    map.addLayer({
        id: 'rays',
        type: 'line',
        source: 'rays',
        layout: {
            visibility: 'visible'
        },
        paint: {
            'line-color': 'red'
        }
    });

    bindLayerControl('rays', map);
}

