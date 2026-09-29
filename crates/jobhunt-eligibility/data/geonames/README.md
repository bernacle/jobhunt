# GeoNames subset

The places Narrow resolves location text against, compiled into the
`jobhunt-eligibility` crate by
[`gazetteer`](../../src/gazetteer.rs). Nothing here is fetched when Narrow
builds, starts or ranks jobs.

## Source and license

Derived from the [GeoNames](https://www.geonames.org) gazetteer dump
(<https://download.geonames.org/export/dump/>), which is licensed under
[Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/).
The files here are an adaptation (a filtered and reformatted subset) and
stay under CC BY 4.0; Narrow's code is Apache-2.0. Redistributing them
(including inside a Narrow binary) requires the attribution: "Geographic
data © GeoNames (geonames.org), CC BY 4.0", which each file's header and
Narrow's README carry. The data is provided as is; GeoNames makes no
warranty of accuracy or completeness.

## What is included

| File | Rows | What |
| --- | --- | --- |
| `countries.tsv` | every ISO 3166 country GeoNames lists (~250) | code, English name, continent, population, every IANA zone of the country, other names |
| `admin1.tsv` | every first-level administrative region (~3,900) | GeoNames id, country, GeoNames' code, English name, the population of its listed places, their IANA zones, other names |
| `cities.tsv` | every place of more than 15,000 people, or a capital (~34,000; GeoNames' `cities15000`) | GeoNames id, country, region, population, IANA zone, name, other names |

"Other names" are the ASCII spelling and GeoNames' alternate names in
English and in each country's own languages (its `countryInfo` languages),
without historic or colloquial names, codes (IATA and the like), links, or
names outside the Latin script. So "Lisboa", "München", "Muenchen",
"Bangalore" and "Brasil" are there; "Londres" (French) and "ロンドン" are
not. About 2 MB in all.

Why this subset: it covers where people live and where jobs are posted
(down to suburbs like Menlo Park or Hayward), with the zone of each place,
while keeping the binary and the first-use load small (about 40 ms in a
release build, once per process). Smaller places are not listed: a town
under 15,000 people is resolved to its region or country when the text
names one ("Small Town, CA"), and stays unrecognized otherwise.

A country GeoNames' `timeZones.txt` gives no zone (Kosovo) takes the zones
of its listed places.

## Refreshing

```bash
./scripts/geonames/download.sh /tmp/geonames      # ~1 GB unpacked
python3 scripts/geonames/generate.py /tmp/geonames
cargo test -p jobhunt-eligibility                 # then review the diff
```

The generator is deterministic: every row and list is sorted, and each
file's header records the SHA-256 of the inputs, so the same inputs give
byte-identical files. GeoNames publishes daily, so a refresh changes some
populations and names; review the test results (ambiguity depends on
populations) and bump `RULES_VERSION` in `decision.rs`, since a refresh
can change decisions.

The inputs of the committed files are the dump of 2026-09-28.
