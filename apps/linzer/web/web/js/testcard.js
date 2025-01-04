import { mapboxProjection, bearingToSVGVector } from "./projections.js";

export function addTestCardLayer(layerName, map, svg, testcard_at_fn) {
    const project = mapboxProjection(map);

    const testCardLayer = svg.append("g")
    .attr("id", `${layerName}-layer`)
    .attr("class", "visible");

    const testCards = [];

    map.on('click', function(e) {
        const testCard = testcard_at_fn(e.lngLat.lng, e.lngLat.lat);
        testCards.push(testCard);
        console.log(`x: ${e.lngLat.lng}, y: ${e.lngLat.lat}, testCard: ${testCard.coord}, ${testCard.bearing_north_degrees}, ${testCard.bearing_east_degrees}`);
        renderTestCards();
    });

    function renderTestCards() {
        testCardLayer.selectAll("circle")
            .data(testCards)
            .enter()
            .append("circle")
            .attr("r", 10)
            .style("fill-opacity", "0%")
            .style("stroke", "red");
        testCardLayer.selectAll("circle")
            .data(testCards)
            .attr("cx", d => project(d.coord).x)
            .attr("cy", d => project(d.coord).y);

        const lineLength = 100;
        testCardLayer.selectAll("line.north")
            .data(testCards)
            .enter()
            .append("line")
            .attr("class", "north")
            .style("stroke", "red")
        testCardLayer.selectAll("line.north")
            .data(testCards)
            .attr("x1", d => project(d.coord).x)
            .attr("y1", d => project(d.coord).y)
            .attr("x2", d => {
                return project(d.coord).x + bearingToSVGVector(d.bearing_north_degrees).x * lineLength;
            })
            .attr("y2",  d => {
                return project(d.coord).y + bearingToSVGVector(d.bearing_north_degrees).y * lineLength;
            })
            ;
        testCardLayer.selectAll("text.north")
            .data(testCards)
            .enter()
            .append("text")
            .attr("class", "north")
            .text(d => `N: ${d.bearing_north_degrees.toFixed(2)}`)
            .style("fill", "red")
            .style("font-size", "12px")
            .style("font-family", "sans-serif")
            ;
        testCardLayer.selectAll("text.north")
            .data(testCards)
            .attr("x", d => project(d.coord).x + bearingToSVGVector(d.bearing_north_degrees).x * lineLength)
            .attr("y", d => project(d.coord).y + bearingToSVGVector(d.bearing_north_degrees).y * lineLength)
            ;

        testCardLayer.selectAll("line.east")
            .data(testCards)
            .enter()
            .append("line")
            .attr("class", "east")
            .style("stroke", "green")
        testCardLayer.selectAll("line.east")
            .data(testCards)
            .attr("x1", d => project(d.coord).x)
            .attr("y1", d => project(d.coord).y)
            .attr("x2", d => {
                return project(d.coord).x + bearingToSVGVector(d.bearing_east_degrees).x * lineLength;
            })
            .attr("y2",  d => {
                return project(d.coord).y + bearingToSVGVector(d.bearing_east_degrees).y * lineLength;
            })
            ;

        testCardLayer.selectAll("text.east")
            .data(testCards)
            .enter()
            .append("text")
            .attr("class", "east")
            .text(d => `E: ${d.bearing_east_degrees.toFixed(2)}`)
            .style("fill", "green")
            .style("font-size", "12px")
            .style("font-family", "sans-serif")
            ;
        testCardLayer.selectAll("text.east")
            .data(testCards)
            .attr("x", d => project(d.coord).x + bearingToSVGVector(d.bearing_east_degrees).x * lineLength)
            .attr("y", d => project(d.coord).y + bearingToSVGVector(d.bearing_east_degrees).y * lineLength)
            ;

    }

    map.on("viewreset", renderTestCards);
    map.on("move", renderTestCards);
    map.on("moveend", renderTestCards);
}