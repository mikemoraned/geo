#!/usr/bin/env python3
"""
Travel time client using PyHafas to fetch journey information for city pairs.

This script reads city pairs from a parquet file, queries journey information
using the PyHafas API, and outputs results to a new parquet file.
"""

import argparse
import logging
from datetime import datetime, timedelta
from typing import Optional, Dict, Any
import pandas as pd
from pyhafas import HafasClient
from pyhafas.profile import DBProfile
from pyhafas.types.fptf import Leg, Journey
from tqdm import tqdm


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
        description="Fetch train journey information for city pairs"
    )
    parser.add_argument(
        "departure_time",
        type=str,
        help="Departure date and time in ISO format (e.g., '2026-01-10T09:00:00')"
    )
    parser.add_argument(
        "input",
        type=str,
        help="Input parquet file with city pairs"
    )
    parser.add_argument(
        "output",
        type=str,
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


def calculate_journey_distance(journey: Journey) -> Optional[float]:
    """
    Calculate total distance of a journey in kilometers.

    Args:
        journey: Journey object from PyHafas

    Returns:
        Total distance in km, or None if not available
    """
    try:
        total_distance = 0.0
        for leg in journey.legs:
            if hasattr(leg, 'distance') and leg.distance is not None:
                total_distance += leg.distance
        return total_distance if total_distance > 0 else None
    except Exception as e:
        logger.debug(f"Could not calculate distance: {e}")
        return None


def find_best_journey(
    client: HafasClient,
    lat_origin: float,
    lon_origin: float,
    lat_dest: float,
    lon_dest: float,
    start_time: datetime
) -> Dict[str, Any]:
    """
    Find the best journey between two locations.

    Args:
        client: HafasClient instance
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
        # Find locations near the coordinates
        origin_loc = client.locations(lat_origin, lon_origin)
        if not origin_loc:
            logger.warning(f"No location found near origin ({lat_origin}, {lon_origin})")
            result['status'] = TravelStatus.INVALID
            return result

        dest_loc = client.locations(lat_dest, lon_dest)
        if not dest_loc:
            logger.warning(f"No location found near destination ({lat_dest}, {lon_dest})")
            result['status'] = TravelStatus.INVALID
            return result

        # Use the first (closest) location for each
        origin = origin_loc[0]
        destination = dest_loc[0]

        # Search for journeys within 1 hour window
        end_time = start_time + timedelta(hours=1)

        journeys = client.journeys(
            origin=origin,
            destination=destination,
            date=start_time,
            max_journeys=5  # Get multiple options to find the best one
        )

        if not journeys:
            logger.info(f"No journeys found from {origin.name} to {destination.name}")
            result['status'] = TravelStatus.IMPOSSIBLE
            return result

        # Filter journeys that depart within the 1-hour window
        valid_journeys = [
            j for j in journeys
            if start_time <= j.departure <= end_time
        ]

        if not valid_journeys:
            logger.info(f"No journeys departing within 1 hour window")
            result['status'] = TravelStatus.IMPOSSIBLE
            return result

        # Find journey with minimum duration
        best_journey = min(valid_journeys, key=lambda j: j.duration)

        # Extract journey information
        result['start_time'] = best_journey.departure.isoformat()
        result['end_time'] = best_journey.arrival.isoformat()
        result['duration'] = best_journey.duration.total_seconds() / 60  # Duration in minutes
        result['distance_travelled'] = calculate_journey_distance(best_journey)
        result['status'] = TravelStatus.SUCCESS

        logger.info(
            f"Found journey: {origin.name} -> {destination.name}, "
            f"duration: {result['duration']:.1f} min"
        )

    except ValueError as e:
        # Invalid input data (e.g., invalid coordinates, station IDs)
        logger.warning(f"Invalid input: {e}")
        result['status'] = TravelStatus.INVALID

    except ConnectionError as e:
        # Network or API connection issues
        logger.error(f"Connection error: {e}")
        result['status'] = TravelStatus.TRANSIENT_ERROR

    except TimeoutError as e:
        # Request timeout
        logger.error(f"Timeout error: {e}")
        result['status'] = TravelStatus.TRANSIENT_ERROR

    except Exception as e:
        # Any other unexpected errors
        logger.error(f"Unexpected error: {type(e).__name__}: {e}")
        result['status'] = TravelStatus.TRANSIENT_ERROR

    return result


def process_city_pairs(
    input_file: str,
    output_file: str,
    start_time: datetime
) -> None:
    """
    Process all city pairs and fetch journey information.

    Args:
        input_file: Path to input parquet file
        output_file: Path to output parquet file
        start_time: Global start time for all journey searches
    """
    # Read input parquet file
    logger.info(f"Reading input file: {input_file}")
    df = pd.read_parquet(input_file)
    logger.info(f"Found {len(df)} city pairs to process")

    # Initialize HafasClient with German Railways (DB) profile
    client = HafasClient(DBProfile())

    # Initialize result columns
    df['start_time'] = None
    df['end_time'] = None
    df['duration'] = None
    df['distance_travelled'] = None
    df['status'] = TravelStatus.TRANSIENT_ERROR

    # Process each city pair with progress bar
    for idx, row in tqdm(df.iterrows(), total=len(df), desc="Processing city pairs"):
        journey_info = find_best_journey(
            client=client,
            lat_origin=row['lat_origin'],
            lon_origin=row['lon_origin'],
            lat_dest=row['lat_dest'],
            lon_dest=row['lon_dest'],
            start_time=start_time
        )

        # Update dataframe with results
        df.at[idx, 'start_time'] = journey_info['start_time']
        df.at[idx, 'end_time'] = journey_info['end_time']
        df.at[idx, 'duration'] = journey_info['duration']
        df.at[idx, 'distance_travelled'] = journey_info['distance_travelled']
        df.at[idx, 'status'] = journey_info['status']

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

        process_city_pairs(
            input_file=args.input,
            output_file=args.output,
            start_time=start_time
        )

    except Exception as e:
        logger.error(f"Fatal error: {e}")
        raise


if __name__ == "__main__":
    main()
