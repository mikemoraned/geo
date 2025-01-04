export function mapboxProjection(map) {
    return function project(d) {
        return map.project(new mapboxgl.LngLat(d[0], d[1]));
    }
}

export function bearingToSVGRadians(bearing) {
    const radian = Math.PI / 180;
    return bearing * radian;
}

export function bearingToSVGVector(bearing) {
    const radians = bearingToSVGRadians(bearing);
    return {
        x: Math.sin(radians),
        y: - Math.cos(radians)
    }
}