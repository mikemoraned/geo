#!/bin/bash
export SECRETS_SCAN_OMIT_PATHS="web/js/settings.js"
sed "s/-TOKEN-/${PUBLIC_MAPBOX_TOKEN}/g" ./settings.js.template > ./web/js/settings.js
