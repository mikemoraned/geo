import marimo

__generated_with = "0.18.4"
app = marimo.App(width="medium")


@app.cell
def _():
    return


@app.cell
def _():
    import matplotlib.pyplot as plt
    import rioxarray as rxr

    # Open and mask no-data values
    dtm = rxr.open_rasterio(
        "data/NN70SE_50CM_DTM_PHASE5.tif", masked=True
    ).squeeze()

    # Plot with matplotlib
    fig, ax = plt.subplots(figsize=(10, 8))
    dtm.plot(cmap="terrain", ax=ax)
    ax.set_title("Edinburgh LIDAR DTM")
    ax.set_axis_off()
    plt.show()

    # Histogram
    dtm.plot.hist(bins=50, color="steelblue")
    plt.xlabel("Elevation (m)")
    plt.title("Distribution of Elevation Values")
    plt.show()
    return plt, rxr


@app.cell
def _():
    from rioxarray.merge import merge_arrays
    import os
    from pathlib import Path
    return Path, merge_arrays


@app.cell
def _(Path, merge_arrays, rxr):
    # Get all your TIF files
    tif_folder = Path("data")
    tif_files = list(tif_folder.glob("*.tif"))

    # Open all rasters
    rasters = [rxr.open_rasterio(f, masked=True).squeeze() for f in tif_files]

    # Merge them into one
    merged_dtm = merge_arrays(rasters)
    return (merged_dtm,)


@app.cell
def _(plt):
    def plot_dtm(dtm):
        # Plot with matplotlib
        fig, ax = plt.subplots(figsize=(10, 8))
        dtm.plot(cmap="terrain", ax=ax)
        ax.set_title("Edinburgh LIDAR DTM")
        ax.set_axis_off()
        plt.show()

        # Histogram
        dtm.plot.hist(bins=50, color="steelblue")
        plt.xlabel("Elevation (m)")
        plt.title("Distribution of Elevation Values")
        plt.show()
    return (plot_dtm,)


@app.cell
def _(merged_dtm, plot_dtm):
    plot_dtm(merged_dtm)
    return


@app.cell
def _(merge_arrays, rxr):
    def fetch_remote_tiles(urls):
        rasters = []
        for url in urls:
            print(f"Fetching {url}")
            r = rxr.open_rasterio(url, masked=True).squeeze()
            rasters.append(r)
        print("Merging")
        merged_dtm = merge_arrays(rasters)
        return merged_dtm
    return (fetch_remote_tiles,)


@app.function
def scottish_lidar_urls(grid_tiles):
    url_prefix = "https://srsp-open-data.s3-eu-west-2.amazonaws.com/lidar/phase-5/dtm/27700/gridded/"
    url_suffix = "_50CM_DTM_PHASE5.tif"
    return [f"{url_prefix}{tile}{url_suffix}" for tile in grid_tiles]


@app.cell
def _():
    edinburgh_grid_tiles = [
        "NT27NW",
        "NT27NE",
        "NT27SW",
        "NT27SE",
        "NT26NW",
        "NT26NE",
        "NT26SW",
        "NT26SE",
    ]
    edinburgh_urls = scottish_lidar_urls(edinburgh_grid_tiles)
    print(edinburgh_urls)

    return (edinburgh_urls,)


@app.cell
def _(edinburgh_urls, fetch_remote_tiles, plot_dtm):
    edinburgh_dtm = fetch_remote_tiles(edinburgh_urls)
    plot_dtm(edinburgh_dtm)
    return


if __name__ == "__main__":
    app.run()
