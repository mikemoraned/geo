import zarr
import fsspec

ENDPOINT_URL = "https://cci-ke-o.s3-ext.jc.rl.ac.uk/"
BUCKET = "esacci"
ZARR_FILE = "ESACCI-L3C_SNOW-SWE-1979-2020-fv2.0.zarr"


def main():
    print("Hello from esa-python-uv-direct!")

    fs = fsspec.filesystem(
        "s3", anon=True, asynchronous=True, client_kwargs={"endpoint_url": ENDPOINT_URL}
    )
    store = zarr.storage.FsspecStore(fs, read_only=True, path=f"{BUCKET}/{ZARR_FILE}")
    z = zarr.open(store, mode="r")
    print("Available datasets:")
    for key in z.keys():
        print(f" - {key}")
    print(z.info_complete())


if __name__ == "__main__":
    main()
