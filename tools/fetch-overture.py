#!/usr/bin/env python3
"""Fetch the two public datasets the bake reads that Overpass cannot supply.

    tools/fetch-overture.py buildings --release 2026-08-19.0 \\
        --bbox 12.130 48.520 12.175 48.555 --out data/landshut_buildings.parquet
    tools/fetch-overture.py dem --bbox 12.130 48.520 12.175 48.555 --out-dir data

Both live in Amazon S3 buckets that answer plain HTTPS, so this needs no SDK,
no credentials and no region configuration -- `urllib` and `pyarrow` are the
whole dependency list.

## Buildings: Overture Maps

Overture publishes its buildings theme as GeoParquet, a few hundred files of a
gigabyte each, partitioned by nothing geographic. Reading a town out of that
the naive way is a terabyte download. What makes it a fifty-megabyte one is
that every file's footer carries min/max statistics per row group for the
`bbox` columns, and a row group whose statistics do not overlap the window
need never be fetched. So: list the bucket, read each footer with an HTTP
range request, keep the row groups whose bbox statistics touch the window,
read only those, and filter the rows. Half a gigabyte of footers and row
groups later, the parquet this writes is a megabyte.

The release string is written into the parquet's key-value metadata, together
with the window, so the bake can say where its buildings came from without
being told twice.

### Licence

The Overture buildings theme is published under ODbL 1.0 as a whole. The
footprints that come from OpenStreetMap are (c) OpenStreetMap contributors;
the ones Microsoft traced from imagery ("Microsoft ML Buildings") are (c)
Microsoft, also ODbL. Anything baked out of either is a derived database and
carries the same licence -- see CREDITS.md. The bake keeps the ML footprints
(they fill the gaps where nobody has traced a shed by hand) but it does not
believe an ML-estimated *height*: those are guesses from a shadow, and they
give St. Martin, the tallest brick tower in the world, twenty metres.

## Relief: Copernicus DEM GLO-30

The surface model the relief is baked from is the Copernicus GLO-30 DSM, one
arc-second (about thirty metres) per pixel, a tile per degree of latitude and
longitude, as cloud-optimised GeoTIFFs in a public bucket. `dem` works out
which tiles cover the window and downloads them whole -- a tile is forty
megabytes and a town needs one, occasionally two where it straddles a degree
line.

### Licence

Copernicus DEM GLO-30 (c) DLR e.V. 2010-2014 and (c) Airbus Defence and
Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA;
all rights reserved. Free to use and redistribute with that attribution,
which the bake writes into the file it produces.
"""

import argparse
import concurrent.futures
import datetime
import io
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

OVERTURE = "https://overturemaps-us-west-2.s3.us-west-2.amazonaws.com/"
COPERNICUS = "https://copernicus-dem-30m.s3.amazonaws.com/"

# The columns the bake reads, plus the ones a future bake plausibly will.
# Anything not in the file's schema is skipped rather than failed on: the
# schema has grown between releases and will again.
COLUMNS = [
    "id", "geometry", "bbox", "subtype", "class", "height", "num_floors",
    "min_height", "roof_shape", "roof_material", "roof_color",
    "facade_material", "names", "sources", "is_underground", "level",
    "min_floor",
]

ATTEMPTS = 5


def fetch(url, headers=None, method="GET", timeout=300):
    """One HTTP request, retried. S3 drops the odd range request."""
    request = urllib.request.Request(url, headers=headers or {}, method=method)
    last = None
    for attempt in range(ATTEMPTS):
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read(), response.headers
        except (urllib.error.URLError, ConnectionError, TimeoutError) as error:
            last = error
            if attempt + 1 < ATTEMPTS:
                continue
    raise RuntimeError(f"{method} {url} failed after {ATTEMPTS} attempts: {last}")


class RangeFile(io.RawIOBase):
    """A remote file that `pyarrow.parquet.ParquetFile` can seek in.

    Parquet is read back to front -- the footer says where the row groups
    are -- and pyarrow only ever asks for the bytes it needs, so a seekable
    file whose `read` is an HTTP range request is exactly the right shape.
    """

    def __init__(self, url):
        self.url = url
        self.pos = 0
        self.bytes = 0
        _, headers = fetch(url, method="HEAD", timeout=120)
        self.size = int(headers["Content-Length"])

    def readable(self):
        return True

    def seekable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, offset, whence=0):
        if whence == 0:
            self.pos = offset
        elif whence == 1:
            self.pos += offset
        else:
            self.pos = self.size + offset
        return self.pos

    def read(self, size=-1):
        if size is None or size < 0:
            size = self.size - self.pos
        if size == 0 or self.pos >= self.size:
            return b""
        end = min(self.size, self.pos + size) - 1
        data, _ = fetch(self.url, headers={"Range": f"bytes={self.pos}-{end}"})
        self.pos += len(data)
        self.bytes += len(data)
        return data

    def readinto(self, buffer):
        data = self.read(len(buffer))
        buffer[: len(data)] = data
        return len(data)


def list_keys(prefix):
    """Every object under a prefix, through S3's paginated XML listing."""
    keys, token = [], None
    while True:
        url = OVERTURE + f"?list-type=2&prefix={urllib.parse.quote(prefix)}&max-keys=1000"
        if token:
            url += f"&continuation-token={urllib.parse.quote(token)}"
        body, _ = fetch(url, timeout=120)
        text = body.decode()
        keys += re.findall(r"<Key>([^<]+)</Key>", text)
        more = re.search(r"<NextContinuationToken>([^<]+)</NextContinuationToken>", text)
        if not more:
            break
        token = more.group(1)
    return keys


def scan(key, window):
    """The rows of one file whose bbox touches the window, or none."""
    import pyarrow.compute as pc
    import pyarrow.parquet as pq

    west, south, east, north = window
    remote = RangeFile(OVERTURE + key)
    reader = pq.ParquetFile(remote)
    metadata = reader.metadata
    names = [
        metadata.row_group(0).column(i).path_in_schema
        for i in range(metadata.num_columns)
    ]
    index = {name: i for i, name in enumerate(names)}
    wanted = [c for c in COLUMNS if c in reader.schema_arrow.names]

    def bound(group, column):
        stats = group.column(index[column]).statistics
        if stats is None or not stats.has_min_max:
            return None, None
        return stats.min, stats.max

    hits = []
    for g in range(metadata.num_row_groups):
        group = metadata.row_group(g)
        xmin, _ = bound(group, "bbox.xmin")
        _, xmax = bound(group, "bbox.xmax")
        ymin, _ = bound(group, "bbox.ymin")
        _, ymax = bound(group, "bbox.ymax")
        if xmin is None or ymin is None or xmax is None or ymax is None:
            # No statistics: it has to be read to be sure.
            hits.append(g)
            continue
        if xmax < west or xmin > east or ymax < south or ymin > north:
            continue
        hits.append(g)

    tables = []
    for g in hits:
        table = reader.read_row_group(g, columns=wanted)
        bbox = table.column("bbox")
        inside = pc.and_(
            pc.and_(
                pc.greater(pc.struct_field(bbox, "xmax"), west),
                pc.less(pc.struct_field(bbox, "xmin"), east),
            ),
            pc.and_(
                pc.greater(pc.struct_field(bbox, "ymax"), south),
                pc.less(pc.struct_field(bbox, "ymin"), north),
            ),
        )
        selected = table.filter(inside)
        if selected.num_rows:
            tables.append(selected)
    return key, hits, remote.bytes, tables


def buildings(args):
    import pyarrow as pa
    import pyarrow.parquet as pq

    window = tuple(args.bbox)
    prefix = f"release/{args.release}/theme={args.theme}/type={args.type}/"
    keys = list_keys(prefix)
    if not keys:
        sys.exit(f"nothing under {OVERTURE}{prefix} -- is the release name right?")
    print(f"{len(keys)} files under {prefix}", file=sys.stderr, flush=True)

    tables, total = [], 0
    with concurrent.futures.ThreadPoolExecutor(args.workers) as pool:
        for key, hits, nbytes, rows in pool.map(lambda k: scan(k, window), keys):
            total += nbytes
            found = sum(t.num_rows for t in rows)
            if hits:
                print(
                    f"  {key.rsplit('/', 1)[-1][:24]} row groups {hits}: {found} rows",
                    file=sys.stderr,
                    flush=True,
                )
            tables += rows
    print(f"read {total / 1e6:.1f} MB", file=sys.stderr)
    if not tables:
        sys.exit("no buildings in the window")

    table = pa.concat_tables(tables, promote_options="default")
    # Provenance rides with the file, so the bake can print it and the header
    # of what it writes can name the release without being told.
    metadata = dict(table.schema.metadata or {})
    metadata.update({
        b"overture_release": args.release.encode(),
        b"overture_theme": args.theme.encode(),
        b"overture_type": args.type.encode(),
        b"bbox": json.dumps(list(window)).encode(),
        b"fetched": datetime.date.today().isoformat().encode(),
        b"licence": b"ODbL-1.0 (c) OpenStreetMap contributors, (c) Microsoft",
    })
    table = table.replace_schema_metadata(metadata)
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    pq.write_table(table, args.out)
    print(f"{table.num_rows} buildings -> {args.out}", file=sys.stderr)


def tile_name(lat, lon):
    """The Copernicus tile whose south-west corner is at (lat, lon)."""
    ns = f"N{lat:02d}" if lat >= 0 else f"S{-lat:02d}"
    ew = f"E{lon:03d}" if lon >= 0 else f"W{-lon:03d}"
    return f"Copernicus_DSM_COG_10_{ns}_00_{ew}_00_DEM"


def dem(args):
    import math

    west, south, east, north = args.bbox
    # Every tile the window touches. A tile is named by its south-west corner
    # and covers one degree, so the corners are the floors of the bounds.
    tiles = [
        tile_name(lat, lon)
        for lat in range(math.floor(south), math.floor(north) + 1)
        for lon in range(math.floor(west), math.floor(east) + 1)
    ]
    os.makedirs(args.out_dir, exist_ok=True)
    paths = []
    for name in tiles:
        path = os.path.join(args.out_dir, f"{name}.tif")
        paths.append(path)
        if os.path.exists(path) and os.path.getsize(path) > 0:
            print(f"have {path}", file=sys.stderr)
            continue
        url = f"{COPERNICUS}{name}/{name}.tif"
        print(f"fetching {url}", file=sys.stderr, flush=True)
        data, _ = fetch(url, timeout=600)
        with open(path + ".part", "wb") as out:
            out.write(data)
        os.replace(path + ".part", path)
        print(f"  {len(data) / 1e6:.1f} MB -> {path}", file=sys.stderr)
    print(
        "Copernicus DEM GLO-30 (c) DLR e.V. 2010-2014 and (c) Airbus Defence and "
        "Space GmbH 2014-2018 provided under COPERNICUS by the European Union and "
        "ESA; all rights reserved",
        file=sys.stderr,
    )
    # The paths, one per line on stdout, so a shell can hand them to the bake.
    for path in paths:
        print(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="what", required=True)

    b = sub.add_parser("buildings", help="Overture buildings in a window, as GeoParquet")
    b.add_argument("--release", required=True, help="an Overture release, e.g. 2026-08-19.0")
    b.add_argument("--bbox", nargs=4, type=float, required=True,
                   metavar=("WEST", "SOUTH", "EAST", "NORTH"))
    b.add_argument("--out", required=True)
    b.add_argument("--theme", default="buildings")
    b.add_argument("--type", default="building")
    b.add_argument("--workers", type=int, default=16,
                   help="footers read in parallel; the bucket does not mind")
    b.set_defaults(run=buildings)

    d = sub.add_parser("dem", help="Copernicus GLO-30 tiles covering a window")
    d.add_argument("--bbox", nargs=4, type=float, required=True,
                   metavar=("WEST", "SOUTH", "EAST", "NORTH"))
    d.add_argument("--out-dir", required=True)
    d.set_defaults(run=dem)

    args = ap.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
