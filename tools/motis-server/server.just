MOTIS_DIR := "motis_server"
GEO_DATA_DIR := "geo_data"

create_dirs:
    mkdir -p ./{{ MOTIS_DIR }}/
    mkdir -p ./{{ GEO_DATA_DIR }}/

download_motis: create_dirs
    time wget https://github.com/motis-project/motis/releases/download/{{VERSION}}/motis-{{TARGET}}.tar.bz2
    cd {{ MOTIS_DIR }} && tar xf ../motis-{{TARGET}}.tar.bz2

# Have the import route high-speed, long-distance, night, and regional rail along OSM track, in place of the timetable's shapes. Every other class keeps the timetable's.
[working-directory: 'motis_server']
route_shapes: prerequisites
    yq -i '.timetable.route_shapes = {"mode": "all", "clasz": {"BUS": false, "COACH": false, "RIDESHARING": false, "ODM": false, "TRAM": false, "SUBWAY": false, "SUBURBAN": false, "FUNICULAR": false, "SHIP": false}}' config.yml

[working-directory: 'motis_server']
motis_server:
    ./motis server
