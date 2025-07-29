import requests
from typing import List, Optional, cast
from pystac import Collection, MediaType
from pystac_client import Client, CollectionClient
from datetime import datetime


def main():
    print("Hello from eopf-stac-python!")

    eopf_stac_api_root_endpoint = (
        "https://stac.core.eopf.eodc.eu/"  # root starting point
    )
    eopf_catalog = Client.open(
        url=eopf_stac_api_root_endpoint
    )  # calls the selected url
    print(eopf_catalog.to_dict())

    try:
        for collection in eopf_catalog.get_all_collections():
            print(collection.id)

    except Exception:
        print(
            "* [https://github.com/EOPF-Sample-Service/eopf-stac/issues/18 appears to not be resolved]"
        )

    S2l2a_coll = eopf_catalog.get_collection("sentinel-2-l2a")
    print("Keywords:        ", S2l2a_coll.keywords)
    print("Catalog ID:      ", S2l2a_coll.id)
    print("Available Links: ", S2l2a_coll.links)

    time_frame = eopf_catalog.search(  # searching the catalog
        collections="sentinel-2-l2a",
        datetime="2020-05-01T00:00:00Z/2023-05-31T23:59:59.999999Z",
    )  # the interval we are interested in, separated by '/'

    # we apply the helper function `list_found_elements`
    time_items = list_found_elements(time_frame)
    print(time_frame)

    print("Search Results:")
    print(
        "Total Items Found for Sentinel-2 L-2A between May 1, 2020, and May 31, 2023:  ",
        len(time_items[0]),
    )


def list_found_elements(search_result):
    id = []
    coll = []
    for item in search_result.items():  # retrieves the result inside the catalogue.
        id.append(item.id)
        coll.append(item.collection_id)
    return id, coll


if __name__ == "__main__":
    main()
