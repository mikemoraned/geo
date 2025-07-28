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


def list_found_elements(search_result):
    id = []
    coll = []
    for item in search_result.items():  # retrieves the result inside the catalogue.
        id.append(item.id)
        coll.append(item.collection_id)
    return id, coll


if __name__ == "__main__":
    main()
