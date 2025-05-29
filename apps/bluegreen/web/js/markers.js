
export function createMarkers(map) {
    return new Markers(map);
}

class Markers {
    constructor(map) {
        this.map = map;
        this.markersById = {};
    }

    createOrUpdateMarker(id, position) {
        if (this.markersById[id]) {
            this.markersById[id].setLngLat([position.lng, position.lat]);
        } else {
            const marker = new mapboxgl.Marker()
                .setLngLat([position.lng, position.lat])
                .addTo(this.map);
            this.markersById[id] = marker;
        }
    }
}