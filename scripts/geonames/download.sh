#!/usr/bin/env bash
# Downloads the GeoNames dump files that scripts/geonames/generate.py reads
# into a directory of your choice (about 1 GB unpacked, mostly alternate
# names). Only needed to refresh Narrow's GeoNames subset; building,
# testing and running Narrow never download anything.
#
#   ./scripts/geonames/download.sh /tmp/geonames
#   python3 scripts/geonames/generate.py /tmp/geonames
#
# GeoNames data is licensed under CC BY 4.0: https://www.geonames.org

set -euo pipefail

if [[ $# -ne 1 ]]; then
    sed -n '2,10s/^# \{0,1\}//p' "$0" >&2
    exit 2
fi

dir=$1
base=https://download.geonames.org/export/dump
mkdir -p "$dir"
cd "$dir"
for file in countryInfo.txt timeZones.txt admin1CodesASCII.txt cities15000.zip alternateNamesV2.zip; do
    echo "fetching $file"
    curl --fail --silent --show-error --location --remote-name "$base/$file"
done
unzip -o -q cities15000.zip cities15000.txt
unzip -o -q alternateNamesV2.zip alternateNamesV2.txt
echo "done: $dir"
