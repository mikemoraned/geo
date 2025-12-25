#!/usr/bin/env python3
"""
Travel time client using TravelTime API to fetch journey information for city pairs.

This script reads city pairs from a parquet file, queries journey information
using the TravelTime API, and outputs results to a new parquet file.
"""

import argparse
import logging
import os
from datetime import datetime, timedelta
from typing import Dict, Any
import pandas as pd
from dotenv import load_dotenv
from traveltimepy import TravelTimeSdk, Location, Coordinates, PublicTransport
from tqdm import tqdm


# Load environment variables from .env file
load_dotenv()

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


class TravelStatus:
    """Status codes for journey search results."""
    SUCCESS = "success"
    INVALID = "invalid"
    IMPOSSIBLE = "impossible"
    TRANSIENT_ERROR = "transient_error"


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        description="Fetch train journey information for city pairs using TravelTime API"
    )
    parser.add_argument(
        "--departure_time",
        type=str,
        required=True,
        help="Departure date and time in ISO format (e.g., '2026-01-10T09:00:00')"
    )
    parser.add_argument(
        "--input",
        type=str,
        required=True,
        help="Input parquet file with city pairs"
    )
    parser.add_argument(
        "--output",
        type=str,
        required=True,
        help="Output parquet file"
    )
    return parser.parse_args()


def parse_departure_time(time_str: str) -> datetime:
    """Parse departure time string in ISO format to datetime object."""
    try:
        return datetime.fromisoformat(time_str)
    except ValueError as e:
        logger.error(f"Invalid time format: {time_str}. Expected ISO format: YYYY-MM-DDTHH:MM:SS")
        raise


def get_api_credentials() -> tuple[str, str]:
    """
    Get TravelTime API credentials from environment variables.

    Returns:
        Tuple of (app_id, api_key)

    Raises:
        ValueError: If credentials are not provided
    """
    app_id = os.environ.get('TRAVELTIME_APP_ID')
    api_key = os.environ.get('TRAVELTIME_APP_KEY')

    if not app_id or not api_key:
        raise ValueError(
            "TravelTime API credentials required. "
            "Set TRAVELTIME_APP_ID and TRAVELTIME_APP_KEY in .env file."
        )

    return app_id, api_key


def find_best_journey(
    client: TravelTimeSdk,
    lat_origin: float,
    lon_origin: float,
    lat_dest: float,
    lon_dest: float,
    start_time: datetime
) -> Dict[str, Any]:
    """
    Find the best journey between two locations using TravelTime API.

    Args:
        client: TravelTime Client instance
        lat_origin: Origin latitude
        lon_origin: Origin longitude
        lat_dest: Destination latitude
        lon_dest: Destination longitude
        start_time: Global start time for journey search

    Returns:
        Dictionary with journey information and status
    """
    result = {
        'start_time': None,
        'end_time': None,
        'duration': None,
        'distance_travelled': None,
        'status': TravelStatus.TRANSIENT_ERROR
    }

    try:
        # Create locations
        origin = Location(
            id="origin",
            coords=Coordinates(lat=lat_origin, lng=lon_origin)
        )
        destination = Location(
            id="destination",
            coords=Coordinates(lat=lat_dest, lng=lon_dest)
        )

        # Query routes using the SDK
        # Use search_ids format: {origin_id: [list of destination_ids]}
        response = client.routes(
            locations=[origin, destination],
            search_ids={"origin": ["destination"]},
            transportation=PublicTransport(),
            departure_time=start_time
        )

        # Check if we got results
        if not response or len(response) == 0:
            logger.info(f"No routes found from ({lat_origin}, {lon_origin}) to ({lat_dest}, {lon_dest})")
            result['status'] = TravelStatus.IMPOSSIBLE
            return result

        # Get the first route
        route = response[0]

        # Extract route information
        if hasattr(route, 'parts') and route.parts and len(route.parts) > 0:
            parts = route.parts

            # Get timing from parts
            # Find first and last parts with timing information
            departure_time = None
            arrival_time = None

            for part in parts:
                # Public transport parts have departure/arrival times
                if hasattr(part, 'departs_at') and part.departs_at and departure_time is None:
                    departure_time = part.departs_at
                if hasattr(part, 'arrives_at') and part.arrives_at:
                    arrival_time = part.arrives_at

            # If we couldn't find timing info, try using the overall route timing
            if departure_time is None:
                departure_time = start_time

            if arrival_time is None:
                # Calculate from travel_time if available
                if hasattr(route, 'travel_time') and route.travel_time:
                    arrival_time = departure_time + timedelta(seconds=route.travel_time)
                else:
                    logger.warning(f"Cannot determine arrival time")
                    result['status'] = TravelStatus.INVALID
                    return result

            # Check if departure is within 1 hour window
            end_time = start_time + timedelta(hours=1)
            if departure_time > end_time:
                logger.info(f"Route departs outside 1-hour window")
                result['status'] = TravelStatus.IMPOSSIBLE
                return result

            # Calculate duration in minutes
            duration_seconds = (arrival_time - departure_time).total_seconds()
            duration_minutes = duration_seconds / 60

            # Extract distance (sum from parts)
            total_distance = 0.0
            for part in parts:
                if hasattr(part, 'distance') and part.distance is not None:
                    total_distance += part.distance

            result['start_time'] = departure_time.isoformat()
            result['end_time'] = arrival_time.isoformat()
            result['duration'] = duration_minutes
            result['distance_travelled'] = total_distance / 1000.0 if total_distance > 0 else None  # Convert meters to km
            result['status'] = TravelStatus.SUCCESS

            logger.info(
                f"Found route: ({lat_origin:.4f}, {lon_origin:.4f}) -> ({lat_dest:.4f}, {lon_dest:.4f}), "
                f"duration: {duration_minutes:.1f} min"
            )
        else:
            logger.warning(f"Route has no parts or invalid structure")
            result['status'] = TravelStatus.INVALID

    except ValueError as e:
        # Invalid input data (e.g., invalid coordinates)
        logger.warning(f"Invalid input: {e}")
        result['status'] = TravelStatus.INVALID

    except Exception as e:
        # API errors and other issues
        logger.warning(f"TravelTime error: {e}")
        error_msg = str(e).lower()
        if "no results" in error_msg or "not found" in error_msg or "no route" in error_msg:
            result['status'] = TravelStatus.IMPOSSIBLE
        elif "invalid" in error_msg or "bad request" in error_msg:
            result['status'] = TravelStatus.INVALID
        else:
            result['status'] = TravelStatus.TRANSIENT_ERROR

    return result


def process_city_pairs(
    input_file: str,
    output_file: str,
    start_time: datetime,
    app_id: str,
    api_key: str
) -> None:
    """
    Process all city pairs and fetch journey information.

    Args:
        input_file: Path to input parquet file
        output_file: Path to output parquet file
        start_time: Global start time for all journey searches
        app_id: TravelTime API app ID
        api_key: TravelTime API key
    """
    # Read input parquet file
    logger.info(f"Reading input file: {input_file}")
    df = pd.read_parquet(input_file)
    logger.info(f"Found {len(df)} city pairs to process")

    # Initialize result columns
    df['start_time'] = None
    df['end_time'] = None
    df['duration'] = None
    df['distance_travelled'] = None
    df['status'] = TravelStatus.TRANSIENT_ERROR

    # Initialize TravelTime SDK
    client = TravelTimeSdk(app_id, api_key)

    # Process each city pair with progress bar
    for idx in tqdm(range(len(df)), desc="Processing city pairs"):
        row = df.iloc[idx]
        journey_info = find_best_journey(
            client=client,
            lat_origin=row['lat_origin'],
            lon_origin=row['lon_origin'],
            lat_dest=row['lat_dest'],
            lon_dest=row['lon_dest'],
            start_time=start_time
        )

        # Update dataframe with results
        df.loc[idx, 'start_time'] = journey_info['start_time']
        df.loc[idx, 'end_time'] = journey_info['end_time']
        df.loc[idx, 'duration'] = journey_info['duration']
        df.loc[idx, 'distance_travelled'] = journey_info['distance_travelled']
        df.loc[idx, 'status'] = journey_info['status']

    # Write output parquet file
    logger.info(f"Writing results to: {output_file}")
    df.to_parquet(output_file, index=False)

    # Print summary statistics
    status_counts = df['status'].value_counts()
    logger.info("=" * 50)
    logger.info("Processing complete! Summary:")
    logger.info(f"Total pairs processed: {len(df)}")
    for status, count in status_counts.items():
        logger.info(f"  {status}: {count}")
    logger.info("=" * 50)


def main():
    """Main entry point."""
    args = parse_args()

    try:
        start_time = parse_departure_time(args.departure_time)
        logger.info(f"Global start time: {start_time}")

        app_id, api_key = get_api_credentials()
        logger.info(f"Loaded TravelTime API credentials")

        process_city_pairs(
            input_file=args.input,
            output_file=args.output,
            start_time=start_time,
            app_id=app_id,
            api_key=api_key
        )

    except Exception as e:
        logger.error(f"Fatal error: {e}")
        raise


if __name__ == "__main__":
    main()
