#!/usr/bin/env python3
import argparse
import logging
from datetime import datetime
import pandas as pd
from tqdm import tqdm
import requests
import time
from typing import Dict, Any, Optional, Tuple

# Configure logging
logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)
logger = logging.getLogger(__name__)

# API Configuration
API_BASE_URL = "https://v6.db.transport.rest"
RATE_LIMIT_DELAY = 0.6  # 100 requests per minute = 0.6 seconds between requests


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
        help="Departure date and time in ISO format (e.g., '2026-01-10T09:00:00')",
    )
    parser.add_argument(
        "--input", type=str, required=True, help="Input parquet file with city pairs"
    )
    parser.add_argument("--output", type=str, required=True, help="Output parquet file")
    return parser.parse_args()


def parse_departure_time(time_str: str) -> datetime:
    """Parse departure time string in ISO format to datetime object."""
    try:
        return datetime.fromisoformat(time_str)
    except ValueError as e:
        logger.error(
            f"Invalid time format: {time_str}. Expected ISO format: YYYY-MM-DDTHH:MM:SS"
        )
        raise


class RateLimitError(Exception):
    """Raised when API returns 429 rate limit error."""

    pass


class ServerError(Exception):
    """Raised when API returns 500+ server error."""

    pass


def query_journey(
    lat_origin: float,
    lon_origin: float,
    lat_dest: float,
    lon_dest: float,
    departure_time: datetime,
) -> Tuple[str, Optional[float], Optional[str]]:
    """
    Query the DB REST API for journey between two coordinates.

    Pre-emptively sleeps RATE_LIMIT_DELAY before making request.
    Raises fatal exceptions on 429 or 500+ errors.

    Args:
        lat_origin: Origin latitude
        lon_origin: Origin longitude
        lat_dest: Destination latitude
        lon_dest: Destination longitude
        departure_time: Departure datetime

    Returns:
        Tuple of (status, travel_time_minutes, route_description)
        - status: One of TravelStatus values
        - travel_time_minutes: Total journey time in minutes (None if not SUCCESS)
        - route_description: Human-readable route (None if not SUCCESS)

    Raises:
        RateLimitError: If API returns 429
        ServerError: If API returns 500+
    """
    # Pre-emptive rate limiting
    time.sleep(RATE_LIMIT_DELAY)

    response = None
    try:
        # Build the API request
        url = f"{API_BASE_URL}/journeys"
        params = {
            "from.latitude": lat_origin,
            "from.longitude": lon_origin,
            "to.latitude": lat_dest,
            "to.longitude": lon_dest,
            "departure": departure_time.isoformat(),
            "results": 1,  # We only need the best journey
            "stopovers": False,  # Don't need intermediate stops
        }
        logger.info(f"Params: {params}")

        # Make the API request
        response = requests.get(url, params=params, timeout=30)

        # Handle rate limiting - fatal error
        if response.status_code == 429:
            logger.error("Rate limited by API (429)")
            logger.error(f"Response body: {response.text}")
            raise RateLimitError("API returned 429 - rate limit exceeded")

        # Handle server errors - fatal error
        if response.status_code >= 500:
            logger.error(f"Server error: {response.status_code}")
            logger.error(f"Response body: {response.text}")
            raise ServerError(f"API returned {response.status_code} - server error")

        # Handle client errors (invalid request)
        if response.status_code == 400:
            logger.warning(f"Invalid request (400): {response.text}")
            return TravelStatus.INVALID, None, None

        # Raise for other bad status codes
        try:
            response.raise_for_status()
        except requests.exceptions.HTTPError as e:
            logger.error(f"HTTP error {response.status_code}: {response.text}")
            raise

        data = response.json()

        # Check if we got any journeys
        journeys = data.get("journeys", [])
        if not journeys:
            logger.debug("No journeys found between coordinates")
            return TravelStatus.IMPOSSIBLE, None, None

        # Get the first (best) journey
        journey = journeys[0]

        # Calculate total travel time
        legs = journey.get("legs", [])
        if not legs:
            return TravelStatus.IMPOSSIBLE, None, None

        # Parse departure and arrival times
        first_leg = legs[0]
        last_leg = legs[-1]

        departure_str = first_leg.get("departure")
        arrival_str = last_leg.get("arrival")

        if not departure_str or not arrival_str:
            logger.debug("Missing departure or arrival time")
            return TravelStatus.INVALID, None, None

        departure_dt = datetime.fromisoformat(departure_str.replace("Z", "+00:00"))
        arrival_dt = datetime.fromisoformat(arrival_str.replace("Z", "+00:00"))

        travel_time_minutes = (arrival_dt - departure_dt).total_seconds() / 60

        # Build route description
        route_parts = []
        for leg in legs:
            if leg.get("walking"):
                route_parts.append("Walk")
            else:
                line = leg.get("line", {})
                line_name = line.get("name", "Unknown")
                origin_name = leg.get("origin", {}).get("name", "Unknown")
                destination_name = leg.get("destination", {}).get("name", "Unknown")
                route_parts.append(f"{line_name}: {origin_name} → {destination_name}")

        route_description = " | ".join(route_parts)

        return TravelStatus.SUCCESS, travel_time_minutes, route_description

    except (RateLimitError, ServerError):
        # Re-raise fatal errors
        raise
    except requests.exceptions.Timeout:
        logger.warning("Request timeout")
        return TravelStatus.TRANSIENT_ERROR, None, None
    except requests.exceptions.RequestException as e:
        logger.warning(f"Request error: {e}")
        try:
            logger.warning(
                f"Response details: {e.response.text if hasattr(e, 'response') and e.response else 'N/A'}"
            )
        except:
            pass
        return TravelStatus.TRANSIENT_ERROR, None, None
    except (KeyError, ValueError) as e:
        logger.warning(f"Error parsing response: {e}")
        if response is not None:
            try:
                logger.warning(f"Response body: {response.text}")
            except:
                pass
        return TravelStatus.INVALID, None, None
    except Exception as e:
        logger.error(f"Unexpected error: {e}")
        import traceback

        logger.error(f"Traceback: {traceback.format_exc()}")
        return TravelStatus.TRANSIENT_ERROR, None, None


def main():
    """Main entry point."""
    args = parse_args()

    try:
        start_time = parse_departure_time(args.departure_time)
        logger.info(f"Global start time: {start_time}")

        # Load input data
        logger.info(f"Loading input from {args.input}")
        df = pd.read_parquet(args.input)
        logger.info(f"Loaded {len(df)} city pairs")

        # Initialize result columns
        results = {
            "status": [],
            "travel_time_minutes": [],
            "route": [],
        }

        # Process each city pair
        logger.info("Querying journeys from DB REST API...")
        for idx, row in tqdm(
            df.iterrows(), total=len(df), desc="Processing city pairs"
        ):
            try:
                status, travel_time, route = query_journey(
                    lat_origin=row["lat_origin"],
                    lon_origin=row["lon_origin"],
                    lat_dest=row["lat_dest"],
                    lon_dest=row["lon_dest"],
                    departure_time=start_time,
                )

                results["status"].append(status)
                results["travel_time_minutes"].append(travel_time)
                results["route"].append(route)

            except (RateLimitError, ServerError) as e:
                logger.error(f"Fatal API error at row {idx}: {e}")
                logger.error("Stopping script due to fatal error")
                raise

        # Add results to dataframe
        for col, values in results.items():
            df[col] = values

        # Save output
        logger.info(f"Saving results to {args.output}")
        df.to_parquet(args.output, index=False)

        # Log summary statistics
        success_count = sum(1 for s in results["status"] if s == TravelStatus.SUCCESS)
        invalid_count = sum(1 for s in results["status"] if s == TravelStatus.INVALID)
        impossible_count = sum(
            1 for s in results["status"] if s == TravelStatus.IMPOSSIBLE
        )
        transient_count = sum(
            1 for s in results["status"] if s == TravelStatus.TRANSIENT_ERROR
        )

        logger.info(f"Results summary:")
        logger.info(f"  SUCCESS: {success_count}")
        logger.info(f"  INVALID: {invalid_count}")
        logger.info(f"  IMPOSSIBLE: {impossible_count}")
        logger.info(f"  TRANSIENT_ERROR: {transient_count}")
        logger.info(f"Output saved to {args.output}")

    except Exception as e:
        logger.error(f"Fatal error: {e}")
        raise


if __name__ == "__main__":
    main()
