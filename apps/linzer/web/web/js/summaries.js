import { mapboxProjection, bearingToSVGVector, bearingToSVGRadians } from "./projections.js";

function bindLayerControl(layerId, svg) {
    const buttonId = `${layerId}-layer-button`;
    const button = document.getElementById(buttonId);
    if (!button) {
        console.error(`Button ${buttonId} not found`);
        return;
    }

    function updateStateBasedOnVisibility() {
        const group = svg.select(`g#${layerId}-layer`);
        const visible = group && group.classed('visible');
        if (visible) {
            button.classList.add('is-success');
        }
        else {
            button.classList.remove('is-success');
        }
    }

    button.addEventListener('click', () => {
        const group = svg.select(`g#${layerId}-layer`);
        const visible = group && group.classed('visible');
        if (visible) {
            group.attr("class", "");
        }
        else {
            group.attr("class", "visible")
        }
        updateStateBasedOnVisibility();
    });

    updateStateBasedOnVisibility();
    button.classList.remove('is-loading');
    button.disabled = false;
}

export function addSummaryLayer(layerId, map, svg, annotated) {
    const project = mapboxProjection(map);
    const summaries = annotated.summaries();

    const summariesLayer = svg.append("g")
        .attr("id", `${layerId}-layer`)
        .attr("class", "visible");

    const summaryGroups = summariesLayer
        .selectAll("g")
        .data(summaries)
        .enter()
        .append("g")
        .attr("class", "summary")
        .attr("id", d => `summary-${d.id}`);

    const dots = summaryGroups
        .append("circle")
        .attr("r", 5)
        .style("fill-opacity", "0%")
        .style("stroke", "blue");

    const summaryRadius = 100;
    const line = d3.lineRadial().curve(d3.curveLinearClosed);
    summaryGroups
        .append("path")
        .attr("stroke", "green")
        .attr("class", "normalised")
        .style("fill-opacity", "0%")
        .attr("d", function (d) {
            const data = d.normalised
                .map((length, bearing) => {
                    return {
                        radians: bearingToSVGRadians(bearing + (d.bucket_width / 2.0)),
                        length: length
                    }
                })
                .map(r => {
                    return [
                        r.radians,
                        r.length * summaryRadius,
                    ]
                });
            const path = line(data);
            return path;
        });
    summaryGroups
        .append("line")
        .attr("stroke", "green")
        .attr("class", "dominant")
        .attr("x1", 0)
        .attr("y1", 0)
        .attr("x2", function(d) {
            return bearingToSVGVector(d.dominant_degree).x * d.dominant_length * summaryRadius;
        })
        .attr("y2", function(d) {
            return bearingToSVGVector(d.dominant_degree).y * d.dominant_length * summaryRadius;
        })
        ;

    function render() {

        const previouslySelected = document.querySelectorAll(".selected");
        if (previouslySelected) {
            previouslySelected.forEach(d => d.classList.remove("selected"));
        }

        const center = map.getCenter();
        const closestId = annotated.id_of_closest_centroid(center.lng, center.lat);
        const similarIds = annotated.most_similar_ids(closestId);
        // const similarIds = [];
        const selectedIds = [ closestId ].concat(similarIds).map(id => `summary-${id}`);

        selectedIds.forEach(id => {
            const selected = document.getElementById(id);
            if (selected) {
                selected.classList.add("selected");
            }
        });

        const zoom = map.getZoom();
        const scale = (1.0 / 10000.0) * Math.pow(2, zoom);
        summaryGroups
            .attr("transform", function (d) {
                const projected = project([ d.centroid.x, d.centroid.y ]);
                return `translate(${projected.x}, ${projected.y}) scale(${scale})`;
            });
    }

    map.on("viewreset", render);
    map.on("move", render);
    map.on("moveend", render);
    render();

    bindLayerControl("summaries", svg);
}